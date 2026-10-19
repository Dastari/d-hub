//! Links in message text. Rendered HTML keeps anchors as `[text][n]` with `[n]: url` lines at
//! the end, and plain text carries bare URLs. Previews show the anchor text, or just the host
//! for a bare URL, and a click opens the link in the desktop's browser or mail client.
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Text and the link it belongs to, if any.
pub(super) type Piece = (String, Option<String>);
/// Pieces with no whitespace between them, such as a link followed by a full stop.
type Word = Vec<Piece>;

/// Source lines (blank lines and the `[n]: url` reference list removed) as words.
pub(super) fn parse(body: &str) -> Vec<Vec<Word>> {
    let mut references = std::collections::HashMap::new();
    let mut lines = vec![];
    for line in body.lines() {
        if let Some((number, url)) = reference(line) {
            references.insert(number, url);
        } else {
            lines.push(line);
        }
    }
    lines
        .into_iter()
        .filter(|line| !line.trim().is_empty() && !crate::domain::rule_line(line))
        .map(|line| words(&segments(line, &references)))
        .collect()
}

/// Each source line as plain text, links shown as their text or host.
pub(super) fn plain(body: &str) -> Vec<String> {
    parse(body)
        .iter()
        .map(|words| {
            words
                .iter()
                .map(|word| word.iter().map(|(t, _)| t.as_str()).collect::<String>())
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect()
}

/// `[12]: https://…` as written by the HTML renderer.
fn reference(line: &str) -> Option<(String, Option<String>)> {
    let rest = line.trim().strip_prefix('[')?;
    let (number, url) = rest.split_once("]: ")?;
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) || url.contains(' ') {
        return None;
    }
    Some((number.into(), openable(url).then(|| url.to_owned())))
}

/// Splits a line into plain and linked runs.
fn segments(
    line: &str,
    references: &std::collections::HashMap<String, Option<String>>,
) -> Vec<Piece> {
    let mut out: Vec<Piece> = vec![];
    let mut plain = String::new();
    let mut rest = line;
    while let Some(c) = rest.chars().next() {
        if c == '['
            && let Some((label, number, after)) = anchor(rest)
        {
            push_plain(&mut out, &mut plain);
            // A reference cut off with a long body still reads as its text.
            match references.get(number).cloned().flatten() {
                // Anchor text that is itself a URL reads better as its host.
                Some(url) if label.trim().is_empty() || openable(label.trim()) => {
                    out.push((short(&url), Some(url)))
                }
                Some(url) => out.push((label.to_owned(), Some(url))),
                None => plain.push_str(label),
            }
            rest = after;
            continue;
        }
        let boundary = plain.chars().last().is_none_or(|p| !p.is_alphanumeric());
        if boundary && (rest.starts_with("https://") || rest.starts_with("http://")) {
            let end = rest
                .find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"'))
                .unwrap_or(rest.len());
            let mut url = &rest[..end];
            // Sentence punctuation after a URL is not part of it; a closing bracket only is
            // when the URL opened one.
            while let Some(last) = url.chars().last() {
                let unbalanced = last == ')' && url.matches('(').count() < url.matches(')').count();
                if matches!(last, '.' | ',' | ';' | ':' | '!' | '?' | '\'' | ']') || unbalanced {
                    url = &url[..url.len() - last.len_utf8()];
                } else {
                    break;
                }
            }
            if openable(url) {
                // "<https://…>" is how plain-text mail marks a URL; the brackets add nothing.
                let wrapped = plain.ends_with('<') && rest[url.len()..].starts_with('>');
                if wrapped {
                    plain.pop();
                }
                push_plain(&mut out, &mut plain);
                out.push((short(url), Some(url.to_owned())));
                rest = &rest[url.len() + usize::from(wrapped)..];
                continue;
            }
        }
        plain.push(c);
        rest = &rest[c.len_utf8()..];
    }
    push_plain(&mut out, &mut plain);
    out
}

fn push_plain(out: &mut Vec<Piece>, plain: &mut String) {
    if !plain.is_empty() {
        out.push((std::mem::take(plain), None));
    }
}

/// `[label][number]` at the start of `text`: the label, number and what follows.
fn anchor(text: &str) -> Option<(&str, &str, &str)> {
    let close = text.find(']')?;
    let label = &text[1..close];
    if label.contains('[') {
        return None;
    }
    let after = text[close + 1..].strip_prefix('[')?;
    let end = after.find(']')?;
    let number = &after[..end];
    (!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
        .then(|| (label, number, &after[end + 1..]))
}

/// Groups runs into whitespace-separated words, so a link's own spaces stay inside the link.
fn words(segments: &[Piece]) -> Vec<Word> {
    let mut words: Vec<Word> = vec![];
    let mut joined = false;
    for (text, link) in segments {
        for (index, part) in text.split(char::is_whitespace).enumerate() {
            if index > 0 {
                joined = false;
            }
            if part.is_empty() {
                continue;
            }
            match words.last_mut() {
                Some(word) if joined => word.push((part.into(), link.clone())),
                _ => words.push(vec![(part.into(), link.clone())]),
            }
            joined = true;
        }
    }
    words
}

/// Greedy wrap by display width. The space between two words of one link is part of the link.
pub(super) fn wrap(words: &[Word], width: usize) -> Vec<Vec<Piece>> {
    let width = width.max(1);
    let mut rows: Vec<Vec<Piece>> = vec![];
    let mut row: Vec<Piece> = vec![];
    let mut used = 0;
    for word in words {
        let cells: usize = word.iter().map(|(t, _)| t.width()).sum();
        if used > 0 && used + 1 + cells > width {
            rows.push(std::mem::take(&mut row));
            used = 0;
        }
        if used > 0 {
            let link = row.last().and_then(|(_, l)| l.clone());
            let same = link.is_some() && link == word[0].1;
            push(&mut row, " ", if same { link } else { None });
            used += 1;
        }
        for (text, link) in word {
            for grapheme in text.graphemes(true) {
                let w = grapheme.width();
                if used + w > width && used > 0 {
                    rows.push(std::mem::take(&mut row));
                    used = 0;
                }
                push(&mut row, grapheme, link.clone());
                used += w;
            }
        }
    }
    if !row.is_empty() || rows.is_empty() {
        rows.push(row);
    }
    rows
}

/// Appends to the last piece when it has the same link.
fn push(row: &mut Vec<Piece>, text: &str, link: Option<String>) {
    match row.last_mut() {
        Some((t, l)) if *l == link => t.push_str(text),
        _ => row.push((text.into(), link)),
    }
}

/// Only web and mail links open; nothing with spaces or control characters.
pub(super) fn openable(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let scheme = ["https://", "http://", "mailto:"]
        .iter()
        .find(|s| lower.starts_with(*s));
    scheme.is_some_and(|s| url.len() > s.len())
        && url.len() <= 4096
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The host (without `www.` or any `user@` prefix), with "/…" when there is more to the URL;
/// the address for mailto links.
pub(super) fn short(url: &str) -> String {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("mailto:") {
        let address = &url["mailto:".len()..];
        return address.split('?').next().unwrap_or(address).to_owned();
    }
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.strip_prefix("www.").unwrap_or(host);
    let more = rest[end..].trim_start_matches('/');
    if more.is_empty() {
        host.to_owned()
    } else {
        format!("{host}/…")
    }
}

/// Programs that hand a URL to the desktop, first available wins. WSL uses Windows' browser.
fn openers() -> &'static [&'static str] {
    let wsl = std::env::var_os("WSL_DISTRO_NAME").is_some()
        || std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .is_ok_and(|v| v.to_ascii_lowercase().contains("microsoft"));
    if wsl {
        &["wslview", "explorer.exe", "xdg-open"]
    } else if cfg!(target_os = "macos") {
        &["open"]
    } else {
        &["xdg-open"]
    }
}

/// Opens a link with the desktop's handler, passing it as a single argument.
pub(super) fn open(url: &str) -> Result<(), String> {
    if !openable(url) {
        return Err("Only web and mail links open.".into());
    }
    for program in openers() {
        match launch(program, url) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("{program} failed: {error}")),
        }
    }
    Err(format!(
        "No link opener found (tried {}).",
        openers().join(", ")
    ))
}

