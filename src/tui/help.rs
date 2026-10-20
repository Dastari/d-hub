//! Discoverable controls shown by ? and :help. Rows lay out as two columns when there is room
//! and as key-then-description in a narrow drawer; demo-only controls are left out live.
use super::*;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, PartialEq)]
enum Show {
    Always,
    Demo,
    Live,
}
use Show::{Always, Demo, Live};

struct Section {
    title: &'static str,
    rows: &'static [(&'static str, &'static str, Show)],
}

const SECTIONS: &[Section] = &[
    Section {
        title: "REPLY · while typing",
        rows: &[
            ("Enter", "send the reply (F5 also sends)", Always),
            (
                "Shift+Enter",
                "new line (also Ctrl+J, Ctrl/Alt+Enter)",
                Always,
            ),
            ("Up / Down", "save this reply and switch message", Always),
            ("Left / Right", "switch source, keeping the reply", Always),
            ("Alt+arrows", "move the cursor in the reply", Always),
            ("Delete", "dismiss and mark read, before typing", Always),
            ("Ctrl+E", "dismiss without replying (whole chat)", Always),
            ("Ctrl+A", "attach a harmless fixture", Demo),
            ("PageUp / Down", "scroll the preview", Always),
            ("Esc", "stop typing and browse; the draft stays", Always),
        ],
    },
    Section {
        title: "BROWSE · after Esc",
        rows: &[
            ("j / k, arrows", "select a message", Always),
            ("gg / G", "first / last message", Always),
            ("Ctrl+d / u", "page down / up", Always),
            ("i / r", "type a reply", Always),
            (
                "Left / Right",
                "cycle source: All, iCloud, Discord, Teams, Email, Office 365",
                Always,
            ),
            (
                "f",
                "focus: none (everything), Work, Home, your own",
                Always,
            ),
            ("S", "in a focus, show or hide filtered messages", Always),
            ("e / D", "dismiss or restore / show dismissed", Always),
            ("Delete", "dismiss and mark read", Always),
            ("u", "toggle unread", Always),
            ("/", "search the feed", Always),
            ("o", "read the whole message (full URLs)", Always),
            ("Ctrl+O", "read it in a Ghostty window", Always),
            ("s", "save the draft and reply to the selection", Always),
            ("d", "discard the draft (asks first)", Always),
            ("v", "open the Discord app", Always),
            ("a / x", "add / remove a fixture attachment", Demo),
            ("n", "simulate an incoming message", Demo),
            ("F6", "simulate drawer active / inactive", Demo),
            ("F7", "next mock outcome: success, failure, uncertain", Demo),
            (":", "command palette (Tab completes)", Always),
            (",", "settings", Always),
            ("t", "change palette", Always),
            ("?", "this help", Always),
            ("q / Ctrl+Q", "save and quit", Always),
        ],
    },
    Section {
        title: "MOUSE",
        rows: &[
            ("click a message", "select it", Always),
            ("click a link", "open it in the browser", Always),
            ("wheel", "scroll without selecting", Always),
        ],
    },
    Section {
        title: "READER · after o",
        rows: &[
            ("j / k, arrows", "scroll", Always),
            ("PageUp / Down", "scroll a page", Always),
            ("/", "search; Enter jumps to the next match", Always),
            ("q / Esc", "close the reader", Always),
        ],
    },
    Section {
        title: "COMMANDS · type : then Tab",
        rows: &[
            (":focus all|ID", "choose a focus", Always),
            (
                ":source NAME",
                "all, discord, teams, email, icloud, office365",
                Always,
            ),
            (":profile add|remove ID", "create or delete a focus", Always),
            (":profile prompt ID", "edit what a focus shows", Always),
            (
                ":spam prompt|person|for-you",
                "edit the filter questions",
                Always,
            ),
            (":about", "about you, for AI decisions", Always),
            (":accounts", "accounts and their status", Always),
            (":connections", "provider status", Live),
            (":account add [PROVIDER [ID]]", "add an account", Always),
            (":account rename ID NAME", "rename an account", Always),
            (
                ":account remove|enable|disable ID",
                "manage an account",
                Always,
            ),
            (":auth ACCOUNT|ai", "sign in, or set the AI key", Always),
            (
                ":resolve accepted|not-sent",
                "settle an uncertain send",
                Live,
            ),
            (
                ":fail · :uncertain · :reconcile",
                "mock send outcomes",
                Demo,
            ),
            (
                ":attach · :remove · :incoming",
                "demo attachments and arrivals",
                Demo,
            ),
            (":follow · :save-switch · :discard", "reply context", Always),
            (":update", "install the latest release", Always),
            (
                ":theme · :motion · :preview",
                "appearance and reader",
                Always,
            ),
            (":quit", "save and quit", Always),
        ],
    },
];

