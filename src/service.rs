//! Shared owner: commit immutable operations before scheduling provider work.
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
    Resolve {
        expected: Composer,
        operation: u64,
        accepted: bool,
    },
    Restore {
        expected: Composer,
        draft: Option<crate::composer::Draft>,
        saved: std::collections::BTreeMap<String, crate::composer::Draft>,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default)]
    pub discord_personal_qr_setup: bool,
    #[serde(default)]
    pub live: bool,
    #[serde(default)]
    pub provider_status: std::collections::BTreeMap<String, String>,
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
        Action::Resolve {
            expected,
            operation,
            accepted,
        } => {
            if &app.session.composer != expected
                || !matches!(&app.session.composer.send,SendState::Uncertain(op) if op.operation==*operation)
            {
                bail!("No matching uncertain operation");
            }
            if *accepted {
                app.accepted(*operation);
            } else {
                app.session.composer.apply(Event::NotSent(*operation));
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
    workers: crate::providers::worker::Workers,
    live: bool,
    send_running: Option<u64>,
    pub app: App,
    pub revision: u64,
    database: Database,
    pending: Option<(Instant, u64, MockOutcome)>,
}
impl Owner {
    pub fn open(mut database: Database) -> Result<Self> {
        let live = database.live;
        let (mut revision, session, settings) = database.load()?;
        let before = serde_json::to_vec(&session)?;
        let mut app = App::new(session, settings)?;
        app.live_service = live;
        app.focused = false;
        for account in &app.settings.accounts {
            app.provider_status.insert(
                account.id.clone(),
                if account.enabled {
                    "Connecting"
                } else {
                    "Disabled"
                }
                .into(),
            );
        }
        if serde_json::to_vec(&app.session)? != before {
            revision = database.commit(revision, &app.session, &app.settings, "recovered")?;
        }
        Ok(Self {
            workers: crate::providers::worker::Workers::default(),
            live,
            send_running: None,
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
            discord_personal_qr_setup: true,
            live: self.live,
            provider_status: self.app.provider_status.clone(),
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
        staged.live_service = self.live;
        for action in actions {
            if self.live {
                match action {
                    Action::Incoming | Action::Reconcile { .. } => {
                        bail!("Simulated arrivals and reconciliation are disabled in live mode")
                    }
                    Action::Compose {
                        event: Event::Attach(_),
                        ..
                    } => bail!(
                        "Live attachments are not implemented; metadata-only attachments cannot be sent"
                    ),
                    Action::Compose {
                        event: Event::Submit,
                        ..
                    } => {
                        let draft = staged
                            .session
                            .composer
                            .draft
                            .as_ref()
                            .context("Write a reply first")?;
                        let account = staged
                            .settings
                            .accounts
                            .iter()
                            .find(|a| {
                                a.id == draft.target.account && a.provider == draft.target.provider
                            })
                            .context("Account removed; preserve draft and reconnect")?;
                        if !account.enabled || !account.allow_send {
                            bail!("Enable the account and allow_send before sending");
                        }
                        if !draft.attachments.is_empty() {
                            bail!("Remove unsupported live attachments before sending");
                        }
                        let item = self
                            .database
                            .remote(draft.target.incoming)
                            .context("Original message unavailable")?;
                        if item.account_scope != crate::providers::scope(account)
                            || item.message(draft.target.incoming, account).target.as_ref()
                                != Some(&draft.target)
                        {
                            bail!("Reply identity changed; draft retained");
                        }
                    }
                    Action::Compose {
                        event: Event::Discard(target),
                        ..
                    } => {
                        if let Some(target) = target
                            && !staged
                                .session
                                .messages
                                .iter()
                                .any(|m| m.target.as_ref() == Some(target))
                        {
                            bail!("Invalid discard target");
                        }
                    }
                    _ => {}
                }
            }
            if let Some(operation) = apply(&mut staged, action)? {
                let Action::Compose { outcome, .. } = action else {
                    bail!("invalid mock submission");
                };
                if !self.live {
                    pending = Some((
                        Instant::now() + Duration::from_millis(650),
                        operation.operation,
                        *outcome,
                    ));
                }
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
        staged.decision_job = self.app.decision_job.take();
        staged.decision_attempted = std::mem::take(&mut self.app.decision_attempted);
        staged.provider_status = std::mem::take(&mut self.app.provider_status);
        if self.live {
            staged
                .provider_status
                .retain(|id, _| staged.settings.accounts.iter().any(|a| &a.id == id));
            for account in &staged.settings.accounts {
                if !account.enabled {
                    staged
                        .provider_status
                        .insert(account.id.clone(), "Disabled".into());
                } else if !self.app.settings.accounts.iter().any(|a| a == account) {
                    staged.provider_status.insert(
                        account.id.clone(),
                        "Connecting with updated settings".into(),
                    );
                }
            }
        }
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
    pub fn refresh(&mut self) {
        self.workers.refresh();
    }
    pub fn with_transport(mut self, factory: crate::providers::worker::Factory) -> Self {
        self.workers = crate::providers::worker::Workers::new(factory);
        self
    }
    fn ingest(
        &mut self,
        account: &crate::policy::AccountConfig,
        batch: crate::providers::Batch,
    ) -> Result<()> {
        let mut staged = App::connected(self.app.session.clone(), self.app.settings.clone())?;
        staged.live_service = true;
        staged.focused = self.app.focused;
        staged.selected = self.app.selected;
        staged.new_items = self.app.new_items;
        staged.feed_offset = self.app.feed_offset;
        staged.provider_status = self.app.provider_status.clone();
        let anchor = self.app.visible().get(self.app.feed_offset).map(|m| m.id);
        for item in batch.items {
            if item.account_scope != crate::providers::scope(account) {
                bail!("Provider returned a mismatched account identity");
            }
            let known = if let Some(id) = self.database.remote_id(&item)? {
                self.database.imported(id)?
            } else {
                false
            };
            let id = self.database.store_remote(&item)?;
            let item = self.database.remote(id)?;
            if item.deleted {
                if let Some(message) = staged.session.messages.iter_mut().find(|m| m.id == id) {
                    message.target = None;
                    message.incoming = false;
                    if !message.subject.starts_with("[Unavailable] ") {
                        message.subject = format!("[Unavailable] {}", message.subject);
                    }
                }
                continue;
            }
            if let Some(existing) = staged.session.messages.iter_mut().find(|m| m.id == id) {
                let unread = existing.unread;
                *existing = item.message(id, account);
                existing.unread = unread;
                if let Some(unread) = item.unread {
                    staged.receive_unread(id, unread);
                }
            } else {
                // Poll flag windows must not repopulate an older, evicted notification.
                if known || item.partial {
                    continue;
                }
                if staged.session.messages.len() >= 200 {
                    let pinned = |id| {
                        staged.selected == Some(id)
                            || staged
                                .session
                                .composer
                                .target
                                .as_ref()
                                .is_some_and(|t| t.incoming == id)
                            || staged
                                .session
                                .saved
                                .values()
                                .any(|d| d.target.incoming == id)
                            || staged.session.read_changes.contains_key(&id)
                    };
                    if let Some(old) = staged
                        .session
                        .messages
                        .iter()
                        .filter(|m| !pinned(m.id))
                        .min_by_key(|m| m.timestamp)
                        .map(|m| m.id)
                    {
                        staged.session.messages.retain(|m| m.id != old);
                        staged.session.message_state.remove(&old);
                    } else {
                        self.app.provider_status.insert(
                            account.id.clone(),
                            "Sync paused: feed is full of drafts or pending read changes".into(),
                        );
                        return Ok(());
                    }
                }
                staged
                    .session
                    .message_state
                    .insert(id, self.database.local_state(id)?);
                staged.session.messages.push(item.message(id, account));
                staged.new_items += 1;
            }
        }
        staged
            .session
            .sync_cursors
            .insert(crate::providers::scope(account), batch.cursor);
        if serde_json::to_vec(&staged.session)?.len() > crate::ipc::MAX_FRAME / 2 {
            self.app.provider_status.insert(
                account.id.clone(),
                "Sync paused: snapshot size limit reached; draft data preserved".into(),
            );
            return Ok(());
        }
        staged
            .provider_status
            .insert(account.id.clone(), batch.status);
        if !staged.focused {
            staged.follow_latest();
        }
        staged.refresh_filters();
        if staged.focused
            && let Some(anchor) = anchor
            && let Some(index) = staged.visible().iter().position(|m| m.id == anchor)
        {
            staged.feed_offset = index;
        }
        staged.decision_job = self.app.decision_job.take();
        staged.decision_attempted = std::mem::take(&mut self.app.decision_attempted);
        self.app = staged;
        Ok(())
    }
    fn tick_live(&mut self) -> Result<()> {
        use crate::providers::{SendResult, worker::Outcome};
        for result in self.workers.collect() {
            let current = self
                .app
                .settings
                .accounts
                .iter()
                .any(|a| *a == result.account && a.enabled);
            match result.outcome {
                Outcome::Send(operation, outcome) => {
                    if matches!(&self.app.session.composer.send,SendState::Submitting(op) if op.operation==operation)
                    {
                        match outcome {
                            SendResult::Accepted => self.app.accepted(operation),
                            SendResult::Failed(error) => {
                                self.app
                                    .session
                                    .composer
                                    .apply(Event::Failed(operation, error));
                            }
                            SendResult::Uncertain => {
                                self.app.session.composer.apply(Event::Uncertain(operation));
                            }
                        }
                        self.app.provider_status.insert(result.account.id.clone(),match &self.app.session.composer.send {
                            SendState::Uncertain(_)=>"Send outcome uncertain · check original client before resolving".into(),
                            SendState::Failed(error)=>error.clone(),
                            _=>"Reply accepted by provider · delivery is not guaranteed".into(),
                        });
                        self.revision = self.database.commit(
                            self.revision,
                            &self.app.session,
                            &self.app.settings,
                            "live_send_result",
                        )?;
                    }
                    self.send_running = None;
                }
                Outcome::Read(change, result) if current => {
                    self.app.acknowledge_read(
                        change.message,
                        change.revision,
                        result.map_err(|e| e.to_string()),
                    );
                    self.revision = self.database.commit(
                        self.revision,
                        &self.app.session,
                        &self.app.settings,
                        "remote_read_result",
                    )?;
                }
                Outcome::Sync(sync_result) if current => {
                    match sync_result {
                        Ok(batch) => {
                            self.ingest(&result.account, batch)?;
                        }
                        Err(error) => {
                            self.app.provider_status.insert(
                                result.account.id.clone(),
                                crate::domain::safe_text(&error.to_string()),
                            );
                        }
                    }
                    self.revision = self.database.commit(
                        self.revision,
                        &self.app.session,
                        &self.app.settings,
                        "provider_sync",
                    )?;
                }
                _ => {}
            }
        }
        if self.app.poll_background() {
            self.revision = self.database.commit(
                self.revision,
                &self.app.session,
                &self.app.settings,
                "decision_result",
            )?;
        }
        let send = if self.send_running.is_none() {
            if let SendState::Submitting(op) = &self.app.session.composer.send {
                let ready = self.app.settings.accounts.iter().any(|a| {
                    a.enabled
                        && a.allow_send
                        && a.id == op.draft.target.account
                        && a.provider == op.draft.target.provider
                });
                if !ready {
                    let id = op.operation;
                    self.app.session.composer.apply(Event::Failed(
                        id,
                        "Account disabled or removed before submission".into(),
                    ));
                    self.revision = self.database.commit(
                        self.revision,
                        &self.app.session,
                        &self.app.settings,
                        "send_validation_failed",
                    )?;
                    return Ok(());
                }
                match self.database.remote(op.draft.target.incoming) {
                    Ok(item) => Some((item, op.clone())),
                    Err(_) => {
                        let operation = op.operation;
                        self.app.session.composer.apply(Event::Failed(
                            operation,
                            "Original message unavailable".into(),
                        ));
                        self.revision = self.database.commit(
                            self.revision,
                            &self.app.session,
                            &self.app.settings,
                            "send_validation_failed",
                        )?;
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };
        let operation = send.as_ref().map(|(_, op)| op.operation);
        if self.workers.schedule(
            &self.app.settings.accounts,
            &self.app.session.sync_cursors,
            &self.app.session.read_changes,
            send,
        ) {
            self.send_running = operation;
        }
        Ok(())
    }
    pub fn tick(&mut self) -> Result<()> {
        if self.live {
            return self.tick_live();
        }
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
