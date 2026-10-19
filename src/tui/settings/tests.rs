use super::*;
use crate::{app::Session, composer::Submission};
use ratatui::{Terminal, backend::TestBackend};
use unicode_width::UnicodeWidthStr;

fn press(app: &mut App, code: KeyCode) {
    super::super::key(
        app,
        KeyEvent::new(code, KeyModifiers::NONE),
        4,
        &mut None,
        &mut None,
        None,
    );
}

fn settings_app() -> App {
    let mut app = App::new(Session::default(), Settings::default()).unwrap();
    app.editor
        .insert("Draft stays with its original recipient 東京");
    app.update_body();
    app.mode = Mode::Settings;
    app
}

fn select(app: &mut App, item: &Item) {
    app.setting_index = items(app)
        .iter()
        .position(|i| i == item)
        .unwrap_or_else(|| panic!("{item:?} missing from {:?}", app.settings_page));
}

fn edit(app: &mut App, value: &str) {
    assert_eq!(app.mode, Mode::EditSetting);
    app.command = Editor::new(value);
    press(app, KeyCode::Enter);
}

#[test]
fn overview_separates_global_sections_from_accounts_and_navigation_keeps_drafts() {
    let mut app = settings_app();
    let before = serde_json::to_vec(&app.session).unwrap();
    let overview = super::super::snapshot(&mut app, 110, 34).unwrap();
    for section in SettingsSection::ALL {
        assert!(overview.contains(section.label()));
    }
    assert!(!overview.contains("accounts."), "no raw account keys");
    select(
        &mut app,
        &Item::Open(SettingsPage::Section(SettingsSection::General)),
    );
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.settings_page,
        SettingsPage::Section(SettingsSection::General)
    );
    assert!(
        items(&app)
            .iter()
            .all(|i| matches!(i, Item::Field(name) if !name.contains('.')))
    );
    press(&mut app, KeyCode::Tab);
    assert_eq!(
        app.settings_page,
        SettingsPage::Section(SettingsSection::Feed)
    );
    press(&mut app, KeyCode::BackTab);
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.settings_page, SettingsPage::Home);
    assert_eq!(
        items(&app)[app.setting_index],
        Item::Open(SettingsPage::Section(SettingsSection::General))
    );
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode, Mode::Navigate);

    // Settings opened at a page from the drawer close from that page with one Esc.
    app.command_origin = Mode::Navigate;
    super::super::run_command(&mut app, ":accounts");
    assert_eq!(app.mode, Mode::Settings);
    select(&mut app, &Item::Action(Action::AddAccount));
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Esc);
    assert_eq!(
        app.settings_page,
        SettingsPage::Section(SettingsSection::Accounts)
    );
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode, Mode::Navigate);
    assert_eq!(serde_json::to_vec(&app.session).unwrap(), before);
}

