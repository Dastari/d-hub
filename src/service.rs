//! Offline-only service reducer. No credentials, provider sessions or live sends.
use crate::{
    app::{App, MockOutcome, Session},
    composer::{Composer, Event, SendState, Submission},
    config::Settings,
    database::{Change, Database},
    domain::MessageId,
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Select {
        id: MessageId,
        prepare_reply: bool,
    },
    Focus {
        focused: bool,
    },
    Park {
        expected: Composer,
    },
    Compose {
        expected: Composer,
        event: Event,
        outcome: MockOutcome,
    },
    Switch {
        expected: Composer,
        id: MessageId,
    },
    Unread {
        id: MessageId,
        unread: bool,
    },
    Dismiss {
        id: MessageId,
        dismissed: bool,
    },
    Configure {
        before: Settings,
        settings: Settings,
    },
    Incoming,
    Reconcile {
        operation: u64,
    },
    Restore {
        expected: Composer,
        draft: Option<crate::composer::Draft>,
        saved: std::collections::BTreeMap<String, crate::composer::Draft>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub revision: u64,
    pub session: Session,
    pub settings: Settings,
    pub selected: Option<MessageId>,
    pub focused: bool,
    pub editor_connected: bool,
    pub changes: Vec<Change>,
    pub resync: bool,
}

/// Used by both the owner and optimistic UI replay; has no transport side effects.
pub fn apply(app: &mut App, action: &Action) -> Result<Option<Submission>> {
    match action {
        Action::Select { id, prepare_reply } => {
            if !app.session.messages.iter().any(|m| m.id == *id) {
                bail!("message no longer exists");
            }
            if *prepare_reply {
                app.select(*id);
            } else {
                app.selected = Some(*id);
                if app.settings.mark_read_on_select {
                    app.set_unread(*id, false);
                }
            }
        }
        Action::Focus { focused } => app.focus(*focused),
        Action::Park { expected } => {
            if &app.session.composer != expected {
                bail!("composer changed; stale park rejected");
            }
            app.park_reply();
        }
        Action::Compose {
            expected, event, ..
        } => {
            if &app.session.composer != expected {
                bail!("composer changed; stale edit rejected");
            }
            match event {
                Event::Body(body) if body.len() <= 65_536 => {
                    app.editor = crate::editor::Editor::new(body);
                    app.update_body();
                }
                Event::Attach(attachment)
                    if app
                        .session
                        .composer
                        .draft
                        .as_ref()
                        .is_none_or(|d| d.attachments.len() < 16) =>
                {
                    app.session
                        .composer
                        .apply(Event::Attach(attachment.clone()));
                }
                Event::RemoveAttachment(index) => {
                    app.session.composer.apply(Event::RemoveAttachment(*index));
                }
                Event::Discard(target) => {
                    app.session.composer.apply(Event::Discard(target.clone()));
                }
                Event::Submit => return Ok(app.submit()),
                _ => bail!("unsupported composer action or content limit exceeded"),
            }
            app.sync_editor();
        }
        Action::Switch { expected, id } => {
            if &app.session.composer != expected {
                bail!("composer changed; stale draft switch rejected");
            }
            app.switch_reply(*id);
        }
        Action::Unread { id, unread } => app.set_unread(*id, *unread),
        Action::Dismiss { id, dismissed } => {
            app.session.message_state.entry(*id).or_default().dismissed = *dismissed;
            app.refresh_filters();
        }
        Action::Configure { before, settings } => {
            if &app.settings != before {
                bail!("settings changed; stale configuration edit rejected");
            }
            settings.validate()?;
            app.settings = settings.clone();
            app.refresh_filters();
        }
        Action::Incoming => app.arrive(),
        Action::Restore {
            expected,
            draft,
            saved,
        } => {
            if &app.session.composer != expected || !app.session.composer.editable() {
                bail!("resolve the current send before restoring drafts");
            }
            for recovered in draft.iter().chain(saved.values()) {
                if recovered.body.len() > 65_536
                    || recovered.attachments.len() > 16
                    || crate::domain::safe_text(&recovered.body) != recovered.body
                    || !app
                        .session
                        .messages
                        .iter()
                        .any(|m| m.target.as_ref() == Some(&recovered.target))
                {
                    bail!("recovered draft has an invalid target or exceeds content limits");
                }
            }
            if let Some(recovered) = draft
                && app
                    .session
                    .composer
                    .draft
                    .as_ref()
                    .is_some_and(|current| current != recovered)
            {
                bail!("current draft differs; save or discard it explicitly before recovery");
            }
            for (key, recovered) in saved {
                if *key != recovered.target.draft_key()
                    || app.session.saved.get(key).is_some_and(|d| d != recovered)
                {
                    bail!("a saved draft conflicts with recovery");
                }
            }
            app.session.saved.extend(saved.clone());
            if let Some(recovered) = draft {
                app.session.composer.target = Some(recovered.target.clone());
                app.session.composer.draft = Some(recovered.clone());
                app.selected = Some(recovered.target.incoming);
            }
            app.sync_editor();
        }
        Action::Reconcile { operation } => {
            if !matches!(&app.session.composer.send, SendState::Uncertain(op) if op.operation == *operation)
            {
                bail!("no matching uncertain mock operation");
            }
            app.accepted(*operation);
        }
    }
    Ok(None)
}

pub struct Owner {
    pub app: App,
    pub revision: u64,
    database: Database,
    pending: Option<(Instant, u64, MockOutcome)>,
}
impl Owner {
    pub fn open(mut database: Database) -> Result<Self> {
        let (mut revision, session, settings) = database.load()?;
        let before = serde_json::to_vec(&session)?;
        let app = App::new(session, settings)?;
        if serde_json::to_vec(&app.session)? != before {
            revision = database.commit(revision, &app.session, &app.settings, "recovered")?;
        }
        Ok(Self {
            app,
            revision,
            database,
            pending: None,
        })
    }
    pub fn snapshot(&self, after: u64, editor_connected: bool) -> Result<Snapshot> {
        let changes = self.database.changes(after)?;
        let resync = after > self.revision
            || (after != 0
                && changes
                    .first()
                    .is_some_and(|c| c.revision > after.saturating_add(1)));
        Ok(Snapshot {
            revision: self.revision,
            session: self.app.session.clone(),
            settings: self.app.settings.clone(),
            selected: self.app.selected,
            focused: self.app.focused,
            editor_connected,
            changes,
            resync,
        })
    }
    pub fn actions(&mut self, expected: u64, actions: &[Action]) -> Result<()> {
        if expected != self.revision {
            bail!("stale service revision");
        }
        if actions.is_empty() || actions.len() > 256 {
            bail!("action batch must contain 1–256 actions");
        }
        let mut staged = App::connected(self.app.session.clone(), self.app.settings.clone())?;
        staged.selected = self.app.selected;
        staged.focused = self.app.focused;
        let mut pending = self.pending;
        for action in actions {
            if let Some(operation) = apply(&mut staged, action)? {
                let Action::Compose { outcome, .. } = action else {
                    bail!("invalid mock submission");
                };
                pending = Some((
                    Instant::now() + Duration::from_millis(650),
                    operation.operation,
                    *outcome,
                ));
            }
        }
        if serde_json::to_vec(&staged.session)?.len() > crate::ipc::MAX_FRAME / 2 {
            bail!("service session limit reached");
        }
        self.revision = self.database.commit(
            self.revision,
            &staged.session,
            &staged.settings,
            "interaction",
        )?;
        self.app = staged;
        self.pending = pending;
        Ok(())
    }
    pub fn read(&mut self, id: MessageId) -> Result<()> {
        self.app
            .session
            .messages
            .iter()
            .find(|m| m.id == id)
            .context("unknown message")?;
        if self.app.settings.mark_read_on_select
            && self
                .app
                .session
                .messages
                .iter()
                .any(|m| m.id == id && m.unread)
        {
            let mut staged = App::connected(self.app.session.clone(), self.app.settings.clone())?;
            staged.set_unread(id, false);
            self.revision = self.database.commit(
                self.revision,
                &staged.session,
                &self.app.settings,
                "message_seen",
            )?;
            self.app.session = staged.session;
        }
        Ok(())
    }
    pub fn tick(&mut self) -> Result<()> {
        if let Some((when, operation, outcome)) = self.pending
            && Instant::now() >= when
        {
            let mut staged = App::connected(self.app.session.clone(), self.app.settings.clone())?;
            staged.selected = self.app.selected;
            staged.focused = self.app.focused;
            match outcome {
                MockOutcome::Success => staged.accepted(operation),
                MockOutcome::Failure => {
                    staged.session.composer.apply(Event::Failed(
                        operation,
                        "Simulated transport failure".into(),
                    ));
                }
                MockOutcome::Uncertain => {
                    staged.session.composer.apply(Event::Uncertain(operation));
                }
            }
            self.revision = self.database.commit(
                self.revision,
                &staged.session,
                &staged.settings,
                "mock_send_result",
            )?;
            self.app = staged;
            self.pending = None;
        }
        Ok(())
    }
}
