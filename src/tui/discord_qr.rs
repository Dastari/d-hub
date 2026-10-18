//! Personal Discord QR sign-in modal, shared by the drawer and standalone CLI.
use super::*;

pub(super) fn new_discord_setup(
    account: &crate::policy::AccountConfig,
) -> crate::discord_auth::Setup {
    let mut setup = crate::discord_auth::Setup::new(&account.id);
    match crate::discord_auth::start(account.clone()) {
        Ok(job) => setup.job = Some(job),
        Err(e) => {
            setup.apply(crate::discord_auth::Event::Failed(e.to_string()));
        }
    }
    setup
}
pub(super) fn poll_setup(
    setup: &mut crate::discord_auth::Setup,
) -> (bool, Option<crate::discord_auth::protocol::Identity>) {
    use crate::discord_auth::{Event, Phase};
    let mut changed = false;
    let mut saved = None;
    let mut finished = false;
    if let Some(job) = &setup.job {
        let mut events = Vec::new();
        loop {
            match job.events.try_recv() {
                Ok(event) => events.push(event),
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }
        for event in events {
            saved = setup.apply(event).or(saved);
            changed = true;
        }
    }
    if finished {
        setup.job = None;
        if !matches!(setup.phase, Phase::Saved | Phase::Failed | Phase::Cancelled) {
            setup.apply(Event::Failed(
                "Discord sign-in worker stopped; press r to retry".into(),
            ));
            changed = true;
        }
    }
    if setup.expires.is_some_and(|t| Instant::now() >= t)
        && matches!(setup.phase, Phase::Scan | Phase::Approve | Phase::Review)
    {
        if let Some(job) = setup.job.take() {
            job.cancel();
        }
        setup.apply(Event::Failed(
            "Discord QR/confirmation expired; press r for a new code".into(),
        ));
        changed = true;
    }
    let seconds = setup
        .expires
        .map(|t| t.saturating_duration_since(Instant::now()).as_secs());
    if setup.seconds != seconds {
        setup.seconds = seconds;
        changed = true;
    }
    (changed, saved)
}
pub(super) fn poll_discord_setup(app: &mut App) -> bool {
    if let Some(setup) = &app.discord_setup
        && setup.phase == crate::discord_auth::Phase::RetryRequested
    {
        if let Some(account) = app
            .settings
            .accounts
            .iter()
            .find(|a| a.id == setup.account)
            .cloned()
        {
            app.discord_setup = Some(new_discord_setup(&account));
        } else if let Some(setup) = &mut app.discord_setup {
            setup.apply(crate::discord_auth::Event::Failed(
                "Account was removed; close setup and add an account".into(),
            ));
        }
        return true;
    }
    let Some(setup) = &mut app.discord_setup else {
        return false;
    };
    if let Some(job) = &setup.job {
        let original = &job.account;
        let same = app.settings.accounts.iter().any(|a| {
            a.id == original.id
                && a.provider == original.provider
                && a.discord_mode == original.discord_mode
                && a.discord_user_id == original.discord_user_id
        });
        if !same && job.cancel() {
            setup.job = None;
            setup.apply(crate::discord_auth::Event::Failed(
                "Account settings changed during sign-in; reopen setup".into(),
            ));
            return true;
        }
    }
    let (changed, saved) = poll_setup(setup);
    if let Some(user) = saved
        && let Some(account) = app.settings.accounts.iter_mut().find(|a| {
            a.id == setup.account
                && a.provider == Provider::Discord
                && a.discord_mode == "personal"
                && (a.discord_user_id.is_empty() || a.discord_user_id == user.id)
        })
    {
        account.discord_user_id = user.id;
        account.username = user.username;
        app.notice = format!(
            "Discord signed in · :account enable {} starts DM sync; allow_send controls replies.",
            account.id
        );
    }
    changed
}
pub(super) fn discord_setup_key(setup: &mut crate::discord_auth::Setup, key: KeyEvent) -> bool {
    use crate::discord_auth::Phase;
    let close = matches!(key.code, KeyCode::Esc | KeyCode::Char('q'))
        || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('q'));
    if close {
        if setup.phase == Phase::Saving {
            setup.status = "Keyring save already started; waiting for its result…".into();
            return false;
        }
        if setup.job.as_ref().is_some_and(|job| !job.cancel()) {
            setup.status = "Keyring save already started; waiting for its result…".into();
            return false;
        }
        return true;
    }
    if key.code == KeyCode::Enter {
        if setup.phase == Phase::Review {
            if let Some(job) = &setup.job
                && job.save()
            {
                setup.phase = Phase::Saving;
                setup.matrix = None;
                setup.expires = None;
                setup.status = "Saving session in desktop keyring…".into();
            }
        } else if setup.phase == Phase::Saved {
            return true;
        }
    }
    if key.code == KeyCode::Char('r') && matches!(setup.phase, Phase::Failed | Phase::Cancelled) {
        // The caller starts retries against current validated account settings.
        setup.phase = Phase::RetryRequested;
    }
    false
}
pub(super) fn draw_discord_qr(
    frame: &mut Frame,
    area: Rect,
    p: Palette,
    setup: &crate::discord_auth::Setup,
) {
    use crate::discord_auth::Phase;
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(p.bg).fg(p.fg)),
        area,
    );
    let inner = area.inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 0,
    });
    if inner.height == 0 {
        return;
    }
    frame.render_widget(
        text(
            format!("Discord · personal account · {}", safe_text(&setup.account)),
            p.accent,
        ),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    frame.render_widget(
        text(
            "Scan using your Discord mobile app, then approve sign-in.",
            p.muted,
        ),
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );
    let mut y = inner.y + 3;
    if let Some(matrix) = &setup.matrix {
        let width = matrix.width as u16;
        let height = matrix.width.div_ceil(2) as u16;
        if inner.width < width || inner.height < height + 9 {
            frame.render_widget(text(format!("Resize terminal to at least {} columns × {} rows to scan. QR is never cropped.",width+2,height+9),p.fg).wrap(Wrap{trim:false}),Rect::new(inner.x,y,inner.width,inner.height.saturating_sub(6)));
            y = inner.bottom().saturating_sub(5);
        } else {
            let x = inner.x + (inner.width - width) / 2;
            let mut lines = Vec::new();
            for row in (0..matrix.width).step_by(2) {
                let symbols: String = (0..matrix.width)
                    .map(
                        |col| match (matrix.dark(col, row), matrix.dark(col, row + 1)) {
                            (true, true) => '█',
                            (true, false) => '▀',
                            (false, true) => '▄',
                            _ => ' ',
                        },
                    )
                    .collect();
                lines.push(Line::styled(
                    symbols,
                    Style::default().fg(Color::Black).bg(Color::White),
                ));
            }
            frame.render_widget(
                Paragraph::new(lines).style(Style::default().bg(Color::White)),
                Rect::new(x, y, width, height),
            );
            y += height + 1;
        }
    }
    if let Some(user) = &setup.user {
        frame.render_widget(
            text(
                format!("@{} · user {}", safe_text(&user.username), user.id),
                p.accent,
            ),
            Rect::new(
                inner.x,
                y.min(inner.bottom().saturating_sub(4)),
                inner.width,
                1,
            ),
        );
        y += 1;
    }
    let remaining = setup
        .expires
        .map(|t| {
            format!(
                " · {}s",
                t.saturating_duration_since(Instant::now()).as_secs()
            )
        })
        .unwrap_or_default();
    let status = format!("{}{}", safe_text(&setup.status), remaining);
    let status_y = y.min(inner.bottom().saturating_sub(4));
    frame.render_widget(
        text(status, p.fg).wrap(Wrap { trim: false }),
        Rect::new(
            inner.x,
            status_y,
            inner.width,
            inner.bottom().saturating_sub(status_y + 2),
        ),
    );
    let footer = match setup.phase {
        Phase::Review => "Enter save this identity · Esc cancel",
        Phase::Saving => "Saving to keyring · no plaintext fallback",
        Phase::Saved => "Enter / Esc close · enable account to sync DMs",
        Phase::Failed | Phase::Cancelled => "r retry · Esc close",
        _ => "Esc cancel · only scan codes you requested here",
    };
    frame.render_widget(
        text(footer, p.muted),
        Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
    );
}
/// Standalone CLI uses the same QR modal and requires an interactive terminal.
pub fn discord_setup(
    account: crate::policy::AccountConfig,
) -> Result<Option<crate::discord_auth::protocol::Identity>> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("Discord QR setup requires an interactive terminal; use d-hub settings");
    }
    let guard = TerminalGuard::enter()?;
    let mut terminal = DefaultTerminal::new(ratatui::backend::CrosstermBackend::new(io::stdout()))?;
    let mut setup = new_discord_setup(&account);
    loop {
        poll_setup(&mut setup);
        if setup.phase == crate::discord_auth::Phase::RetryRequested {
            setup = new_discord_setup(&account);
        }
        terminal.draw(|frame| {
            draw_discord_qr(
                frame,
                frame.area(),
                palette(crate::config::Theme::Midnight, Duration::ZERO, true),
                &setup,
            )
        })?;
        if event::poll(Duration::from_millis(80))?
            && let Event::Key(key) = event::read()?
            && key.kind != KeyEventKind::Release
            && discord_setup_key(&mut setup, key)
        {
            break;
        }
    }
    let user = if setup.phase == crate::discord_auth::Phase::Saved {
        setup.user.clone()
    } else {
        None
    };
    drop(setup);
    drop(terminal);
    drop(guard);
    Ok(user)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Session;

    #[test]
    fn qr_setup_render_decodes_and_keeps_a_four_module_quiet_zone() {
        let url = "https://discord.com/ra/fictional-fingerprint";
        let matrix = crate::discord_auth::Matrix::encode(url).unwrap();
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        let mut setup = crate::discord_auth::Setup::new("personal");
        setup.apply(crate::discord_auth::Event::Code {
            matrix: matrix.clone(),
            expires: Instant::now() + Duration::from_secs(60),
        });
        app.discord_setup = Some(setup);
        app.mode = Mode::DiscordQr;
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 40)).unwrap();
        terminal
            .draw(|frame| render(frame, &mut app, Duration::ZERO))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let width = matrix.width;
        let x = 1 + (78 - width as u16) / 2;
        let y = 3;
        let scale = 5;
        let side = width * scale;
        let mut image = vec![255; side * side];
        for row in 0..width {
            for col in 0..width {
                let cell = &buffer[(x + col as u16, y + (row / 2) as u16)];
                assert_eq!(cell.fg, Color::Black);
                assert_eq!(cell.bg, Color::White);
                let dark = matches!((cell.symbol(), row % 2), ("█", _) | ("▀", 0) | ("▄", 1));
                if row < 4 || col < 4 || row >= width - 4 || col >= width - 4 {
                    assert!(!dark, "QR quiet zone must be white");
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        image[(row * scale + dy) * side + col * scale + dx] =
                            if dark { 0 } else { 255 };
                    }
                }
            }
        }
        let mut decoder = quircs::Quirc::default();
        let decoded = decoder
            .identify(side, side, &image)
            .next()
            .unwrap()
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!(String::from_utf8(decoded.payload).unwrap(), url);
        let frame = snapshot(&mut app, 32, 18).unwrap();
        assert!(frame.contains("Resize terminal"));
        assert!(!frame.contains('█'));
    }
    #[test]
    fn qr_expiry_cancel_and_retry_preserve_the_composer_and_keep_session_data_private() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        app.editor.insert("Retain my draft 東京");
        app.update_body();
        let before = serde_json::to_string(&app.session).unwrap();
        let mut setup = crate::discord_auth::Setup::new("personal");
        setup.apply(crate::discord_auth::Event::Code {
            matrix: crate::discord_auth::Matrix::encode("fictional-private-session-url").unwrap(),
            expires: Instant::now() - Duration::from_secs(1),
        });
        app.discord_setup = Some(setup);
        app.mode = Mode::DiscordQr;
        assert!(poll_discord_setup(&mut app));
        assert!(app.discord_setup.as_ref().unwrap().matrix.is_none());
        assert_eq!(
            app.discord_setup.as_ref().unwrap().phase,
            crate::discord_auth::Phase::Failed
        );
        let frame = snapshot(&mut app, 80, 40).unwrap();
        assert!(!frame.contains("fictional-private-session-url"));
        assert!(frame.contains("expired"));
        key(
            &mut app,
            KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
            4,
            &mut None,
            &mut None,
            None,
        );
        assert!(app.discord_setup.is_none());
        assert_eq!(before, serde_json::to_string(&app.session).unwrap());
        let mut setup = crate::discord_auth::Setup::new("personal");
        setup.apply(crate::discord_auth::Event::Failed("Failure".into()));
        assert!(!discord_setup_key(
            &mut setup,
            KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)
        ));
        assert_eq!(setup.phase, crate::discord_auth::Phase::RetryRequested);
    }
}