#[test]
fn add_account_form_creates_several_accounts_of_one_provider_without_connecting() {
    let mut app = settings_app();
    open(&mut app, SettingsPage::Section(SettingsSection::Accounts));
    select(&mut app, &Item::Action(Action::AddAccount));
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.settings_page, SettingsPage::AddAccount);
    assert_eq!(app.new_account.id, "discord");
    select(&mut app, &Item::New(NewField::Name));
    press(&mut app, KeyCode::Enter);
    edit(&mut app, "  Son 👦  ");
    assert!(
        app.settings.accounts.is_empty(),
        "nothing saved before Create"
    );
    select(&mut app, &Item::Action(Action::CreateAccount));
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.settings_page, SettingsPage::Account("discord".into()));
    assert_eq!(
        items(&app)[app.setting_index],
        Item::Action(Action::SignIn("discord".into()))
    );
    let created = &app.settings.accounts[0];
    assert_eq!(
        (created.label(), created.enabled, created.allow_send),
        ("Son 👦", false, false)
    );

    // The second account of a provider gets the next free ID; typed IDs survive provider changes.
    press(&mut app, KeyCode::Esc);
    select(&mut app, &Item::Action(Action::AddAccount));
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.new_account.id, "discord-2");
    press(&mut app, KeyCode::Right);
    assert_eq!(
        (app.new_account.provider, app.new_account.id.as_str()),
        (Provider::ICloud, "icloud")
    );
    press(&mut app, KeyCode::Left);
    assert_eq!(app.new_account.id, "discord-2");
    select(&mut app, &Item::New(NewField::Id));
    press(&mut app, KeyCode::Enter);
    edit(&mut app, "discord");
    assert_eq!(app.mode, Mode::EditSetting, "duplicate IDs are rejected");
    assert!(app.notice.contains("already exists"));
    edit(&mut app, "kids");
    select(&mut app, &Item::New(NewField::Provider));
    press(&mut app, KeyCode::Right);
    press(&mut app, KeyCode::Left);
    assert_eq!(app.new_account.id, "kids");
    select(&mut app, &Item::Action(Action::CreateAccount));
    press(&mut app, KeyCode::Enter);
    let ids: Vec<_> = app
        .settings
        .accounts
        .iter()
        .map(|a| a.id.as_str())
        .collect();
    assert_eq!(ids, ["discord", "kids"]);
    assert!(
        app.settings
            .accounts
            .iter()
            .all(|a| a.provider == Provider::Discord && !a.enabled)
    );

    // Display names distinguish accounts of one provider in the feed.
    for message in &mut app.session.messages {
        message.provider = Some(Provider::Discord);
        message.account = Some("discord".into());
    }
    app.mode = Mode::Navigate;
    let feed = super::super::snapshot(&mut app, 100, 40).unwrap();
    assert!(feed.contains("Discord · Son 👦"), "{feed}");
}

#[test]
fn renaming_keeps_connection_identity_scope_and_ai_decisions() {
    let mut settings = Settings::default();
    settings.add_account("home", Provider::ICloud).unwrap();
    settings.accounts[0].username = "fictional@example.invalid".into();
    let before = settings.clone();
    settings.set("accounts.home.name", "Home mail").unwrap();
    let (old, new) = (&before.accounts[0], &settings.accounts[0]);
    assert!(old.same_connection(new) && old != new);
    assert_eq!(crate::providers::scope(old), crate::providers::scope(new));
    let mut message = crate::fixtures::messages().remove(0);
    message.account = Some("home".into());
    message.provider = Some(Provider::ICloud);
    assert_eq!(
        crate::policy::fingerprint(&message, &before),
        crate::policy::fingerprint(&message, &settings)
    );
    for invalid in ["two\nlines", &"x".repeat(81)] {
        assert!(settings.set("accounts.home.name", invalid).is_err());
    }
    settings.set("accounts.home.name", "").unwrap();
    assert_eq!(settings.accounts[0].label(), "home");
    let stored = serde_json::to_value(&settings.accounts[0]).unwrap();
    assert!(
        stored.get("name").is_none(),
        "empty names stay out of configs read by older releases"
    );
}

