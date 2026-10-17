use crate::{
    composer::{Composer, Draft, Event, SendState, Submission},
    config::Settings,
    domain::{Focus, Message, MessageId},
    editor::Editor,
    fixtures,
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub schema_version: u32,
    pub messages: Vec<Message>,
    pub composer: Composer,
    pub saved: BTreeMap<String, Draft>,
    pub arrivals: u64,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            schema_version: 1,
            messages: fixtures::messages(),
            composer: Composer::default(),
            saved: BTreeMap::new(),
            arrivals: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Insert,
    Navigate,
    Command,
    Search,
    Help,
    Settings,
    Discard,
    Reader,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MockOutcome {
    Success,
    Failure,
    Uncertain,
}

pub struct App {
    pub session: Session,
    pub settings: Settings,
    pub selected: Option<MessageId>,
    pub focused: bool,
    pub mode: Mode,
    pub editor: Editor,
    pub command: Editor,
    pub query: String,
    pub notice: String,
    pub new_items: usize,
    pub feed_offset: usize,
    pub setting_index: usize,
    pub next_outcome: MockOutcome,
    pub reader: Option<Message>,
    pub reader_scroll: u16,
    pub reader_query: String,
}

impl App {
    pub fn new(mut session: Session, settings: Settings) -> Result<Self> {
        if session.schema_version != 1 {
            bail!("unsupported demo state schema {}", session.schema_version);
        }
        session.composer.recover();
        let editor = Editor::new(session.composer.body());
        let mut app = Self {
            session,
            settings,
            selected: None,
            focused: true,
            mode: Mode::Insert,
            editor,
            command: Editor::default(),
            query: String::new(),
            notice: "Offline demo · every send is simulated.  Esc then ? for controls.".into(),
            new_items: 0,
            feed_offset: 0,
            setting_index: 0,
            next_outcome: MockOutcome::Success,
            reader: None,
            reader_scroll: 0,
            reader_query: String::new(),
        };
        app.follow_latest();
        if let Some(target) = app.session.composer.reply_target() {
            app.selected = Some(target.incoming);
        }
        if !app.settings.insert_on_focus {
            app.mode = Mode::Navigate;
        }
        Ok(app)
    }

    pub fn visible(&self) -> Vec<&Message> {
        let query = self.query.to_lowercase();
        let mut messages: Vec<_> = self
            .session
            .messages
            .iter()
            .filter(|m| {
                m.matches(self.settings.focus)
                    && (query.is_empty()
                        || format!("{} {} {}", m.sender, m.subject, m.body)
                            .to_lowercase()
                            .contains(&query))
            })
            .collect();
        messages.sort_by_key(|m| std::cmp::Reverse((m.timestamp, m.id)));
        messages
    }

    pub fn selected_message(&self) -> Option<&Message> {
        self.session
            .messages
            .iter()
            .find(|m| Some(m.id) == self.selected)
    }

    pub fn latest(&self) -> Option<&Message> {
        // Search only changes the viewport; the active focus determines follow eligibility.
        self.session
            .messages
            .iter()
            .filter(|m| m.eligible(self.settings.focus))
            .max_by_key(|m| (m.timestamp, m.id))
    }

    pub fn follow_latest(&mut self) {
        if self.session.composer.draft.is_some() {
            return;
        }
        let message = self.latest().cloned();
        self.selected = message.as_ref().map(|m| m.id);
        self.session
            .composer
            .apply(Event::Select(message.and_then(|m| m.target)));
        self.feed_offset = 0;
    }

    pub fn focus(&mut self, focused: bool) {
        self.focused = focused;
        if !focused {
            self.follow_latest();
        } else if self.settings.insert_on_focus
            && self.session.composer.draft.is_none()
            && matches!(self.mode, Mode::Insert | Mode::Navigate)
        {
            self.mode = Mode::Insert;
        }
    }

    pub fn select(&mut self, id: MessageId) {
        let target = self
            .session
            .messages
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| m.target.clone());
        self.selected = Some(id);
        if self.session.composer.draft.is_none()
            && let Some(target) = &target
        {
            self.session.composer.draft = self.session.saved.remove(&target.draft_key());
        }
        self.session.composer.apply(Event::Select(target));
        self.sync_editor();
    }

    pub fn move_selection(&mut self, delta: isize, rows: usize) {
        let messages = self.visible();
        if messages.is_empty() {
            return;
        }
        let current = messages
            .iter()
            .position(|m| Some(m.id) == self.selected)
            .unwrap_or(0);
        let index = current.saturating_add_signed(delta).min(messages.len() - 1);
        let id = messages[index].id;
        self.select(id);
        self.feed_offset = self
            .feed_offset
            .min(index)
            .max(index.saturating_sub(rows.saturating_sub(1)));
    }

    pub fn set_focus(&mut self, focus: Focus) {
        self.settings.focus = focus;
        self.feed_offset = 0;
        if !self.focused {
            self.follow_latest();
        } else if self.session.composer.draft.is_none()
            && !self.session.composer.reply_target().is_some_and(|target| {
                self.session
                    .messages
                    .iter()
                    .any(|m| m.id == target.incoming && m.eligible(focus))
            })
        {
            self.session.composer.apply(Event::Select(None));
        }
        // Never redirect a focused empty composer; clear an ineligible target instead.
    }

    pub fn arrive(&mut self) {
        if self.session.arrivals >= 10_000 {
            self.notice = "Fixture arrival limit reached.".into();
            return;
        }
        let anchor = self.visible().get(self.feed_offset).map(|m| m.id);
        self.session.arrivals += 1;
        self.session
            .messages
            .push(fixtures::arrival(self.session.arrivals));
        if !self.focused {
            self.follow_latest();
        } else {
            self.new_items += 1;
            if let Some(id) = anchor {
                let index = self.visible().iter().position(|m| m.id == id);
                if let Some(index) = index {
                    self.feed_offset = index;
                }
            }
        }
        self.notice = "Simulated incoming message · composer context preserved.".into();
    }

    pub fn update_body(&mut self) {
        if self.has_saved_target() {
            self.sync_editor();
            self.notice =
                "A saved draft exists here · explicitly select the conversation to resume it."
                    .into();
            return;
        }
        self.session
            .composer
            .apply(Event::Body(self.editor.text.clone()));
        if self.session.composer.draft.is_none() {
            self.sync_editor();
            self.notice = "Select a reply-capable message first.".into();
        }
    }

    pub fn sync_editor(&mut self) {
        if self.editor.text != self.session.composer.body() {
            self.editor = Editor::new(self.session.composer.body());
        }
    }

    pub fn attach(&mut self) {
        if self.has_saved_target() {
            self.notice =
                "A saved draft exists here · explicitly select the conversation to resume it."
                    .into();
            return;
        }
        self.session
            .composer
            .apply(Event::Attach(fixtures::attachment()));
        self.notice = "Added harmless fixture attachment (no file access or upload).".into();
    }

    fn has_saved_target(&self) -> bool {
        self.session.composer.draft.is_none()
            && self
                .session
                .composer
                .reply_target()
                .is_some_and(|t| self.session.saved.contains_key(&t.draft_key()))
    }

    pub fn submit(&mut self) -> Option<Submission> {
        let target = self.session.composer.reply_target()?;
        if !self
            .session
            .messages
            .iter()
            .any(|m| m.incoming && m.target.as_ref() == Some(target))
        {
            self.notice =
                "Source unavailable · draft retained; select a valid context with save-and-switch."
                    .into();
            return None;
        }
        self.session.composer.apply(Event::Submit)
    }

    pub fn discard(&mut self) {
        let target = self.selected_message().and_then(|m| m.target.clone());
        self.session.composer.apply(Event::Discard(target));
        self.sync_editor();
        self.mode = Mode::Navigate;
    }

    pub fn save_switch(&mut self) {
        if !self.session.composer.editable() {
            self.notice = "Resolve the pending send before switching drafts.".into();
            return;
        }
        let Some(target) = self.selected_message().and_then(|m| m.target.clone()) else {
            self.notice = "Select a reply-capable message first.".into();
            return;
        };
        let key = target.draft_key();
        if let Some(draft) = self.session.composer.draft.take() {
            self.session.saved.insert(draft.target.draft_key(), draft);
        }
        self.session.composer.target = Some(target);
        self.session.composer.draft = self.session.saved.remove(&key);
        self.session.composer.send = SendState::Ready;
        self.sync_editor();
        self.notice = "Previous draft saved · selected conversation ready.".into();
    }

    pub fn open_reader(&mut self) {
        self.reader = self.selected_message().cloned();
        self.reader_scroll = 0;
        self.reader_query.clear();
        if self.reader.is_some() {
            self.mode = Mode::Reader;
        }
    }
}
