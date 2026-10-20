use crate::{
    composer::{Composer, Draft, Event, SendState, Submission},
    config::Settings,
    domain::{Focus, Message, MessageId, SourceFilter},
    editor::Editor,
    fixtures, policy,
    read_state::{MessageState, ReadChange, ReadTransport},
};
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod settings_page;
pub use settings_page::{NewAccount, SettingsPage, SettingsSection};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    #[serde(default)]
    pub sync_cursors: BTreeMap<String, serde_json::Value>,
    pub schema_version: u32,
    pub messages: Vec<Message>,
    pub composer: Composer,
    pub saved: BTreeMap<String, Draft>,
    pub arrivals: u64,
    #[serde(default)]
    pub message_state: BTreeMap<MessageId, MessageState>,
    #[serde(default)]
    pub read_changes: BTreeMap<MessageId, ReadChange>,
    #[serde(default)]
    pub notification_ids: std::collections::BTreeSet<String>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            schema_version: 1,
            sync_cursors: BTreeMap::new(),
            messages: fixtures::messages(),
            composer: Composer::default(),
            saved: BTreeMap::new(),
            arrivals: 0,
            message_state: BTreeMap::new(),
            read_changes: BTreeMap::new(),
            notification_ids: std::collections::BTreeSet::new(),
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
    EditSetting,
    Secret,
    DiscordQr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MockOutcome {
    Success,
    Failure,
    Uncertain,
}

pub type PendingDecision = (
    MessageId,
    String,
    std::sync::mpsc::Receiver<Result<policy::Classification, String>>,
);

pub struct App {
    pub shared: bool,
    pub update_requested: bool,
    pub update_status: Option<crate::update::Status>,
    pub live_service: bool,
    /// The terminal reports modified keys, so Shift+Enter differs from Enter.
    pub modified_keys: bool,
    pub service_supports_discord_qr: bool,
    pub provider_status: BTreeMap<String, String>,
    pub service_socket: Option<std::path::PathBuf>,
    pub actions: Vec<crate::service::Action>,
    pub session: Session,
    pub settings: Settings,
    pub selected: Option<MessageId>,
    pub focused: bool,
    pub mode: Mode,
    pub editor: Editor,
    pub command: Editor,
    pub command_origin: Mode,
    pub(crate) command_completion: Option<crate::tui::commands::Completion>,
    pub query: String,
    pub notice: String,
    pub new_items: usize,
    pub feed_offset: usize,
    /// Selection the feed last scrolled into view; wheel scrolling leaves it alone.
    pub feed_revealed: Option<MessageId>,
    pub setting_index: usize,
    pub settings_scroll: usize,
    pub settings_page_size: usize,
    pub settings_page: SettingsPage,
    /// Page settings were opened at; Esc there returns to the drawer.
    pub settings_entry: SettingsPage,
    pub new_account: NewAccount,
    pub next_outcome: MockOutcome,
    pub reader: Option<Message>,
    pub reader_scroll: u16,
    /// PageUp/PageDown offset of the preview pane from its default position, kept only while
    /// the same message stays selected.
    pub preview_scroll: (Option<MessageId>, isize),
    /// When the current notice first appeared; notices clear themselves after a few seconds.
    pub notice_since: Option<(String, std::time::Instant)>,
    /// Lines per page and the offsets the last drawn preview could reach.
    pub preview_bounds: (usize, isize, isize),
    pub reader_query: String,
    pub edit_key: Option<String>,
    pub secret_ref: Option<String>,
    pub discord_setup: Option<crate::discord_auth::Setup>,
    pub auth_job: Option<std::sync::mpsc::Receiver<String>>,
    /// Latest connection check per account, shown on its settings page.
    pub account_checks: BTreeMap<String, Vec<crate::providers::check::Step>>,
    pub check_job: Option<(
        String,
        std::sync::mpsc::Receiver<Vec<crate::providers::check::Step>>,
    )>,
    pub live_setup: bool,
    pub live_feed: bool,
    pub bridge_ready: bool,
    pub notifications: Option<std::sync::mpsc::Receiver<crate::discord_notifications::Event>>,
    pub decision_job: Option<PendingDecision>,
    pub decision_attempted: std::collections::BTreeSet<String>,
}