#[test]
fn account_actions_connect_disconnect_and_remove_only_after_confirmation() {
    let mut app = settings_app();
    let draft = app.session.composer.draft.clone();
    app.settings
        .add_account("personal", Provider::Discord)
        .unwrap();
    let page = SettingsPage::Account("personal".into());
    open(&mut app, page.clone());
    // Arrows never run account actions: Left on an action goes back instead.
    select(&mut app, &Item::Action(Action::Connect("personal".into())));
    press(&mut app, KeyCode::Left);
    assert!(!app.settings.accounts[0].enabled);
    assert_eq!(
        app.settings_page,
        SettingsPage::Section(SettingsSection::Accounts)
    );
    press(&mut app, KeyCode::Enter);
    select(&mut app, &Item::Action(Action::Connect("personal".into())));
    press(&mut app, KeyCode::Enter);
    assert!(app.settings.accounts[0].enabled);
    assert!(app.notice.contains("sign in"), "{}", app.notice);
    assert_eq!(
        items(&app)[app.setting_index],
        Item::Action(Action::Disconnect("personal".into()))
    );
    press(&mut app, KeyCode::Enter);
    assert!(!app.settings.accounts[0].enabled);
    select(
        &mut app,
        &Item::Field("accounts.personal.allow_send".into()),
    );
    press(&mut app, KeyCode::Right);
    assert!(app.settings.accounts[0].allow_send);
    press(&mut app, KeyCode::Left);
    assert!(!app.settings.accounts[0].allow_send);

    select(
        &mut app,
        &Item::Action(Action::AskRemove("personal".into())),
    );
    press(&mut app, KeyCode::Enter);
    assert_eq!(
        app.settings_page,
        SettingsPage::RemoveAccount("personal".into())
    );
    assert!(
        super::super::snapshot(&mut app, 60, 30)
            .unwrap()
            .contains("Remove personal?")
    );
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.settings_page, page);
    press(&mut app, KeyCode::Enter);
    select(&mut app, &Item::Action(Action::Back));
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.settings.accounts.len(), 1);

    // A pending or uncertain send from the account blocks removal.
    let mut busy = draft.clone().expect("draft");
    busy.target.account = "personal".into();
    app.session.composer.send = SendState::Uncertain(Submission {
        operation: 7,
        draft: busy,
    });
    open(&mut app, SettingsPage::RemoveAccount("personal".into()));
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.settings.accounts.len(), 1);
    assert!(app.notice.contains("uncertain"));
    app.session.composer.send = SendState::Ready;
    press(&mut app, KeyCode::Enter);
    assert!(app.settings.accounts.is_empty());
    assert_eq!(
        app.settings_page,
        SettingsPage::Section(SettingsSection::Accounts)
    );
    assert!(app.notice.starts_with("Removed personal"));
    assert_eq!(app.session.composer.draft, draft);
}

#[test]
fn pages_fill_tall_windows_and_keep_the_selected_row_visible_when_resized() {
    let mut app = settings_app();
    app.settings
        .add_account("work-chat", Provider::Teams)
        .unwrap();
    let before = serde_json::to_vec(&app.session).unwrap();
    for page in [
        SettingsPage::Home,
        SettingsPage::Section(SettingsSection::Feed),
        SettingsPage::Account("work-chat".into()),
        SettingsPage::AddAccount,
    ] {
        open(&mut app, page);
        for index in 0..items(&app).len() {
            app.setting_index = index;
            for (width, height) in [(28, 20), (40, 72), (120, 45), (28, 16)] {
                let output = super::super::snapshot(&mut app, width, height).unwrap();
                assert!(output.contains('›'), "selection hidden at {width}x{height}");
                assert_eq!(output.lines().count(), height as usize);
            }
        }
    }
    open(&mut app, SettingsPage::Account("work-chat".into()));
    let tall = super::super::snapshot(&mut app, 40, 72).unwrap();
    assert!(
        tall.contains("Remove account"),
        "tall windows show the whole page"
    );
    assert!(!tall.contains("Discord mode"), "only Teams fields appear");
    assert!(app.settings_page_size > 6);
    assert_eq!(serde_json::to_vec(&app.session).unwrap(), before);
}

#[test]
fn wide_navigation_is_clickable_and_second_click_follows_links() {
    let mut app = settings_app();
    let mut terminal = Terminal::new(TestBackend::new(110, 34)).unwrap();
    let mut hits = HitAreas::default();
    terminal
        .draw(|frame| hits = super::super::draw(frame, &mut app, Duration::ZERO))
        .unwrap();
    let (_, page) = hits
        .settings_nav
        .iter()
        .find(|(_, page)| *page == SettingsPage::Section(SettingsSection::Spam))
        .expect("spam navigation entry");
    open(&mut app, page.clone());
    assert_eq!(
        app.settings_page,
        SettingsPage::Section(SettingsSection::Spam)
    );
    terminal
        .draw(|frame| hits = super::super::draw(frame, &mut app, Duration::ZERO))
        .unwrap();
    assert!(!hits.settings_nav.is_empty());
    let mut narrow = Terminal::new(TestBackend::new(40, 34)).unwrap();
    narrow
        .draw(|frame| hits = super::super::draw(frame, &mut app, Duration::ZERO))
        .unwrap();
    assert!(hits.settings_nav.is_empty());
    open(&mut app, SettingsPage::Home);
    click(&mut app, 2);
    assert_eq!(app.settings_page, SettingsPage::Home);
    click(&mut app, 2);
    assert_eq!(
        app.settings_page,
        SettingsPage::Section(SettingsSection::Feed)
    );
}

