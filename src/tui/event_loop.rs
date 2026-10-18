//! Terminal lifecycle and the event loop; restores terminal modes on every exit path.
use super::*;

pub(super) struct TerminalGuard;

impl TerminalGuard {
    pub(super) fn enter() -> Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            EnableFocusChange,
            EnableMouseCapture
        )?;
        // Unsupported terminals ignore this protocol; F5 remains an unambiguous send key.
        execute!(
            io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            io::stdout(),
            PopKeyboardEnhancementFlags,
            DisableMouseCapture,
            DisableFocusChange,
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

pub fn run(
    app: App,
    store: Option<&DemoStore>,
    config_path: Option<&Path>,
    reader_only: bool,
) -> Result<()> {
    run_inner(app, store, config_path, reader_only, None)
}
pub fn run_connected(app: App, link: crate::ipc::Link, reader_only: bool) -> Result<()> {
    run_inner(app, None, None, reader_only, Some(link))
}
pub(super) fn run_inner(
    mut app: App,
    store: Option<&DemoStore>,
    config_path: Option<&Path>,
    reader_only: bool,
    mut link: Option<crate::ipc::Link>,
) -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("interactive terminal required; use demo --snapshot for a text preview");
    }
    let _guard = TerminalGuard::enter()?;
    let mut terminal = DefaultTerminal::new(ratatui::backend::CrosstermBackend::new(io::stdout()))?;
    // Restore terminal modes before the panic report as well as on ordinary errors.
    let old_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(
            io::stdout(),
            PopKeyboardEnhancementFlags,
            DisableMouseCapture,
            DisableFocusChange,
            DisableBracketedPaste,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
        old_hook(info);
    }));
    let started = Instant::now();
    let mut pending = None;
    let mut g_prefix: Option<Instant> = None;
    let mut dirty = true;
    let mut exit_notice = None;
    let mut hits = HitAreas::default();
    let mut previous_session = serde_json::to_vec(&app.session)?;
    let mut previous_settings = app.settings.clone();
    if let Some(store) = store {
        store.save(&app.session)?;
    }
    loop {
        if let Some(link) = &mut link {
            dirty |= link.sync(&mut app)?;
            if link.restart_frontend() {
                break;
            }
        }
        dirty |= poll_discord_setup(&mut app);
        if link.is_none() || app.auth_job.is_some() {
            dirty |= app.poll_background();
        }
        if let Some((deadline, operation, outcome)) = pending
            && Instant::now() >= deadline
        {
            pending = None;
            finish_send(&mut app, operation, outcome);
            dirty = true;
        }
        if g_prefix.is_some_and(|time| time.elapsed() >= Duration::from_millis(500)) {
            g_prefix = None;
            app.notice = "Prefix expired · ? for controls".into();
            dirty = true;
        }
        if dirty || !app.settings.reduced_motion {
            terminal.draw(|frame| {
                hits = draw(frame, &mut app, started.elapsed());
            })?;
            dirty = false;
        }
        let mut quit = false;
        if event::poll(Duration::from_millis(80))? {
            dirty = true;
            let input = event::read()?;
            if link.as_ref().is_some_and(|l| l.frozen) {
                app.notice = "Updating d-hub · drafts saved · waiting for service handover…".into();
                continue;
            }
            match input {
                Event::Key(event) => {
                    let mode = app.mode;
                    if link.as_ref().is_some_and(|l| !l.connected) && !reader_only {
                        quit = (event.modifiers.contains(KeyModifiers::CONTROL)
                            && event.code == KeyCode::Char('q'))
                            || event.code == KeyCode::Char('q');
                    } else {
                        quit = key(
                            &mut app,
                            event,
                            hits.feed_page.max(1),
                            &mut pending,
                            &mut g_prefix,
                            config_path,
                        );
                    }
                    if reader_only && mode == Mode::Reader && app.mode == Mode::Navigate {
                        quit = true;
                    }
                }
                Event::Paste(value) => {
                    if link.as_ref().is_none_or(|l| l.connected) {
                        commands::paste(&mut app, &value);
                    }
                }
                Event::FocusGained if app.mode != Mode::DiscordQr => app.focus(true),
                Event::FocusLost if app.mode != Mode::DiscordQr => app.focus(false),
                Event::FocusGained | Event::FocusLost => {}
                Event::Resize(_, _) => {}
                Event::Mouse(event) => mouse(&mut app, event, &hits),
            }
        }
        if let Some(store) = store {
            let bytes = serde_json::to_vec(&app.session)?;
            if bytes != previous_session {
                store.save(&app.session).context(
                    "could not persist demo; exiting with previous on-disk draft intact",
                )?;
                previous_session = bytes;
            }
        }
        if app.settings != previous_settings {
            if let Some(path) = config_path {
                app.settings.save(path)?;
            }
            previous_settings = app.settings.clone();
        }
        if quit {
            if let Some(link) = &mut link {
                link.flush(&mut app)?;
                if !link.connected {
                    exit_notice = Some(safe_text(&app.notice));
                }
            }
            break;
        }
    }
    drop(terminal);
    drop(_guard);
    if let Some(notice) = exit_notice {
        eprintln!("{notice}");
    }
    if link.as_ref().is_some_and(|l| l.restart_frontend()) {
        let socket = link.as_ref().map(|l| l.socket.clone());
        drop(link);
        // The server releases the old editor asynchronously after socket close.
        if !reader_only && let Some(socket) = socket {
            let until = Instant::now() + Duration::from_secs(3);
            while Instant::now() < until {
                if crate::ipc::Client::build_info(&socket).is_ok_and(|reply| {
                    matches!(
                        reply,
                        crate::ipc::Reply::BuildInfo {
                            editor_connected: false,
                            ..
                        }
                    )
                }) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
        }
        crate::update::reexec(&crate::update::executable()?)?;
    }
    Ok(())
}

pub fn snapshot(app: &mut App, width: u16, height: u16) -> Result<String> {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height))?;
    terminal.draw(|frame| render(frame, app, Duration::ZERO))?;
    let buffer = terminal.backend().buffer();
    let mut output = String::new();
    for row in 0..height {
        let mut line = String::new();
        for col in 0..width {
            let cell = &buffer[(col, row)];
            if col > 0 && unicode_width::UnicodeWidthStr::width(buffer[(col - 1, row)].symbol()) > 1
            {
                continue;
            }
            line.push_str(cell.symbol());
        }
        output.push_str(line.trim_end());
        output.push('\n');
    }
    Ok(output)
}

pub fn reader(id: crate::domain::MessageId, settings: Settings) -> Result<()> {
    let message = crate::fixtures::find(id).context("unknown demo message ID")?;
    let mut app = App::new(crate::app::Session::default(), settings)?;
    app.reader = Some(message);
    app.mode = Mode::Reader;
    run(app, None, None, true)
}