const LIVE_NOTES: &str = "Replies go through the account shown above the reply box; it needs accounts.ID.enabled and allow_send. After an uncertain send, check the original client, then :resolve. Attachments are listed for reference only.";
const DEMO_NOTES: &str =
    "Demo messages and sends are fictional. Notification capture is read-only.";

/// Help laid out for `width` cells.
pub(super) fn lines(app: &App, width: usize, p: Palette) -> Vec<Line<'static>> {
    let live = app.live_service || app.live_feed || app.live_setup;
    let shown = |show: Show| show == Always || (show == Live) == live;
    let width = width.max(1);
    let key_style = Style::default().fg(p.second);
    let text_style = Style::default().fg(p.fg);
    let rows = || {
        SECTIONS
            .iter()
            .flat_map(|s| s.rows.iter())
            .filter(|r| shown(r.2))
    };
    // The key column fits the common short keys; longer ones (commands) go on their own line.
    let column = rows()
        .map(|r| r.0.width())
        .filter(|w| *w <= 16)
        .max()
        .unwrap_or(0)
        + 2;
    let columns = width >= column + 24;
    let mut out = vec![];
    for section in SECTIONS {
        if !out.is_empty() {
            out.push(Line::default());
        }
        out.push(Line::styled(
            section.title,
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
        ));
        for (key, does, show) in section.rows {
            if !shown(*show) {
                continue;
            }
            if columns && key.width() < column {
                let pad = column - key.width();
                for (index, line) in wrapped(does, width - column).into_iter().enumerate() {
                    out.push(if index == 0 {
                        Line::from(vec![
                            Span::styled(format!("{key}{}", " ".repeat(pad)), key_style),
                            Span::styled(line, text_style),
                        ])
                    } else {
                        Line::styled(format!("{}{line}", " ".repeat(column)), text_style)
                    });
                }
            } else {
                for line in wrapped(key, width) {
                    out.push(Line::styled(line, key_style));
                }
                let indent = 2.min(width.saturating_sub(1));
                for line in wrapped(does, width - indent) {
                    out.push(Line::styled(
                        format!("{}{line}", " ".repeat(indent)),
                        text_style,
                    ));
                }
            }
        }
    }
    out.push(Line::default());
    for line in wrapped(if live { LIVE_NOTES } else { DEMO_NOTES }, width) {
        out.push(Line::styled(line, Style::default().fg(p.muted)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{app::Session, config::Settings};

    fn render(app: &App, width: usize) -> Vec<String> {
        let p = palette(Theme::Midnight, Duration::ZERO, true);
        lines(app, width, p)
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn help_uses_columns_when_wide_and_stacks_keys_when_narrow() {
        let app = App::new(Session::default(), Settings::default()).unwrap();
        let wide = render(&app, 80);
        let row = wide.iter().find(|l| l.starts_with("Shift+Enter")).unwrap();
        assert!(row.contains("new line"), "{row}");
        let narrow = render(&app, 30);
        let at = narrow.iter().position(|l| l == "Shift+Enter").unwrap();
        assert!(narrow[at + 1].starts_with("  new line"), "{narrow:#?}");
        for lines in [&wide, &narrow] {
            for line in lines.iter() {
                let limit = if lines.len() == wide.len() { 80 } else { 30 };
                assert!(line.width() <= limit, "{line}");
            }
        }
    }

    #[test]
    fn live_help_leaves_out_demo_controls() {
        let mut app = App::new(Session::default(), Settings::default()).unwrap();
        let demo = render(&app, 80).join("\n");
        assert!(demo.contains("simulate an incoming message"));
        assert!(!demo.contains(":resolve"));
        app.live_service = true;
        let live = render(&app, 80).join("\n");
        assert!(
            !live.contains("simulate") && !live.contains("fixture"),
            "{live}"
        );
        assert!(live.contains(":resolve accepted|not-sent"));
        assert!(live.contains("click a link"));
    }
}