#[test]
fn titles_labels_values_and_notes_have_distinct_styles_in_both_themes() {
    for theme in [Theme::Midnight, Theme::Dusk] {
        let mut app = App::new(
            Session::default(),
            Settings {
                theme,
                reduced_motion: true,
                ..Settings::default()
            },
        )
        .unwrap();
        app.mode = Mode::Settings;
        open(&mut app, SettingsPage::Section(SettingsSection::General));
        let p = palette(theme, Duration::ZERO, true);
        let mut terminal = Terminal::new(TestBackend::new(110, 42)).unwrap();
        terminal
            .draw(|frame| {
                draw(frame, frame.area(), p, &mut app);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        // Columns count cells, so multi-byte symbols such as › do not shift positions.
        let find = |needle: &str| {
            for y in 0..42 {
                let cells: Vec<&str> = (0..110).map(|x| buffer[(x, y)].symbol()).collect();
                for x in 0..cells.len() {
                    if cells[x..].concat().starts_with(needle) {
                        return (x as u16, y);
                    }
                }
            }
            panic!("missing {needle}");
        };
        let colors = ["› General", "Automatic updates", "true", "Install stable"].map(|needle| {
            let (x, y) = find(needle);
            let x = if needle.starts_with('›') { x + 2 } else { x };
            buffer[(x, y)].fg
        });
        for i in 0..colors.len() {
            for j in i + 1..colors.len() {
                assert_ne!(colors[i], colors[j]);
            }
        }
        let (_, row) = find("Automatic updates");
        assert_eq!(buffer[(108, row)].bg, p.selected);
    }
}

#[test]
fn wrapping_preserves_unicode_clusters_and_limits_long_value_previews() {
    let value = "東京 👩🏽‍💻 e\u{301} a_very_long_unbroken_setting_name";
    for width in [4, 9, 24, 80] {
        let lines = wrapped(value, width);
        assert!(lines.iter().all(|line| line.width() <= width));
        assert_eq!(
            lines.join(" ").split_whitespace().collect::<String>(),
            value.split_whitespace().collect::<String>()
        );
    }
    let summary = preview(&"東京👩🏽‍💻".repeat(1000), 24);
    assert!(summary.width() <= 24 && summary.ends_with('…'));
    assert_eq!(preview("", 24), "Not set");
    assert_eq!(preview("hello\nworld", 24), "hello ↵");
}

#[test]
fn connection_checks_are_offered_for_mail_and_teams_and_results_show_on_the_page() {
    use crate::providers::check::Step;
    let mut app = settings_app();
    app.settings
        .add_account("work-chat", Provider::Teams)
        .unwrap();
    app.settings
        .add_account("discord", Provider::Discord)
        .unwrap();
    open(&mut app, SettingsPage::Account("discord".into()));
    assert!(
        !items(&app)
            .iter()
            .any(|i| matches!(i, Item::Action(Action::Test(_))))
    );
    open(&mut app, SettingsPage::Account("work-chat".into()));
    let test = Item::Action(Action::Test("work-chat".into()));
    select(&mut app, &test);
    // The offline demo never connects, even on request.
    press(&mut app, KeyCode::Enter);
    assert!(app.check_job.is_none());
    assert!(app.notice.contains("offline demo"));

    app.account_checks.insert(
        "work-chat".into(),
        vec![
            Step::ok("Sign in", "Signed in as owner@example.invalid"),
            Step::ok("Chats", "2 recent chats")
                .with_items(vec!["Jules · oneOnOne · 19:abc@unq.gbl.spaces".into()]),
            Step::failed(
                "Synced chats",
                "None selected; add chat IDs from the list to resources",
            ),
        ],
    );
    let frame = super::super::snapshot(&mut app, 110, 40).unwrap();
    assert!(
        frame.contains("✓ Sign in · Signed in as owner@example.invalid"),
        "{frame}"
    );
    assert!(frame.contains("19:abc@unq.gbl.spaces"));
    assert!(frame.contains("✗ Synced chats"));
    assert!(frame.contains("Test connection"));
}

#[test]
fn values_are_edited_in_their_row_without_leaving_the_page() {
    let mut app = settings_app();
    let draft = app.session.composer.draft.clone();
    open(&mut app, SettingsPage::Section(SettingsSection::Ai));
    select(&mut app, &Item::Field("ai.model".into()));
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.mode, Mode::EditSetting);
    press(&mut app, KeyCode::Char('2'));
    let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
    terminal
        .draw(|frame| {
            super::super::draw(frame, &mut app, Duration::ZERO);
        })
        .unwrap();
    let screen: Vec<String> = (0..30)
        .map(|y| {
            (0..110)
                .map(|x| terminal.backend().buffer()[(x, y)].symbol())
                .collect()
        })
        .collect();
    let row = screen
        .iter()
        .position(|line| line.contains("› Model"))
        .expect("the page stays visible while editing");
    assert!(screen[row].contains("jev-latest2"), "{}", screen[row]);
    assert!(screen[row + 1].contains("Enter saves · Esc cancels"));
    assert!(
        screen
            .iter()
            .any(|line| line.contains("Settings › AI decisions"))
    );
    assert!(!screen.iter().any(|line| line.contains("Draft stays")));
    let cursor = terminal.get_cursor_position().unwrap();
    assert_eq!(cursor.y as usize, row, "cursor sits in the row");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.mode, Mode::Settings);
    assert_eq!(app.settings.ai.model, "jev-latest2");

    // Esc abandons the typed value; invalid values stay in the row with the reason.
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Char('x'));
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.mode, Mode::Settings);
    assert_eq!(app.settings.ai.model, "jev-latest2");
    select(&mut app, &Item::Field("ai.timeout_ms".into()));
    press(&mut app, KeyCode::Enter);
    edit(&mut app, "5");
    assert_eq!(app.mode, Mode::EditSetting, "rejected values keep editing");
    assert!(app.notice.contains("100"), "{}", app.notice);
    assert_eq!(app.session.composer.draft, draft);
}