#[cfg(not(test))]
fn launch(program: &str, url: &str) -> std::io::Result<()> {
    let mut child = std::process::Command::new(program)
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
thread_local! {
    /// Tests record launches instead of opening a browser.
    pub(super) static LAUNCHED: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(vec![]) };
}

#[cfg(test)]
fn launch(_: &str, url: &str) -> std::io::Result<()> {
    LAUNCHED.with(|l| l.borrow_mut().push(url.to_owned()));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(body: &str) -> Vec<Piece> {
        let parsed = parse(body);
        wrap(&parsed[0], 200).remove(0)
    }

    #[test]
    fn anchors_show_their_text_and_bare_urls_their_host() {
        let body = "Hi [view your order][1] and [https://x.io][2]. [mail me][3] [logo] [x][4][y][9]\n\
                    \n[1]: https://example.com/a?b=1&c=2\n[2]: https://x.io\n[3]: mailto:a@b.c\n[4]: cid:abc";
        assert_eq!(
            line(body),
            vec![
                ("Hi ".into(), None),
                (
                    "view your order".into(),
                    Some("https://example.com/a?b=1&c=2".into())
                ),
                (" and ".into(), None),
                ("x.io".into(), Some("https://x.io".into())),
                (". ".into(), None),
                ("mail me".into(), Some("mailto:a@b.c".into())),
                (" [logo] xy".into(), None),
            ]
        );
        assert_eq!(parse(body).len(), 1, "the reference list is not shown");
        assert_eq!(
            plain(body),
            ["Hi view your order and x.io. mail me [logo] xy"]
        );
        let plain = line("See <https://www.plain.example/long/path?q=1>, or (https://a.io/x_(y)).");
        assert_eq!(
            plain,
            vec![
                ("See ".into(), None),
                (
                    "plain.example/…".into(),
                    Some("https://www.plain.example/long/path?q=1".into())
                ),
                (", or (".into(), None),
                ("a.io/…".into(), Some("https://a.io/x_(y)".into())),
                (").".into(), None),
            ]
        );
        // Not a URL inside a word, and the shown host is the real one.
        assert_eq!(line("xhttps://a.io"), vec![("xhttps://a.io".into(), None)]);
        assert_eq!(
            short("https://bank.com@evil.example/login"),
            "evil.example/…"
        );
        assert_eq!(short("mailto:a@b.c?subject=hi"), "a@b.c");
    }

    #[test]
    fn wrapping_keeps_link_spaces_inside_the_link_and_splits_long_words() {
        let words = &parse("[one two three][1] end\n[1]: https://a.io")[0];
        let rows = wrap(words, 9);
        assert_eq!(
            rows,
            vec![
                vec![("one two".into(), Some("https://a.io".into()))],
                vec![
                    ("three".into(), Some("https://a.io".into())),
                    (" end".into(), None)
                ],
            ]
        );
        let rows = wrap(&parse("abcdefghij")[0], 4);
        assert_eq!(rows.len(), 3);
    }

    #[test]
    fn only_web_and_mail_links_open_and_never_through_a_shell() {
        for bad in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "https://a.io/x y",
            "https://",
            "-x",
        ] {
            assert!(open(bad).is_err(), "{bad}");
        }
        LAUNCHED.with(|l| l.borrow_mut().clear());
        assert!(open("https://example.com/a?b=1&c=$(rm)").is_ok());
        LAUNCHED.with(|l| {
            assert_eq!(
                *l.borrow(),
                vec!["https://example.com/a?b=1&c=$(rm)".to_owned()]
            )
        });
    }
}