impl App {
    pub fn new(mut session: Session, settings: Settings) -> Result<Self> {
        if session.schema_version != 1 {
            bail!("unsupported demo state schema {}", session.schema_version);
        }
        session.composer.recover();
        let mut saved = BTreeMap::new();
        for draft in session.saved.into_values() {
            let key = draft.target.draft_key();
            if let Some(existing) = saved.get(&key)
                && existing != &draft
            {
                bail!("conflicting saved drafts; original snapshot preserved");
            }
            saved.insert(key, draft);
        }
        session.saved = saved;
        if settings.dismiss_after_reply {
            for sent in &session.composer.sent {
                let id = sent.draft.target.incoming;
                if let std::collections::btree_map::Entry::Vacant(entry) =
                    session.message_state.entry(id)
                {
                    entry.insert(MessageState {
                        seen: true,
                        dismissed: true,
                        ..MessageState::default()
                    });
                    if let Some(message) = session.messages.iter_mut().find(|m| m.id == id) {
                        message.unread = false;
                    }
                }
            }
        }
        let editor = Editor::new(session.composer.body());
        let mut app = Self {
            shared: false,
            update_requested: false,
            update_status: None,
            live_service: false,
            modified_keys: false,
            service_supports_discord_qr: false,
            provider_status: BTreeMap::new(),
            service_socket: None,
            actions: vec![],
            session,
            settings,
            selected: None,
            focused: true,
            mode: Mode::Insert,
            editor,
            command: Editor::default(),
            command_origin: Mode::Navigate,
            command_completion: None,
            query: String::new(),
            notice: "Offline demo · every send is simulated.  Esc then ? for controls.".into(),
            new_items: 0,
            feed_offset: 0,
            feed_revealed: None,
            setting_index: 0,
            settings_scroll: 0,
            settings_page_size: 1,
            settings_page: SettingsPage::Home,
            settings_entry: SettingsPage::Home,
            new_account: NewAccount::default(),
            next_outcome: MockOutcome::Success,
            reader: None,
            reader_scroll: 0,
            preview_scroll: (None, 0),
            notice_since: None,
            preview_bounds: (1, 0, 0),
            reader_query: String::new(),
            edit_key: None,
            secret_ref: None,
            discord_setup: None,
            auth_job: None,
            account_checks: BTreeMap::new(),
            check_job: None,
            live_setup: false,
            live_feed: false,
            bridge_ready: false,
            notifications: None,
            decision_job: None,
            decision_attempted: std::collections::BTreeSet::new(),
        };
        app.follow_latest();
        if let Some(target) = app.session.composer.reply_target() {
            app.selected = Some(target.incoming);
        }
        if !app.settings.insert_on_focus {
            app.mode = Mode::Navigate;
        }
        app.reconcile_filters();
        Ok(app)
    }

    /// Client snapshots must not recover an in-flight operation or run follow logic.
    pub fn connected(session: Session, settings: Settings) -> Result<Self> {
        let mut app = Self::new(session.clone(), settings)?;
        app.session = session;
        app.sync_editor();
        Ok(app)
    }
    fn record(&mut self, action: crate::service::Action) {
        if self.shared {
            self.actions.push(action);
        }
    }
    pub fn compose(&mut self, event: Event) -> Option<Submission> {
        self.record(crate::service::Action::Compose {
            expected: self.session.composer.clone(),
            event: event.clone(),
            outcome: self.next_outcome,
        });
        self.session.composer.apply(event)
    }
    pub fn remove_attachment(&mut self, index: usize) {
        self.compose(Event::RemoveAttachment(index));
    }