#[test]
fn prompts_wrap_in_place_and_commands_open_their_row() {
    let mut app = settings_app();
    app.mode = Mode::Navigate;
    super::super::run_command(&mut app, ":spam prompt");
    assert_eq!(app.mode, Mode::EditSetting);
    assert_eq!(
        app.settings_page,
        SettingsPage::Section(SettingsSection::Spam)
    );
    assert_eq!(
        items(&app)[app.setting_index],
        Item::Field("spam.prompt".into())
    );
    let screen = super::super::snapshot(&mut app, 110, 34).unwrap();
    assert!(screen.contains("Settings › Filtering"));
    assert!(screen.contains("Ctrl+S saves · Enter adds a line · Esc cancels"));
    // The default prompt wraps at words across rows instead of scrolling sideways.
    let prompt_rows = screen
        .lines()
        .filter(|line| line.contains("Is this unsolicited") || line.contains("contain a link."))
        .count();
    assert_eq!(prompt_rows, 2, "{screen}");
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.mode, Mode::EditSetting, "Enter adds a line");
    press(&mut app, KeyCode::Char('K'));
    super::super::key(
        &mut app,
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        4,
        &mut None,
        &mut None,
        None,
    );
    assert_eq!(app.mode, Mode::Settings);
    assert!(app.settings.spam.prompt.ends_with("link.\nK"));

    // An unknown profile refuses rather than opening an editor for nothing.
    super::super::run_command(&mut app, ":profile prompt nobody");
    assert_ne!(app.mode, Mode::EditSetting);
    assert!(app.notice.contains("nobody"), "{}", app.notice);
}

