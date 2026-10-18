use super::*;
use anyhow::Context;
use imap::types::Flag;
use mailparse::MailHeaderMap;
use std::{
    net::{TcpStream, ToSocketAddrs},
    time::Duration,
};
type Session = imap::Session<native_tls::TlsStream<TcpStream>>;
const HOST: &str = "imap.mail.me.com";
fn connect(account: &AccountConfig, password: &str) -> Result<Session> {
    if account.username.parse::<lettre::Address>().is_err() {
        bail!("Set the full iCloud email address in account username");
    }
    let address = (HOST, 993)
        .to_socket_addrs()?
        .next()
        .context("iCloud address unavailable")?;
    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(8))
        .context("iCloud connection failed")?;
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    stream.set_write_timeout(Some(Duration::from_secs(15)))?;
    let tls = native_tls::TlsConnector::new()?
        .connect(HOST, stream)
        .map_err(|_| anyhow::anyhow!("iCloud TLS validation failed"))?;
    let mut client = imap::Client::new(tls);
    client.read_greeting().context("iCloud greeting failed")?;
    client.login(&account.username, password).map_err(|(e, _)| {
        anyhow::anyhow!(
            "iCloud login refused ({}); check app-specific password and username",
            crate::domain::safe_text(&e.to_string())
        )
    })
}
fn body(part: &mailparse::ParsedMail<'_>, wanted: &str) -> Option<String> {
    if part.get_content_disposition().disposition == mailparse::DispositionType::Attachment {
        return None;
    }
    if part.ctype.mimetype == wanted {
        return part.get_body().ok();
    }
    part.subparts.iter().find_map(|p| body(p, wanted))
}
fn addresses(value: &str) -> Vec<String> {
    fn collect(list: &mailparse::MailAddrList) -> Vec<String> {
        list.iter()
            .flat_map(|a| match a {
                mailparse::MailAddr::Single(a) => vec![a.addr.clone()],
                mailparse::MailAddr::Group(g) => g.addrs.iter().map(|a| a.addr.clone()).collect(),
            })
            .filter(|a| a.parse::<lettre::Address>().is_ok())
            .collect()
    }
    mailparse::addrparse(value)
        .map(|a| collect(&a))
        .unwrap_or_default()
}
pub fn parse(
    account: &AccountConfig,
    mailbox: &str,
    validity: u32,
    uid: u32,
    unread: bool,
    raw: &[u8],
) -> Result<Item> {
    let mail = mailparse::parse_mail(raw).context("Invalid MIME message")?;
    let header = |name| mail.headers.get_first_value(name).unwrap_or_default();
    let reply = header("Reply-To");
    let from = header("From");
    let recipients = addresses(if reply.is_empty() { &from } else { &reply });
    let mut rendered = body(&mail, "text/plain")
        .map(|s| text(&s))
        .or_else(|| body(&mail, "text/html").map(|s| html(&s)))
        .unwrap_or_else(|| "[No supported text body]".into());
    let attachments: Vec<_> = mail
        .subparts
        .iter()
        .filter_map(|p| {
            let d = p.get_content_disposition();
            (d.disposition == mailparse::DispositionType::Attachment).then(|| {
                d.params
                    .get("filename")
                    .cloned()
                    .unwrap_or_else(|| "unnamed file".into())
            })
        })
        .collect();
    for name in attachments {
        rendered.push_str(&format!(
            "\n[Attachment: {} · open original client to download]",
            safe_text(&name)
        ));
    }
    Ok(Item {
        account_scope: scope(account),
        remote: RemoteMessage::Imap {
            account: account.id.clone(),
            mailbox: mailbox.into(),
            uid_validity: validity,
            uid,
        },
        sender: header("From"),
        recipients,
        subject: header("Subject"),
        body: text(&rendered),
        timestamp: mailparse::dateparse(&header("Date")).unwrap_or(0).max(0) as u64,
        unread: Some(unread),
        message_id: header("Message-ID"),
        references: header("References"),
        deleted: false,
        partial: false,
    })
}
/// Read-only: sign in, list folders, examine the synced ones and test SMTP sign-in.
pub fn check(account: &AccountConfig, password: &str) -> Vec<check::Step> {
    let mut steps = match connect(account, password) {
        Ok(mut session) => {
            let mut steps = vec![check::Step::ok(
                "Sign in",
                format!("{HOST} accepted {}", account.username),
            )];
            steps.extend(check_session(account, &mut session));
            let _ = session.logout();
            steps
        }
        Err(e) => return vec![check::Step::failed("Sign in", e)],
    };
    steps.push(check_smtp(account, password));
    steps
}
fn check_session<T: std::io::Read + std::io::Write>(
    account: &AccountConfig,
    session: &mut imap::Session<T>,
) -> Vec<check::Step> {
    let mut steps = vec![];
    match session.list(None, Some("*")) {
        Ok(names) => {
            let names: Vec<_> = names.iter().map(|n| n.name().to_owned()).collect();
            steps.push(
                check::Step::ok("Folders", format!("{} folders", names.len())).with_items(names),
            );
        }
        Err(e) => steps.push(check::Step::failed("Folders", e)),
    }
    let folders = if account.resources.is_empty() {
        vec!["INBOX".to_owned()]
    } else {
        account.resources.clone()
    };
    for folder in folders {
        let name = format!("Folder {folder}");
        steps.push(match session.examine(&folder) {
            Ok(mailbox) => check::Step::ok(name, format!("{} messages", mailbox.exists)),
            Err(e) => check::Step::failed(name, format!("{e}; check the folder name")),
        });
    }
    steps
}
/// EHLO, STARTTLS and AUTH, then QUIT; no message is submitted.
fn check_smtp(account: &AccountConfig, password: &str) -> check::Step {
    let transport = match lettre::SmtpTransport::starttls_relay("smtp.mail.me.com") {
        Ok(t) => t
            .port(587)
            .timeout(Some(Duration::from_secs(20)))
            .credentials(lettre::transport::smtp::authentication::Credentials::new(
                account.username.clone(),
                password.into(),
            ))
            .build(),
        Err(e) => return check::Step::failed("Sending", e),
    };
    match transport.test_connection() {
        Ok(true) => check::Step::ok("Sending", "smtp.mail.me.com accepted sign-in; nothing sent"),
        Ok(false) => check::Step::failed("Sending", "smtp.mail.me.com did not respond"),
        Err(e) => check::Step::failed("Sending", e),
    }
}
pub fn sync(account: &AccountConfig, password: &str, cursor: Value) -> Result<Batch> {
    let mut session = connect(account, password)?;
    let result = sync_session(account, &mut session, cursor);
    let _ = session.logout();
    result
}
fn sync_session<T: std::io::Read + std::io::Write>(
    account: &AccountConfig,
    session: &mut imap::Session<T>,
    mut cursor: Value,
) -> Result<Batch> {
    let folders = if account.resources.is_empty() {
        vec!["INBOX".into()]
    } else {
        account.resources.clone()
    };
    if cursor.is_null() {
        cursor = serde_json::json!({});
    }
    let mut items = Vec::new();
    let mut backlog = false;
    for folder in folders {
        let mailbox = session
            .select(&folder)
            .with_context(|| format!("iCloud mailbox {folder:?} unavailable"))?;
        let validity = mailbox.uid_validity.context("iCloud omitted UIDVALIDITY")?;
        let old = &cursor[&folder];
        let reset = old["validity"].as_u64() != Some(validity as u64);
        let previous = old["uid"].as_u64().unwrap_or(0) as u32;
        let recent = if mailbox.exists > 0 {
            Some(session.fetch(
                format!("{}:*", mailbox.exists.saturating_sub(199).max(1)),
                "(UID FLAGS RFC822.SIZE INTERNALDATE)",
            )?)
        } else {
            None
        };
        let mut latest: Vec<_> = recent
            .as_ref()
            .into_iter()
            .flat_map(|m| m.iter())
            .filter_map(|m| m.uid)
            .collect();
        latest.sort_unstable();
        let maximum = latest.last().copied().unwrap_or(previous);
        let end = maximum.min(previous.saturating_add(500));
        let mut new = if reset {
            latest.iter().rev().take(50).copied().collect::<Vec<_>>()
        } else if end > previous {
            session
                .uid_search(format!("UID {}:{end}", previous + 1))?
                .into_iter()
                .collect()
        } else {
            Vec::new()
        };
        new.sort_unstable();
        let high = if reset {
            maximum
        } else if new.len() > 50 {
            new[49]
        } else {
            end.max(previous)
        };
        new.truncate(50);
        backlog |= maximum > high;
        // Fetch recent flags in one bounded metadata request; bodies only for newly imported UIDs.
        for meta in recent.as_ref().into_iter().flat_map(|m| m.iter()) {
            if let Some(uid) = meta.uid {
                items.push(Item {
                    account_scope: scope(account),
                    remote: RemoteMessage::Imap {
                        account: account.id.clone(),
                        mailbox: folder.clone(),
                        uid_validity: validity,
                        uid,
                    },
                    sender: String::new(),
                    recipients: vec![],
                    subject: String::new(),
                    body: String::new(),
                    timestamp: 0,
                    unread: Some(!meta.flags().contains(&Flag::Seen)),
                    message_id: String::new(),
                    references: String::new(),
                    deleted: false,
                    partial: true,
                });
            }
        }
        for uid in new {
            let fetched_meta =
                session.uid_fetch(uid.to_string(), "(UID FLAGS RFC822.SIZE INTERNALDATE)")?;
            let Some(meta) = fetched_meta.iter().find(|m| m.uid == Some(uid)) else {
                continue;
            };
            let oversized = meta.size.unwrap_or(u32::MAX) > 1_048_576;
            let unread = !meta.flags().contains(&Flag::Seen);
            let received = meta.internal_date().map(|d| d.timestamp().max(0) as u64);
            let fetched = session.uid_fetch(
                uid.to_string(),
                if oversized {
                    "(UID BODY.PEEK[HEADER])"
                } else {
                    "(UID BODY.PEEK[])"
                },
            )?;
            if let Some(raw) = fetched
                .iter()
                .find(|f| f.uid == Some(uid))
                .and_then(|f| if oversized { f.header() } else { f.body() })
            {
                let mut item = parse(account, &folder, validity, uid, unread, raw)?;
                if let Some(received) = received {
                    item.timestamp = received;
                }
                if oversized {
                    item.body="[Message exceeds one MiB; open the original client for its body and attachments]".into();
                }
                items.push(item);
            } else {
                bail!("Message body disappeared during sync; cursor retained for retry");
            }
        }
        if let Some(known) = old["known"].as_array() {
            let ids: Vec<_> = known
                .iter()
                .filter_map(Value::as_u64)
                .filter_map(|u| u32::try_from(u).ok())
                .collect();
            if !ids.is_empty() {
                let remaining = if reset {
                    std::collections::HashSet::new()
                } else {
                    session.uid_search(format!(
                        "UID {}",
                        ids.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
                    ))?
                };
                for uid in ids.into_iter().filter(|uid| !remaining.contains(uid)) {
                    items.push(Item {
                        account_scope: scope(account),
                        remote: RemoteMessage::Imap {
                            account: account.id.clone(),
                            mailbox: folder.clone(),
                            uid_validity: old["validity"].as_u64().unwrap_or(validity as u64)
                                as u32,
                            uid,
                        },
                        sender: String::new(),
                        recipients: vec![],
                        subject: String::new(),
                        body: String::new(),
                        timestamp: 0,
                        unread: None,
                        message_id: String::new(),
                        references: String::new(),
                        deleted: true,
                        partial: false,
                    });
                }
            }
        }
        cursor[&folder] = serde_json::json!({"validity":validity,"uid":high,"known":latest});
    }
    Ok(Batch {
        items,
        cursor,
        status: if backlog {
            "Catching up · recent read flags synchronized"
        } else {
            "Connected · recent 200 read flags synchronized"
        }
        .into(),
    })
}
pub fn unread(
    account: &AccountConfig,
    password: &str,
    remote: &RemoteMessage,
    unread: bool,
) -> Result<()> {
    let RemoteMessage::Imap {
        account: id,
        mailbox,
        uid_validity,
        uid,
    } = remote
    else {
        bail!("Invalid iCloud identity")
    };
    if id != &account.id {
        bail!("Account mismatch");
    }
    let mut session = connect(account, password)?;
    let selected = session.select(mailbox)?;
    if selected.uid_validity != Some(*uid_validity) {
        bail!("Mailbox identity changed; original message is unavailable");
    }
    if session.uid_search(format!("UID {uid}"))?.is_empty() {
        bail!("Original message was removed");
    }
    session.uid_store(
        uid.to_string(),
        if unread {
            "-FLAGS.SILENT (\\Seen)"
        } else {
            "+FLAGS.SILENT (\\Seen)"
        },
    )?;
    let _ = session.logout();
    Ok(())
}
pub fn build_reply(
    account: &AccountConfig,
    item: &Item,
    submission: &Submission,
) -> Result<lettre::Message> {
    let mut builder = lettre::Message::builder()
        .from(account.username.parse()?)
        .subject(if item.subject.to_lowercase().starts_with("re:") {
            item.subject.clone()
        } else {
            format!("Re: {}", item.subject)
        })
        .message_id(None);
    for recipient in &submission.draft.target.recipients {
        builder = builder.to(recipient.parse()?);
    }
    if item.message_id.starts_with('<')
        && item.message_id.ends_with('>')
        && !item.message_id.contains(['\r', '\n'])
    {
        builder = builder.in_reply_to(item.message_id.clone()).references(
            format!("{} {}", item.references, item.message_id)
                .trim()
                .into(),
        );
    }
    // A bare body has no Content-Type, so readers would assume US-ASCII and garble Unicode.
    Ok(builder.singlepart(lettre::message::SinglePart::plain(
        submission.draft.body.clone(),
    ))?)
}
pub fn send(
    account: &AccountConfig,
    password: &str,
    item: &Item,
    submission: &Submission,
) -> SendResult {
    let mail = match build_reply(account, item, submission) {
        Ok(v) => v,
        Err(_) => return SendResult::Failed("Invalid sender, recipient or reply headers".into()),
    };
    // Check UIDVALIDITY again immediately before submitting; a reset cannot redirect a reply.
    let validation = (|| -> Result<()> {
        let RemoteMessage::Imap {
            mailbox,
            uid_validity,
            uid,
            ..
        } = &item.remote
        else {
            bail!("Invalid identity")
        };
        let mut session = connect(account, password)?;
        if session.select(mailbox)?.uid_validity != Some(*uid_validity)
            || session.uid_search(format!("UID {uid}"))?.is_empty()
        {
            bail!("Original message is unavailable");
        }
        let _ = session.logout();
        Ok(())
    })();
    if validation.is_err() {
        return SendResult::Failed(
            "Cannot validate original message; reconnect or use the original client".into(),
        );
    }
    let transport = match lettre::SmtpTransport::starttls_relay("smtp.mail.me.com") {
        Ok(t) => t
            .port(587)
            .timeout(Some(Duration::from_secs(20)))
            .credentials(lettre::transport::smtp::authentication::Credentials::new(
                account.username.clone(),
                password.into(),
            ))
            .build(),
        Err(_) => return SendResult::Failed("SMTP TLS setup failed".into()),
    };
    deliver(&transport, &mail)
}
fn deliver(transport: &lettre::SmtpTransport, mail: &lettre::Message) -> SendResult {
    use lettre::Transport;
    match transport.send(mail) {
        Ok(_) => SendResult::Accepted,
        Err(e) if e.status().is_some() => SendResult::Failed(format!(
            "SMTP refused reply ({}); verify account, recipient and server policy",
            crate::domain::safe_text(&e.to_string())
        )),
        // Connecting and the TLS handshake precede any mail data, so nothing was sent.
        Err(e) if e.is_tls() || e.to_string().starts_with("Connection error") => {
            SendResult::Failed("Could not reach iCloud mail; reply not sent".into())
        }
        Err(_) => SendResult::Uncertain,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
    };
    fn account() -> AccountConfig {
        let mut a = AccountConfig::new("mail-test", Provider::ICloud);
        a.username = "owner@example.invalid".into();
        a
    }
    #[test]
    fn mime_decodes_alternatives_reply_to_unicode_and_removes_terminal_sequences() {
        let raw=b"From: Sender <sender@example.invalid>\r\nReply-To: Replies <reply@example.invalid>\r\nSubject: =?UTF-8?Q?Caf=C3=A9?=\r\nMessage-ID: <original@example.invalid>\r\nReferences: <earlier@example.invalid>\r\nDate: Sun, 18 Oct 2026 10:00:00 +0000\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=outer\r\n\r\n--outer\r\nContent-Type: multipart/alternative; boundary=inner\r\n\r\n--inner\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>HTML fallback</p>\r\n--inner\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: quoted-printable\r\n\r\nPlain caf=C3=A9 =1B[31mtext\r\n--inner--\r\n--outer\r\nContent-Type: application/pdf\r\nContent-Disposition: attachment; filename=fiction.pdf\r\n\r\nfictional bytes\r\n--outer--\r\n";
        let a = account();
        let item = parse(&a, "INBOX", 7, 4, true, raw).unwrap();
        assert_eq!(item.subject, "Café");
        assert_eq!(item.recipients, vec!["reply@example.invalid"]);
        assert!(item.body.contains("Plain café text"));
        assert!(!item.body.contains("HTML fallback"));
        assert!(item.body.contains("fiction.pdf"));
        let draft = crate::composer::Draft {
            target: item.message(MessageId(1), &a).target.unwrap(),
            body: "Reply 東京".into(),
            attachments: vec![],
            revision: 1,
        };
        let mail = build_reply(
            &a,
            &item,
            &Submission {
                operation: 1,
                draft,
            },
        )
        .unwrap();
        let wire = String::from_utf8(mail.formatted()).unwrap();
        assert!(wire.contains("In-Reply-To: <original@example.invalid>"));
        assert!(wire.contains("To: reply@example.invalid"));
        assert!(!wire.contains("To: sender@example.invalid"));
        assert!(wire.contains("References: <earlier@example.invalid> <original@example.invalid>"));
        assert!(wire.contains("MIME-Version: 1.0"));
        assert!(wire.contains("Content-Type: text/plain; charset=utf-8"));
        let parsed = mailparse::parse_mail(wire.as_bytes()).unwrap();
        assert_eq!(parsed.get_body().unwrap().trim_end(), "Reply 東京");
    }
    #[test]
    fn fake_imap_uses_uids_peek_size_guard_and_detects_uidvalidity_reset() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let commands = Arc::new(Mutex::new(Vec::new()));
        let observed = commands.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            write!(stream, "* OK fictional IMAP\r\n").unwrap();
            let mut selects = 0;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                observed.lock().unwrap().push(line.clone());
                let tag = line.split_whitespace().next().unwrap();
                let upper = line.to_uppercase();
                if upper.contains("LOGIN") {
                } else if upper.contains("SELECT") {
                    selects += 1;
                    write!(
                        stream,
                        "* 1 EXISTS\r\n* OK [UIDVALIDITY {}] validity\r\n",
                        if selects == 1 { 77 } else { 88 }
                    )
                    .unwrap();
                } else if upper.contains("UID SEARCH") {
                    write!(stream, "* SEARCH 3\r\n").unwrap();
                } else if upper.contains("RFC822.SIZE") {
                    write!(stream,"* 1 FETCH (UID 3 FLAGS () RFC822.SIZE 2000000 INTERNALDATE \"18-Oct-2026 10:00:00 +0000\")\r\n").unwrap();
                } else if upper.contains("BODY.PEEK[HEADER]") {
                    let raw=b"From: Fiction <fiction@example.invalid>\r\nSubject: Big mail\r\nMessage-ID: <big@example.invalid>\r\n\r\n";
                    write!(
                        stream,
                        "* 1 FETCH (UID 3 BODY[HEADER] {{{}}}\r\n",
                        raw.len()
                    )
                    .unwrap();
                    stream.write_all(raw).unwrap();
                    write!(stream, ")\r\n").unwrap();
                } else {
                    panic!("Unexpected IMAP command: {upper}");
                }
                write!(stream, "{tag} OK complete\r\n").unwrap();
            }
        });
        let mut client = imap::Client::new(TcpStream::connect(address).unwrap());
        client.read_greeting().unwrap();
        let mut session = client.login("fictional", "fictional").unwrap();
        let a = account();
        let first = sync_session(&a, &mut session, Value::Null).unwrap();
        assert_eq!(first.items.iter().filter(|m| !m.partial).count(), 1);
        assert!(
            first
                .items
                .iter()
                .find(|m| !m.partial)
                .unwrap()
                .body
                .contains("exceeds one MiB")
        );
        assert_eq!(first.cursor["INBOX"]["validity"], 77);
        let second = sync_session(&a, &mut session, first.cursor).unwrap();
        assert_eq!(second.cursor["INBOX"]["validity"], 88);
        assert_ne!(
            first.items.iter().find(|m| !m.partial).unwrap().remote,
            second
                .items
                .iter()
                .find(|m| !m.partial && !m.deleted)
                .unwrap()
                .remote
        );
        drop(session);
        server.join().unwrap();
        let commands = commands.lock().unwrap();
        assert!(commands.iter().all(|c| !c.contains("BODY[]")));
        assert!(commands.iter().any(|c| c.contains("BODY.PEEK[HEADER]")));
    }
    fn smtp(accept: bool) -> (lettre::SmtpTransport, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            write!(stream, "220 fictional SMTP\r\n").unwrap();
            let mut data = false;
            let mut body = String::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                if data {
                    if line == ".\r\n" {
                        if !accept {
                            return body;
                        }
                        write!(stream, "250 accepted\r\n").unwrap();
                        data = false;
                    } else {
                        body.push_str(&line);
                    }
                } else if line.starts_with("EHLO") {
                    write!(stream, "250 fictional\r\n").unwrap();
                } else if line.starts_with("MAIL FROM") || line.starts_with("RCPT TO") {
                    write!(stream, "250 ok\r\n").unwrap();
                } else if line.starts_with("DATA") {
                    write!(stream, "354 continue\r\n").unwrap();
                    data = true;
                } else if line.starts_with("QUIT") {
                    write!(stream, "221 goodbye\r\n").unwrap();
                    break;
                } else {
                    panic!("Unexpected SMTP command");
                }
            }
            body
        });
        (
            lettre::SmtpTransport::builder_dangerous("127.0.0.1")
                .port(address.port())
                .timeout(Some(Duration::from_secs(2)))
                .build(),
            server,
        )
    }
    #[test]
    fn fake_smtp_acceptance_and_disconnect_after_data_are_distinct() {
        let a = account();
        let item = parse(
            &a,
            "INBOX",
            1,
            1,
            true,
            b"From: sender@example.invalid\r\nSubject: Fiction\r\n\r\nOriginal",
        )
        .unwrap();
        let draft = crate::composer::Draft {
            target: item.message(MessageId(1), &a).target.unwrap(),
            body: "Fictional reply".into(),
            attachments: vec![],
            revision: 1,
        };
        let message = build_reply(
            &a,
            &item,
            &Submission {
                operation: 1,
                draft,
            },
        )
        .unwrap();
        for accepted in [true, false] {
            let (transport, server) = smtp(accepted);
            assert_eq!(
                deliver(&transport, &message),
                if accepted {
                    SendResult::Accepted
                } else {
                    SendResult::Uncertain
                }
            );
            let body = server.join().unwrap();
            assert!(body.contains("Fictional reply"));
            assert!(body.contains("To: sender@example.invalid"));
        }
    }
    #[test]
    fn an_smtp_server_that_cannot_be_reached_means_not_sent() {
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = closed.local_addr().unwrap().port();
        drop(closed);
        let transport = lettre::SmtpTransport::builder_dangerous("127.0.0.1")
            .port(port)
            .timeout(Some(Duration::from_secs(2)))
            .build();
        let message = lettre::Message::builder()
            .from("owner@example.invalid".parse().unwrap())
            .to("friend@example.invalid".parse().unwrap())
            .body(String::from("Fictional"))
            .unwrap();
        assert!(matches!(
            deliver(&transport, &message),
            SendResult::Failed(reason) if reason.contains("not sent")
        ));
    }
    #[test]
    fn the_connection_check_lists_folders_and_examines_without_selecting() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let commands = Arc::new(Mutex::new(Vec::new()));
        let observed = commands.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            write!(stream, "* OK fictional IMAP\r\n").unwrap();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 {
                    break;
                }
                observed.lock().unwrap().push(line.clone());
                let tag = line.split_whitespace().next().unwrap().to_owned();
                let upper = line.to_uppercase();
                if upper.contains("LIST") {
                    write!(
                        stream,
                        "* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n* LIST (\\HasNoChildren) \"/\" \"Sent Messages\"\r\n"
                    )
                    .unwrap();
                } else if upper.contains("EXAMINE \"INBOX\"") {
                    write!(stream, "* 42 EXISTS\r\n* OK [UIDVALIDITY 7] validity\r\n").unwrap();
                } else if upper.contains("EXAMINE") {
                    write!(stream, "{tag} NO [NONEXISTENT] Unknown Mailbox\r\n").unwrap();
                    continue;
                } else if upper.contains("LOGOUT") {
                    write!(stream, "* BYE\r\n{tag} OK bye\r\n").unwrap();
                    break;
                }
                write!(stream, "{tag} OK complete\r\n").unwrap();
            }
        });
        let mut client = imap::Client::new(TcpStream::connect(address).unwrap());
        client.read_greeting().unwrap();
        let mut session = client.login("fictional", "fictional").unwrap();
        let mut a = account();
        a.resources = vec!["INBOX".into(), "Archive".into()];
        let steps = check_session(&a, &mut session);
        let _ = session.logout();
        server.join().unwrap();
        assert!(steps[0].ok);
        assert_eq!(steps[0].items, vec!["INBOX", "Sent Messages"]);
        assert_eq!(
            (steps[1].ok, steps[1].detail.as_str()),
            (true, "42 messages")
        );
        assert!(
            !steps[2].ok && steps[2].detail.contains("Unknown Mailbox"),
            "{:?}",
            steps[2]
        );
        // Read-only: EXAMINE, never SELECT, FETCH or STORE.
        let commands = commands.lock().unwrap().join("").to_uppercase();
        for forbidden in ["SELECT", "FETCH", "STORE"] {
            assert!(!commands.contains(forbidden), "{commands}");
        }
    }
}