    pub fn visible(&self) -> Vec<&Message> {
        let query = self.query.to_lowercase();
        let mut messages: Vec<_> = self
            .session
            .messages
            .iter()
            .filter(|m| {
                self.profile_matches(m)
                    && m.matches_source(self.settings.source)
                    && (self.settings.show_dismissed || !self.dismissed(m.id))
                    && (self.settings.show_spam || !self.spam_match(m))
                    && !self.hold_pending(m)
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
        // Search only changes the viewport; source and focus determine follow eligibility.
        self.session
            .messages
            .iter()
            .filter(|m| self.eligible(m))
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
        self.record(crate::service::Action::Focus { focused });
        self.focused = focused;
        if !focused {
            self.follow_latest();
        } else if !self.live_feed
            && !self.live_setup
            && self.settings.insert_on_focus
            && self.session.composer.draft.is_none()
            && matches!(self.mode, Mode::Insert | Mode::Navigate)
        {
            self.mode = Mode::Insert;
        }
    }

    pub fn select(&mut self, id: MessageId) {
        self.record(crate::service::Action::Select {
            id,
            prepare_reply: self.session.composer.draft.is_none(),
        });
        let target = self
            .session
            .messages
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| m.target.clone());
        if self.selected != Some(id) {
            self.preview_scroll = (None, 0);
        }
        self.selected = Some(id);
        if self.session.composer.draft.is_none()
            && let Some(target) = &target
        {
            self.session.composer.draft = self.session.saved.remove(&target.draft_key());
        }
        self.session.composer.apply(Event::Select(target));
        self.sync_editor();
        self.mark_seen(id);
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
        self.set_profile(focus.label());
    }

    pub fn set_profile(&mut self, id: &str) {
        let before = self.settings.clone();
        if let Err(error) = self.settings.set("focus", id) {
            self.notice = error.to_string();
            return;
        }
        self.record(crate::service::Action::Configure {
            before,
            settings: self.settings.clone(),
        });
        self.refresh_filters();
        let name = crate::domain::safe_text(self.settings.profile_label());
        self.notice = if id == "all" {
            "No focus · everything shows, filtered messages labelled · f picks a focus.".into()
        } else if self.settings.ai.enabled {
            format!("{name} focus · hides spam and what the {name} prompt rules out · f next.")
        } else {
            format!(
                "{name} focus · hides spam; its prompt needs AI decisions (:auth ai, ai.enabled) · f next."
            )
        };
    }

    pub fn set_source(&mut self, source: SourceFilter) {
        let before = self.settings.clone();
        self.settings.source = source;
        self.record(crate::service::Action::Configure {
            before,
            settings: self.settings.clone(),
        });
        self.refresh_filters();
        self.notice = format!(
            "Source: {} · Left/Right cycles sources; f changes focus.",
            source.label()
        );
    }

    /// Keeps selection and an empty composer consistent with the feed after any change,
    /// including background ones; never touches a draft with content.
    pub(crate) fn reconcile_filters(&mut self) {
        self.feed_offset = 0;
        if !self.focused {
            self.follow_latest();
        } else if self.session.composer.draft.is_none()
            && !self.session.composer.reply_target().is_some_and(|target| {
                self.session
                    .messages
                    .iter()
                    .any(|m| m.id == target.incoming && self.eligible(m))
            })
        {
            self.session.composer.apply(Event::Select(None));
        }
        if !self.visible().iter().any(|m| Some(m.id) == self.selected) {
            // Browsing a filtered feed is distinct from selecting a reply target.
            self.selected = self.visible().first().map(|m| m.id);
        }
        // Never redirect a focused empty composer; clear an ineligible target instead.
    }

    pub fn arrive(&mut self) {
        self.record(crate::service::Action::Incoming);
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
        if self.settings.reply_first
            && self.session.composer.draft.is_none()
            && (self.session.composer.reply_target().is_none() || self.has_saved_target())
            && let Some(id) = self.selected
            && self.selected_message().is_some_and(|m| m.target.is_some())
        {
            let input = self.editor.clone();
            self.switch_reply(id);
            if self.session.composer.draft.is_some() {
                self.editor.insert(&input.text);
            } else {
                self.editor = input;
            }
        }
        if self.has_saved_target() {
            self.sync_editor();
            self.notice =
                "A saved draft exists here · explicitly select the conversation to resume it."
                    .into();
            return;
        }
        self.compose(Event::Body(self.editor.text.clone()));
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
        self.compose(Event::Attach(fixtures::attachment()));
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
        self.compose(Event::Submit)
    }

    pub fn discard(&mut self) {
        let target = self.selected_message().and_then(|m| m.target.clone());
        self.compose(Event::Discard(target));
        self.sync_editor();
        self.mode = Mode::Navigate;
    }

    pub fn save_switch(&mut self) {
        if let Some(id) = self.selected {
            self.switch_reply(id);
        }
    }

    /// Explicit navigation saves the old message draft before selecting the next target.
    pub fn switch_reply(&mut self, id: MessageId) {
        if !self.session.composer.editable() {
            self.notice = "Finish or reconcile the pending send before switching replies.".into();
            return;
        }
        let Some(message) = self.session.messages.iter().find(|m| m.id == id) else {
            return;
        };
        let target = message.target.clone();
        self.record(crate::service::Action::Switch {
            expected: self.session.composer.clone(),
            id,
        });
        // Switching is explicit, so an emptied draft is released rather than saved.
        if let Some(draft) = self.session.composer.draft.take()
            && !draft.is_empty()
        {
            self.session.saved.insert(draft.target.draft_key(), draft);
        }
        self.session.composer.target = target.clone();
        self.session.composer.draft = target
            .as_ref()
            .and_then(|t| self.session.saved.remove(&t.draft_key()))
            .filter(|d| !d.is_empty());
        self.session.composer.send = SendState::Ready;
        self.selected = Some(id);
        self.mark_seen(id);
        self.sync_editor();
        self.mode = Mode::Insert;
        self.notice = if target.is_some() { "Reply ready · previous message’s draft saved · arrows switch messages/sources, Alt+arrows edit text." } else { "Previous reply saved · this item has no reply capability." }.into();
    }

    pub fn park_reply(&mut self) {
        if !self.session.composer.editable() {
            return;
        }
        self.record(crate::service::Action::Park {
            expected: self.session.composer.clone(),
        });
        // Switching is explicit, so an emptied draft is released rather than saved.
        if let Some(draft) = self.session.composer.draft.take()
            && !draft.is_empty()
        {
            self.session.saved.insert(draft.target.draft_key(), draft);
        }
        self.session.composer.target = None;
        self.session.composer.send = SendState::Ready;
        self.selected = None;
        self.sync_editor();
        self.mode = Mode::Insert;
    }
    pub fn cycle_reply_source(&mut self, backwards: bool) {
        if !self.session.composer.editable() {
            self.notice = "Finish or reconcile the pending send before switching replies.".into();
            return;
        }
        self.set_source(self.settings.source.cycle(backwards));
        if let Some(id) = self.selected {
            self.switch_reply(id);
        } else {
            self.park_reply();
        }
    }

    pub fn move_reply(&mut self, delta: isize, rows: usize) {
        let visible = self.visible();
        if visible.is_empty() {
            return;
        }
        let current = visible
            .iter()
            .position(|m| Some(m.id) == self.selected)
            .unwrap_or(0);
        let index = current.saturating_add_signed(delta).min(visible.len() - 1);
        let id = visible[index].id;
        self.switch_reply(id);
        self.feed_offset = self
            .feed_offset
            .min(index)
            .max(index.saturating_sub(rows.saturating_sub(1)));
    }

    pub fn open_reader(&mut self) {
        if let Some(id) = self.selected {
            self.mark_seen(id);
        }
        self.reader = self.selected_message().cloned();
        self.reader_scroll = 0;
        self.reader_query.clear();
        if self.reader.is_some() {
            self.mode = Mode::Reader;
        }
    }

    /// Clears a notice once it has been visible for a few seconds, so the status bar returns.
    /// Returns true when the screen needs redrawing.
    pub fn expire_notice(&mut self) -> bool {
        const VISIBLE: std::time::Duration = std::time::Duration::from_secs(8);
        if self.notice.is_empty() {
            self.notice_since = None;
            return false;
        }
        match &self.notice_since {
            Some((text, since)) if *text == self.notice => {
                if since.elapsed() < VISIBLE {
                    return false;
                }
                self.notice.clear();
                self.notice_since = None;
                true
            }
            _ => {
                self.notice_since = Some((self.notice.clone(), std::time::Instant::now()));
                false
            }
        }
    }

    /// Pages the preview of the selected message; a new selection starts at its default view.
    pub fn scroll_preview(&mut self, pages: isize) {
        let (page, low, high) = self.preview_bounds;
        let offset = self.preview_offset();
        let offset = offset.saturating_add(pages.saturating_mul(page.max(1) as isize));
        self.preview_scroll = (self.selected, offset.clamp(low.min(0), high.max(0)));
    }

    pub fn preview_offset(&self) -> isize {
        match self.preview_scroll {
            (Some(id), offset) if Some(id) == self.selected => offset,
            _ => 0,
        }
    }

    /// Delete with no reply started: dismiss the selection and mark it read. Never restores.
    /// A draft emptied of text and attachments is released first; Delete is the explicit act.
    pub fn dismiss_read(&mut self) {
        let Some(id) = self.selected else {
            return;
        };
        if self.dismissed(id) {
            self.notice = "Already dismissed · e restores it.".into();
            return;
        }
        let ids: Vec<_> = self.burst(id).iter().map(|m| m.id).collect();
        if self
            .session
            .composer
            .draft
            .as_ref()
            .is_some_and(|d| d.is_empty())
            && self.session.composer.editable()
        {
            let target = self.selected_message().and_then(|m| m.target.clone());
            self.compose(Event::Discard(target));
            self.sync_editor();
        }
        self.toggle_dismissed();
        if ids.iter().all(|id| self.dismissed(*id)) {
            let unread: Vec<_> = self
                .session
                .messages
                .iter()
                .filter(|m| m.unread && ids.contains(&m.id))
                .map(|m| m.id)
                .collect();
            for id in unread {
                self.set_unread(id, false);
            }
        }
    }

    pub fn dismissed(&self, id: MessageId) -> bool {
        self.session
            .message_state
            .get(&id)
            .is_some_and(|s| s.dismissed)
    }

    pub fn classification(&self, message: &Message) -> Option<&policy::Classification> {
        self.session
            .message_state
            .get(&message.id)
            .and_then(|s| s.classification.as_ref())
            .filter(|c| c.fingerprint == policy::fingerprint(message, &self.settings))
    }

    pub fn spam_match(&self, message: &Message) -> bool {
        self.filtered(message).is_some()
    }

    /// Why the feed hides a message, if it does. Filtering applies only in a focus profile;
    /// with focus off the feed shows everything, labelled by `filter_rule`.
    pub fn filtered(&self, message: &Message) -> Option<Filtered> {
        if self.settings.focus == "all" {
            return None;
        }
        self.filter_rule(message)
    }

    /// Which filter matches a message, whatever the focus. Local rules apply at once; decision
    /// answers apply when a current result exists. Unclassified messages stay visible (unless
    /// `ai.hold_pending`), so a failed or slow decision never loses anything.
    pub fn filter_rule(&self, message: &Message) -> Option<Filtered> {
        let spam = &self.settings.spam;
        if !spam.enabled {
            return None;
        }
        if spam.no_reply && message.no_reply() {
            return Some(Filtered::NoReply);
        }
        if spam.no_reply && message.signals.automated {
            return Some(Filtered::Automated);
        }
        if policy::matches_terms(message, &spam.local_terms) {
            return Some(Filtered::Terms);
        }
        let c = self.classification(message)?;
        if c.spam >= spam.threshold {
            return Some(Filtered::Promotion);
        }
        let below = |p: Option<f64>| p.is_some_and(|p| p < spam.for_you_threshold);
        if spam.for_you && below(c.person) {
            return Some(Filtered::NotPerson);
        }
        if spam.for_you && below(c.for_you) {
            return Some(Filtered::NotForYou);
        }
        None
    }

    /// Why the feed is empty, in order: what is hidden and how to show it. Empty when nothing
    /// has arrived for this source and focus at all.
    pub fn empty_reasons(&self) -> Vec<String> {
        let in_view: Vec<_> = self
            .session
            .messages
            .iter()
            .filter(|m| m.matches_source(self.settings.source) && self.profile_matches(m))
            .collect();
        let count = |f: &dyn Fn(&Message) -> bool| in_view.iter().filter(|m| f(m)).count();
        let mut reasons = vec![];
        let dismissed = count(&|m| self.dismissed(m.id));
        if dismissed > 0 && !self.settings.show_dismissed {
            reasons.push(format!("{dismissed} dismissed · D shows them"));
        }
        let filtered = count(&|m| !self.dismissed(m.id) && self.filtered(m).is_some());
        if filtered > 0 && !self.settings.show_spam {
            reasons.push(format!("{filtered} filtered · S shows them"));
        }
        let held = count(&|m| !self.dismissed(m.id) && self.hold_pending(m));
        if held > 0 {
            reasons.push(format!(
                "{held} waiting for AI decisions · ai.hold_pending holds them"
            ));
        }
        if !self.query.is_empty() {
            reasons.push(format!(
                "searching \"{}\" · / then clear it to see all",
                crate::domain::safe_text(&self.query)
            ));
        }
        let in_source = self
            .session
            .messages
            .iter()
            .filter(|m| m.matches_source(self.settings.source))
            .count();
        let outside_focus = in_source - in_view.len();
        if outside_focus > 0 {
            reasons.push(format!(
                "{outside_focus} outside {} focus · f changes focus",
                crate::domain::safe_text(self.settings.profile_label())
            ));
        }
        let elsewhere = self.session.messages.len() - in_source;
        if elsewhere > 0 {
            reasons.push(format!("{elsewhere} in other sources · ←→ changes source"));
        }
        reasons
    }

    /// How many messages the current source and focus hide through filtering.
    pub fn filtered_count(&self) -> usize {
        self.session
            .messages
            .iter()
            .filter(|m| {
                m.incoming
                    && m.matches_source(self.settings.source)
                    && self.profile_matches(m)
                    && !self.dismissed(m.id)
                    && self.filtered(m).is_some()
            })
            .count()
    }

    /// Whether a message belongs to the current focus profile. Messages without a decision
    /// (AI off, pending or failed) stay in every profile unless the profile has terms, so a
    /// focus never hides mail just because nobody has judged it yet.
    pub fn profile_matches(&self, message: &Message) -> bool {
        if self.settings.focus == "all" {
            return true;
        }
        if let Some(c) = self.classification(message) {
            return c
                .profiles
                .get(&self.settings.focus)
                .is_none_or(|p| *p >= self.settings.ai.threshold);
        }
        let Some(profile) = self
            .settings
            .profiles
            .iter()
            .find(|p| p.id == self.settings.focus)
        else {
            return true;
        };
        if !profile.terms.is_empty() {
            return policy::matches_terms(message, &profile.terms);
        }
        // Demo fixtures carry a work/personal tag; live messages are untagged.
        message.focus == crate::domain::Focus::All
            || profile
                .fixture_focus
                .is_none_or(|focus| message.matches(focus))
    }

    fn eligible(&self, message: &Message) -> bool {
        message.incoming
            && message.target.is_some()
            && self.profile_matches(message)
            && message.matches_source(self.settings.source)
            && !self.dismissed(message.id)
            // Even with focus off, the composer never follows a filtered arrival on its own.
            && self.filter_rule(message).is_none()
            && !self.hold_pending(message)
    }

    /// The run of adjacent visible messages sharing `id`'s feed heading, newest first.
    pub fn stack(&self, id: MessageId) -> Vec<&Message> {
        let visible = self.visible();
        let Some(index) = visible.iter().position(|m| m.id == id) else {
            return self
                .session
                .messages
                .iter()
                .filter(|m| m.id == id)
                .collect();
        };
        let key = visible[index].stack_key();
        let start = visible[..index]
            .iter()
            .rposition(|m| m.stack_key() != key)
            .map_or(0, |i| i + 1);
        let end = visible[index..]
            .iter()
            .position(|m| m.stack_key() != key)
            .map_or(visible.len(), |i| index + i);
        visible[start..end].to_vec()
    }

    /// What is read and dismissed as one: a chat stack, or a single email.
    pub fn burst(&self, id: MessageId) -> Vec<&Message> {
        let stack = self.stack(id);
        if stack.iter().any(|m| m.is_mail()) {
            stack.into_iter().filter(|m| m.id == id).collect()
        } else {
            stack
        }
    }

    /// Viewing a message shows its whole chat stack, so selection marks the stack read.
    pub fn mark_seen(&mut self, id: MessageId) {
        if !self.settings.mark_read_on_select {
            return;
        }
        let unread: Vec<_> = self
            .burst(id)
            .iter()
            .filter(|m| m.unread || m.id == id)
            .map(|m| m.id)
            .collect();
        for id in unread {
            self.set_unread(id, false);
        }
    }

    /// Dismisses the selection without replying (a whole chat burst, or one email) and moves to
    /// the next message. Restores instead when the selection is already dismissed.
    pub fn toggle_dismissed(&mut self) {
        let Some(id) = self.selected else {
            return;
        };
        let dismissed = !self.dismissed(id);
        let burst = self.burst(id);
        let sender = burst
            .first()
            .map(|m| crate::domain::display_name(&m.sender))
            .unwrap_or_default();
        let ids: Vec<_> = burst
            .iter()
            .map(|m| m.id)
            .filter(|id| self.dismissed(*id) != dismissed)
            .collect();
        // Only a reply with content blocks dismissal; an emptied one is released below.
        if dismissed
            && let Some(draft) = &self.session.composer.draft
            && !draft.is_empty()
            && ids.contains(&draft.target.incoming)
        {
            self.notice =
                "Reply in progress · send it, or Esc then d to discard it, before dismissing."
                    .into();
            return;
        }
        // Continue below the burst (older), else above it, so reading stays in place.
        let next = (dismissed && !self.settings.show_dismissed)
            .then(|| {
                let visible = self.visible();
                let index = visible.iter().position(|m| m.id == id)?;
                visible[index..]
                    .iter()
                    .chain(visible[..index].iter().rev())
                    .find(|m| !ids.contains(&m.id))
                    .map(|m| m.id)
            })
            .flatten();
        for &id in &ids {
            self.record(crate::service::Action::Dismiss { id, dismissed });
            self.session.message_state.entry(id).or_default().dismissed = dismissed;
        }
        self.refresh_filters();
        if let Some(next) = next {
            if self.settings.reply_first
                && self.mode == Mode::Insert
                && self.session.composer.draft.is_none()
            {
                self.switch_reply(next);
            } else {
                self.select(next);
            }
        }
        let what = match ids.len() {
            1 => format!("message from {sender}"),
            n => format!("{n} messages from {sender}"),
        };
        self.notice = if dismissed {
            format!("Dismissed {what} · D or :dismissed shows dismissed items.")
        } else {
            format!("Restored {what}.")
        };
    }

    pub fn toggle_show_dismissed(&mut self) {
        self.settings.show_dismissed = !self.settings.show_dismissed;
        self.refresh_filters();
        self.notice = if self.settings.show_dismissed {
            "Dismissed messages included · e restores the selected item."
        } else {
            "Dismissed messages hidden."
        }
        .into();
    }

    /// After an explicit filter change (source, focus, dismissal, D, S, settings): reconcile,
    /// then in reply-first mode an empty composer replies to the selected visible message, or
    /// to nothing when the feed is empty. Background changes use `reconcile_filters` alone.
    pub fn refresh_filters(&mut self) {
        // An explicit filter change releases a draft emptied of text and attachments, as
        // Delete and save-and-switch do; background changes never release one.
        if self.session.composer.editable()
            && self
                .session
                .composer
                .draft
                .as_ref()
                .is_some_and(|d| d.is_empty())
        {
            self.session.composer.draft = None;
            self.sync_editor();
        }
        self.reconcile_filters();
        if self.settings.reply_first
            && self.session.composer.draft.is_none()
            && self.session.composer.editable()
        {
            let target = self
                .selected
                .filter(|id| self.visible().iter().any(|m| m.id == *id))
                .and_then(|id| self.session.messages.iter().find(|m| m.id == id))
                .and_then(|m| m.target.clone());
            self.session.composer.apply(Event::Select(target));
        }
    }

    pub fn accepted(&mut self, operation: u64) {
        let count = self.session.composer.sent.len();
        self.session.composer.apply(Event::Accepted(operation));
        if self.session.composer.sent.len() > count
            && let Some(sent) = self.session.composer.sent.last()
        {
            let id = sent.draft.target.incoming;
            // The reply answers its message and the older ones stacked with it; anything newer
            // that arrived meanwhile stays unread and in the feed.
            let answered: Vec<_> = self
                .burst(id)
                .iter()
                .map(|m| m.id)
                .skip_while(|m| *m != id)
                .collect();
            for &id in &answered {
                self.set_unread(id, false);
                if self.settings.dismiss_after_reply {
                    self.session.message_state.entry(id).or_default().dismissed = true;
                }
            }
            self.reconcile_filters();
        }
        self.sync_editor();
    }

    pub fn set_unread(&mut self, id: MessageId, unread: bool) {
        self.record(crate::service::Action::Unread { id, unread });
        let Some(message) = self.session.messages.iter_mut().find(|m| m.id == id) else {
            return;
        };
        let changed = message.unread != unread;
        message.unread = unread;
        let state = self.session.message_state.entry(id).or_default();
        state.seen = !unread;
        if !changed {
            return;
        }
        state.revision += 1;
        if let Some(remote) = &message.remote {
            self.session.read_changes.insert(
                id,
                ReadChange {
                    account_scope: message.account_scope.clone(),
                    message: id,
                    remote: remote.clone(),
                    unread,
                    revision: state.revision,
                    error: None,
                },
            );
        }
    }

    pub fn receive_unread(&mut self, id: MessageId, unread: bool) {
        if self.session.read_changes.contains_key(&id) {
            return;
        }
        if let Some(message) = self.session.messages.iter_mut().find(|m| m.id == id) {
            message.unread = unread;
            self.session.message_state.entry(id).or_default().seen = !unread;
        }
    }

    pub fn acknowledge_read(&mut self, id: MessageId, revision: u64, result: Result<(), String>) {
        if let Some(change) = self.session.read_changes.get_mut(&id)
            && change.revision == revision
        {
            match result {
                Ok(()) => {
                    self.session.read_changes.remove(&id);
                }
                Err(error) => change.error = Some(crate::domain::safe_text(&error)),
            }
        }
    }

    pub fn sync_read_changes(&mut self, transport: &mut impl ReadTransport) {
        for change in self
            .session
            .read_changes
            .values()
            .cloned()
            .collect::<Vec<_>>()
        {
            let result = transport.set_unread(&change.remote, change.unread);
            self.acknowledge_read(change.message, change.revision, result);
        }
    }

    pub fn apply_classification(&mut self, id: MessageId, result: policy::Classification) -> bool {
        let Some(message) = self.session.messages.iter().find(|m| m.id == id) else {
            return false;
        };
        if result.fingerprint != policy::fingerprint(message, &self.settings)
            || !policy::probability(result.spam)
            || [result.person, result.for_you]
                .into_iter()
                .flatten()
                .any(|p| !policy::probability(p))
            || result.profiles.values().any(|p| !policy::probability(*p))
        {
            return false;
        }
        let state = self.session.message_state.entry(id).or_default();
        state.classification = Some(result);
        state.classification_error = None;
        self.reconcile_filters();
        true
    }

    fn hold_pending(&self, message: &Message) -> bool {
        (self.live_feed || self.live_service)
            && self.settings.ai.hold_pending
            && self.classification(message).is_none()
            && self
                .session
                .message_state
                .get(&message.id)
                .is_none_or(|s| s.classification_error.is_none())
            && crate::decisions::DecisionJob::build(message, &self.settings).is_ok()
    }

    pub fn ingest_notification(
        &mut self,
        event: &crate::discord_notifications::Notification,
    ) -> bool {
        if self.session.notification_ids.contains(&event.event_id) {
            return false;
        }
        if self.session.messages.len() >= 2000 {
            self.notice = "Notification snapshot limit reached; capture paused. Retain/export messages before restarting with a new profile.".into();
            return false;
        }
        let id = MessageId(
            self.session
                .messages
                .iter()
                .map(|m| m.id.0)
                .max()
                .unwrap_or(1_000_000)
                .saturating_add(1),
        );
        let timestamp = crate::discord_notifications::now().max(
            self.session
                .messages
                .iter()
                .map(|m| m.timestamp)
                .max()
                .unwrap_or(0),
        );
        let message = self
            .settings
            .accounts
            .iter()
            .find_map(|a| crate::discord_notifications::into_message(a, event, id, timestamp));
        let Some(message) = message else {
            return false;
        };
        let anchor = self.visible().get(self.feed_offset).map(|m| m.id);
        self.session.messages.push(message);
        self.session.notification_ids.insert(event.event_id.clone());
        if !self.focused {
            self.follow_latest();
        } else {
            self.new_items += 1;
            if let Some(id) = anchor
                && let Some(index) = self.visible().iter().position(|m| m.id == id)
            {
                self.feed_offset = index;
            }
        }
        self.notice = "Discord notification captured · local seen/dismissal only · v opens the official client.".into();
        true
    }

    pub fn poll_background(&mut self) -> bool {
        let mut changed = false;
        if let Some(receiver) = self.auth_job.take() {
            loop {
                match receiver.try_recv() {
                    Ok(notice) => {
                        self.notice = notice;
                        changed = true;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        self.auth_job = Some(receiver);
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                }
            }
        }
        if let Some((id, receiver)) = self.check_job.take() {
            match receiver.try_recv() {
                Ok(steps) => {
                    let failed = steps.iter().filter(|s| !s.ok).count();
                    self.notice = if failed == 0 {
                        format!("Connection check passed for {id}.")
                    } else {
                        format!(
                            "Connection check: {failed} problem(s) for {id} · see its account page."
                        )
                    };
                    self.account_checks.insert(id, steps);
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => self.check_job = Some((id, receiver)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.notice = format!("Connection check for {id} stopped unexpectedly.");
                    changed = true;
                }
            }
        }
        if let Some(receiver) = self.notifications.take() {
            let mut connected = true;
            // Bound work per frame even during a notification burst.
            for _ in 0..32 {
                match receiver.try_recv() {
                    Ok(crate::discord_notifications::Event::Ready) => {
                        self.bridge_ready = true;
                        self.notice = "Discord notification capture ready. Official client must emit desktop notifications.".into();
                        changed = true;
                    }
                    Ok(crate::discord_notifications::Event::Notification(event)) => {
                        changed |= self.ingest_notification(&event)
                    }
                    Ok(crate::discord_notifications::Event::Error(error)) => {
                        self.bridge_ready = false;
                        self.notice = error;
                        changed = true;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        connected = false;
                        if self.bridge_ready {
                            self.bridge_ready = false;
                            self.notice = "Discord notification stream disconnected; reopen capture to reconnect.".into();
                            changed = true;
                        }
                        break;
                    }
                }
            }
            if connected {
                self.notifications = Some(receiver);
            }
        }
        if let Some((id, fingerprint, receiver)) = self.decision_job.take() {
            match receiver.try_recv() {
                Ok(Ok(result)) => {
                    changed |= self.apply_classification(id, result);
                }
                Ok(Err(error)) => {
                    if self.session.messages.iter().any(|m| {
                        m.id == id && policy::fingerprint(m, &self.settings) == fingerprint
                    }) {
                        self.session
                            .message_state
                            .entry(id)
                            .or_default()
                            .classification_error = Some(error);
                        self.reconcile_filters();
                        changed = true;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.decision_job = Some((id, fingerprint, receiver))
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if self.session.messages.iter().any(|m| {
                        m.id == id && policy::fingerprint(m, &self.settings) == fingerprint
                    }) {
                        self.session
                            .message_state
                            .entry(id)
                            .or_default()
                            .classification_error =
                            Some("Decision worker stopped; using local fallback.".into());
                        self.reconcile_filters();
                        changed = true;
                    }
                }
            }
        }
        if (self.live_feed || self.live_service)
            && !self.shared
            && self.service_socket.is_none()
            && self.settings.ai.enabled
            && self.decision_job.is_none()
        {
            // Newest first; messages the local rules already hide are not worth a request.
            let mut candidates: Vec<_> = self
                .session
                .messages
                .iter()
                .filter(|m| {
                    !self.dismissed(m.id)
                        && self.classification(m).is_none()
                        && self.filter_rule(m).is_none()
                })
                .collect();
            candidates.sort_by_key(|m| std::cmp::Reverse((m.timestamp, m.id)));
            let job = candidates.into_iter().find_map(|m| {
                let job = crate::decisions::DecisionJob::build(m, &self.settings).ok()?;
                (!self.decision_attempted.contains(&job.fingerprint)).then_some(job)
            });
            if let Some(job) = job {
                let (tx, rx) = std::sync::mpsc::sync_channel(1);
                self.decision_attempted.insert(job.fingerprint.clone());
                self.decision_job = Some((job.message, job.fingerprint.clone(), rx));
                std::thread::spawn(move || {
                    let _ = tx.send(
                        crate::decisions::evaluate(&job, &crate::decisions::HttpDecisions)
                            .map_err(|_| "Decision unavailable; local rules remain active.".into()),
                    );
                });
            }
        }
        changed
    }
}

/// Why a message is hidden from the feed (shown again with S / `show_spam`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filtered {
    NoReply,
    Automated,
    Terms,
    Promotion,
    NotPerson,
    NotForYou,
}

impl Filtered {
    pub fn label(self) -> &'static str {
        match self {
            Self::NoReply => "no-reply",
            Self::Automated => "automated",
            Self::Terms => "excluded term",
            Self::Promotion => "promotional",
            Self::NotPerson => "not from a person",
            Self::NotForYou => "not for you",
        }
    }
}