#[test]
fn about_me_and_the_for_you_questions_open_in_place_from_commands() {
    let mut app = settings_app();
    for (command, page, field) in [
        (":about", SettingsSection::Ai, "ai.about"),
        (":spam person", SettingsSection::Spam, "spam.person_prompt"),
        (
            ":spam for-you",
            SettingsSection::Spam,
            "spam.for_you_prompt",
        ),
    ] {
        app.mode = Mode::Navigate;
        super::super::run_command(&mut app, command);
        assert_eq!(app.mode, Mode::EditSetting, "{command}");
        assert_eq!(app.settings_page, SettingsPage::Section(page));
        assert_eq!(items(&app)[app.setting_index], Item::Field(field.into()));
        let screen = super::super::snapshot(&mut app, 110, 40).unwrap();
        assert!(
            screen.contains("Ctrl+S saves · Enter adds a line"),
            "{screen}"
        );
        press(&mut app, KeyCode::Esc);
    }
    // About me saves through the shared schema like any other field.
    app.mode = Mode::Navigate;
    super::super::run_command(&mut app, ":about");
    for c in "Toby".chars() {
        press(&mut app, KeyCode::Char(c));
    }
    super::super::key(
        &mut app,
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        4,
        &mut None,
        &mut None,
        None,
    );
    assert_eq!(app.settings.ai.about, "Toby");
    let screen = super::super::snapshot(&mut app, 110, 40).unwrap();
    assert!(screen.contains("About me"), "{screen}");
}

#[test]
fn credentials_typed_from_settings_are_masked_in_their_row() {
    let mut app = settings_app();
    app.settings
        .add_account("home-mail", Provider::ICloud)
        .unwrap();
    open(&mut app, SettingsPage::Account("home-mail".into()));
    select(&mut app, &Item::Action(Action::SignIn("home-mail".into())));
    app.mode = Mode::Secret;
    app.command_origin = Mode::Settings;
    app.command = Editor::new("fictional-secret-marker");
    let screen = super::super::snapshot(&mut app, 110, 34).unwrap();
    assert!(!screen.contains("fictional-secret-marker"));
    let row = screen.lines().find(|l| l.contains("› Sign in")).unwrap();
    assert!(row.contains(&"•".repeat(23)), "{row}");
    assert!(screen.contains("Enter stores it in the keyring"));
    let narrow = super::super::snapshot(&mut app, 40, 34).unwrap();
    assert!(!narrow.contains("fictional-secret-marker"));
}

#[test]
fn soft_rows_break_at_spaces_and_split_long_words() {
    let text = "aaa bbb ccc\nabcdefghij";
    let rows: Vec<&str> = render::soft_rows(text, 7)
        .into_iter()
        .map(|(s, e)| &text[s..e])
        .collect();
    assert_eq!(rows, ["aaa ", "bbb ccc", "abcdefg", "hij"]);
    assert_eq!(render::soft_rows("", 5), [(0, 0)]);
}

#[test]
fn mail_accounts_choose_their_folders_on_the_account_page() {
    let mut app = settings_app();
    app.settings
        .add_account("home-mail", Provider::ICloud)
        .unwrap();
    app.settings
        .add_account("work-mail", Provider::Office365)
        .unwrap();
    open(&mut app, SettingsPage::Account("home-mail".into()));
    let folders = Item::Field("accounts.home-mail.resources".into());
    select(&mut app, &folders);
    let screen = super::super::snapshot(&mut app, 110, 34).unwrap();
    let row = screen.lines().find(|l| l.contains("› Folders")).unwrap();
    assert!(row.contains("INBOX"), "{row}");
    assert!(screen.contains("empty syncs INBOX"));
    assert!(screen.contains("lists your folders"));
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.mode, Mode::EditSetting);
    app.command = Editor::new("INBOX\nArchive");
    super::super::key(
        &mut app,
        KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        4,
        &mut None,
        &mut None,
        None,
    );
    assert_eq!(app.settings.accounts[0].resources, ["INBOX", "Archive"]);
    open(&mut app, SettingsPage::Account("work-mail".into()));
    select(
        &mut app,
        &Item::Field("accounts.work-mail.resources".into()),
    );
    let screen = super::super::snapshot(&mut app, 110, 34).unwrap();
    let row = screen.lines().find(|l| l.contains("› Folders")).unwrap();
    assert!(row.contains("inbox"), "{row}");
}
