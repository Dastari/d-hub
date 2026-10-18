use super::*;
use anyhow::Context;
use reqwest::{
    Method, Url,
    blocking::{Client, Response},
};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

pub struct Graph {
    client: Client,
    token: Option<(String, Zeroizing<String>, Instant)>,
    base: Url,
}
#[derive(Debug)]
pub struct Throttled(pub u64);
impl std::fmt::Display for Throttled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Microsoft throttled this account; retry in {} seconds",
            self.0
        )
    }
}
impl std::error::Error for Throttled {}
impl Graph {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: crate::auth::http_client(20_000)?,
            token: None,
            base: Url::parse("https://graph.microsoft.com/v1.0/")?,
        })
    }
    fn url(&self, parts: &[&str]) -> Url {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("fixed graph base")
            .pop_if_empty()
            .extend(parts);
        url
    }
    fn continuation(&self, value: &str) -> Result<Url> {
        let url = Url::parse(value)?;
        if url.origin() != self.base.origin()
            || !url.path().starts_with(self.base.path())
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            bail!("Microsoft returned an unsafe continuation URL");
        }
        Ok(url)
    }
    fn token(&mut self, account: &AccountConfig) -> Result<Zeroizing<String>> {
        let scope = scope(account);
        if let Some((key, token, expires)) = &self.token
            && key == &scope
            && Instant::now() < *expires
        {
            return Ok(token.clone());
        }
        self.refresh_token(
            account,
            &SecretService,
            Url::parse(&format!(
                "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
                account.tenant
            ))?,
        )
    }
    fn refresh_token(
        &mut self,
        account: &AccountConfig,
        store: &impl CredentialStore,
        endpoint: Url,
    ) -> Result<Zeroizing<String>> {
        let scope = scope(account);
        if account.username.trim().is_empty() {
            bail!(
                "Set the Microsoft username to the signed-in email/UPN to pin this account identity"
            );
        }
        let secret = store.load(&account.credential_ref())?;
        let mut stored: Value =
            serde_json::from_str(&secret).context("Microsoft credential needs device login")?;
        if stored["client_id"] != account.client_id || stored["tenant"] != account.tenant {
            bail!("Microsoft account settings changed; repeat device login");
        }
        let refresh = Zeroizing::new(
            stored["refresh_token"]
                .as_str()
                .context("Repeat Microsoft device login")?
                .to_owned(),
        );
        let response = self
            .client
            .post(endpoint)
            .form(&[
                ("client_id", account.client_id.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.as_str()),
                ("scope", crate::auth::graph_scopes(account.provider)),
            ])
            .send()
            .context("Microsoft token refresh failed")?;
        if !response.status().is_success() {
            bail!("Microsoft session expired or consent missing; repeat account auth");
        }
        let mut value = crate::auth::response_json(response)?;
        let token = Zeroizing::new(
            value["access_token"]
                .as_str()
                .context("Microsoft access token missing")?
                .to_owned(),
        );
        if let Some(refresh) = value["refresh_token"].take().as_str() {
            stored["refresh_token"] = refresh.into();
            let serialized = Zeroizing::new(serde_json::to_string(&stored)?);
            store.store(&account.credential_ref(), &serialized)?;
        }
        let expires = value["expires_in"]
            .as_u64()
            .unwrap_or(300)
            .saturating_sub(60)
            .min(86_400);
        let identity_response = self
            .client
            .get(self.url(&["me"]))
            .bearer_auth(token.as_str())
            .send()
            .context("Could not verify Microsoft account identity")?;
        if !identity_response.status().is_success() {
            bail!("Microsoft profile access requires User.Read consent; repeat account auth");
        }
        let identity = crate::auth::response_json(identity_response)?;
        if !["userPrincipalName", "mail"].iter().any(|field| {
            identity[*field]
                .as_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(&account.username))
        }) {
            bail!(
                "Microsoft login identity differs from configured username; original account data retained"
            );
        }
        self.token = Some((
            scope,
            token.clone(),
            Instant::now() + Duration::from_secs(expires),
        ));
        Ok(token)
    }
    fn request(
        &mut self,
        account: &AccountConfig,
        method: Method,
        url: Url,
        body: Option<Value>,
    ) -> Result<Response> {
        let token = self.token(account)?;
        let mut request = self
            .client
            .request(method, url)
            .bearer_auth(token.as_str())
            .header(
                "Prefer",
                "IdType=\"ImmutableId\", outlook.body-content-type=\"text\", odata.maxpagesize=50",
            );
        if let Some(body) = body {
            request = request.json(&body);
        }
        request
            .send()
            .map_err(|_| anyhow::anyhow!("Microsoft connection interrupted"))
    }
    fn check(&mut self, response: Response) -> Result<Response> {
        let status = response.status();
        if status.as_u16() == 401 {
            self.token = None;
            bail!("Microsoft authentication expired; reconnect or repeat account auth");
        }
        if status.as_u16() == 429 || status.as_u16() == 503 {
            let seconds = response
                .headers()
                .get("Retry-After")
                .and_then(|s| s.to_str().ok())
                .and_then(|s| s.parse().ok())
                .unwrap_or(60);
            return Err(Throttled(seconds).into());
        }
        if status.as_u16() == 403 {
            bail!("Microsoft refused permission; verify delegated scopes and tenant consent");
        }
        if status.as_u16() == 410 {
            bail!("Microsoft sync cursor expired; reset the account cursor explicitly");
        }
        if !status.is_success() {
            bail!("Microsoft request failed (HTTP {})", status.as_u16());
        }
        Ok(response)
    }
    fn get(&mut self, account: &AccountConfig, url: Url) -> Result<Value> {
        let response = self.request(account, Method::GET, url, None)?;
        crate::auth::response_json(self.check(response)?)
    }
    pub fn sync(&mut self, account: &AccountConfig, cursor: Value) -> Result<Batch> {
        match account.provider {
            Provider::Office365 => self.mail(account, cursor),
            Provider::Teams => self.chats(account, cursor),
            _ => bail!("Invalid Microsoft provider"),
        }
    }
    fn mail(&mut self, account: &AccountConfig, mut cursor: Value) -> Result<Batch> {
        if cursor.is_null() {
            cursor = serde_json::json!({});
        }
        let folders = if account.resources.is_empty() {
            vec!["inbox".into()]
        } else {
            account.resources.clone()
        };
        let mut items = Vec::new();
        let mut catching_up = false;
        for folder in folders {
            let old = cursor[&folder].as_str();
            let url = if let Some(url) = old {
                self.continuation(url)?
            } else {
                let mut url = self.url(&["me", "mailFolders", &folder, "messages", "delta"]);
                let since = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
                url.query_pairs_mut().append_pair("$select","id,from,replyTo,subject,body,receivedDateTime,isRead,internetMessageId,hasAttachments").append_pair("$filter",&format!("receivedDateTime ge {since}"));
                url
            };
            let value = match self.get(account, url) {
                Ok(v) => v,
                Err(e) if old.is_some() && e.to_string().contains("cursor expired") => {
                    // Persist a reset only alongside data from the next successful page.
                    let mut reset = cursor.clone();
                    reset
                        .as_object_mut()
                        .expect("cursor object")
                        .remove(&folder);
                    return self.mail(account, reset);
                }
                Err(e) => return Err(e),
            };
            for value in value["value"]
                .as_array()
                .context("Microsoft message list missing")?
            {
                if let Some(item) = mail_item(account, value)? {
                    items.push(item);
                }
            }
            let next = value["@odata.nextLink"].as_str();
            let continuation = next
                .or_else(|| value["@odata.deltaLink"].as_str())
                .context("Microsoft delta cursor missing")?;
            self.continuation(continuation)?;
            cursor[&folder] = continuation.into();
            catching_up |= next.is_some();
        }
        items.sort_by_key(|i| i.timestamp);
        Ok(Batch {
            items,
            cursor,
            status: if catching_up {
                "Catching up · mail delta page saved"
            } else {
                "Connected · mail read state synchronized"
            }
            .into(),
        })
    }
    fn chats(&mut self, account: &AccountConfig, mut cursor: Value) -> Result<Batch> {
        if account.resources.is_empty() {
            bail!("Add Teams chat IDs to account resources; channel sync is not implemented");
        }
        if cursor.is_null() {
            cursor = serde_json::json!({});
        }
        let me = self.get(account, self.url(&["me"]))?;
        let own = me["id"]
            .as_str()
            .context("Microsoft user identity missing")?;
        let mut items = Vec::new();
        let mut catching_up = false;
        for chat in &account.resources {
            let saved = &cursor[chat];
            let mut high = saved["high"].as_str().unwrap_or("").to_owned();
            let url = if let Some(next) = saved["next"].as_str() {
                self.continuation(next)?
            } else {
                let mut url = self.url(&["chats", chat, "messages"]);
                url.query_pairs_mut()
                    .append_pair("$top", "50")
                    .append_pair("$orderby", "lastModifiedDateTime desc");
                if !high.is_empty() {
                    url.query_pairs_mut()
                        .append_pair("$filter", &format!("lastModifiedDateTime gt {high}"));
                }
                url
            };
            let value = self.get(account, url)?;
            for message in value["value"]
                .as_array()
                .context("Microsoft chat messages missing")?
            {
                let modified = message["lastModifiedDateTime"].as_str().unwrap_or("");
                if modified > high.as_str() {
                    high = modified.into();
                }
                if let Some(item) = chat_item(account, chat, own, message)? {
                    items.push(item);
                }
            }
            let next = if saved["high"].as_str().is_some() {
                value["@odata.nextLink"].as_str()
            } else {
                None
            };
            if let Some(next) = next {
                self.continuation(next)?;
            }
            cursor[chat] = serde_json::json!({"high":high,"next":next});
            catching_up |= next.is_some();
        }
        items.sort_by_key(|i| i.timestamp);
        Ok(Batch {
            items,
            cursor,
            status: if catching_up {
                "Catching up · Teams chat page saved · read state local"
            } else {
                "Connected · Teams read state local"
            }
            .into(),
        })
    }
    pub fn unread(
        &mut self,
        account: &AccountConfig,
        remote: &RemoteMessage,
        unread: bool,
    ) -> Result<()> {
        let RemoteMessage::Graph {
            account: id,
            message,
        } = remote
        else {
            bail!("Remote read state unavailable for this conversation");
        };
        if id != &account.id {
            bail!("Account mismatch");
        }
        let response = self.request(
            account,
            Method::PATCH,
            self.url(&["me", "messages", message]),
            Some(serde_json::json!({"isRead":!unread})),
        )?;
        self.check(response)?;
        Ok(())
    }
    pub fn send(
        &mut self,
        account: &AccountConfig,
        item: &Item,
        submission: &Submission,
    ) -> SendResult {
        let (url, body, expected) = match &item.remote {
            RemoteMessage::Graph {
                account: id,
                message,
            } if id == &account.id => (
                self.url(&["me", "messages", message, "reply"]),
                serde_json::json!({"message":{"body":{"contentType":"Text","content":submission.draft.body},"toRecipients":submission.draft.target.recipients.iter().map(|r|serde_json::json!({"emailAddress":{"address":r}})).collect::<Vec<_>>()}}),
                202,
            ),
            RemoteMessage::GraphChat {
                account: id, chat, ..
            } if id == &account.id => (
                self.url(&["chats", chat, "messages"]),
                serde_json::json!({"body":{"contentType":"text","content":submission.draft.body}}),
                201,
            ),
            _ => return SendResult::Failed("Invalid Microsoft reply identity".into()),
        };
        // Resolve token/consent errors before the non-idempotent request begins.
        if self.token(account).is_err() {
            return SendResult::Failed(
                "Microsoft authentication unavailable; repeat account auth".into(),
            );
        }
        match self.request(account, Method::POST, url, Some(body)) {
            Ok(response) if response.status().as_u16() == expected => SendResult::Accepted,
            Ok(response) if response.status().is_client_error() => {
                let _ = self.check(response);
                SendResult::Failed(
                    "Microsoft rejected reply; check account permissions and conversation".into(),
                )
            }
            // An interruption or unexpected server response can follow acceptance.
            _ => SendResult::Uncertain,
        }
    }
}
fn email(value: &Value) -> Option<String> {
    let address = value["emailAddress"]["address"].as_str()?;
    address.parse::<lettre::Address>().ok()?;
    Some(address.into())
}
pub fn mail_item(account: &AccountConfig, value: &Value) -> Result<Option<Item>> {
    let id = value["id"]
        .as_str()
        .context("Microsoft message ID missing")?;
    let deleted = value.get("@removed").is_some();
    let recipients = if let Some(reply) = value["replyTo"].as_array().filter(|v| !v.is_empty()) {
        reply.iter().filter_map(email).collect()
    } else {
        email(&value["from"]).into_iter().collect()
    };
    let mut body = if value["body"]["contentType"]
        .as_str()
        .is_some_and(|t| t.eq_ignore_ascii_case("html"))
    {
        html(value["body"]["content"].as_str().unwrap_or(""))
    } else {
        text(value["body"]["content"].as_str().unwrap_or(""))
    };
    if value["hasAttachments"] == true {
        body.push_str("\n[Attachments available in original mail client]");
    }
    Ok(Some(Item {
        account_scope: scope(account),
        remote: RemoteMessage::Graph {
            account: account.id.clone(),
            message: id.into(),
        },
        sender: value["from"]["emailAddress"]["name"]
            .as_str()
            .filter(|n| !n.is_empty())
            .or_else(|| value["from"]["emailAddress"]["address"].as_str())
            .unwrap_or("Unknown sender")
            .into(),
        recipients,
        subject: value["subject"].as_str().unwrap_or("").into(),
        body,
        timestamp: timestamp(value["receivedDateTime"].as_str().unwrap_or("")),
        unread: value["isRead"].as_bool().map(|r| !r),
        message_id: value["internetMessageId"].as_str().unwrap_or("").into(),
        references: String::new(),
        deleted,
        partial: !deleted && (value.get("body").is_none() || value.get("from").is_none()),
    }))
}
pub fn chat_item(
    account: &AccountConfig,
    chat: &str,
    own: &str,
    value: &Value,
) -> Result<Option<Item>> {
    if value["messageType"].as_str() != Some("message")
        || value["from"]["user"]["id"].as_str() == Some(own)
    {
        return Ok(None);
    }
    let id = value["id"]
        .as_str()
        .context("Microsoft chat message ID missing")?;
    let deleted = value["deletedDateTime"].is_string();
    let user = value["from"]["user"]["id"].as_str().unwrap_or("");
    let mut body = if value["body"]["contentType"].as_str() == Some("html") {
        html(value["body"]["content"].as_str().unwrap_or(""))
    } else {
        text(value["body"]["content"].as_str().unwrap_or(""))
    };
    if value["attachments"]
        .as_array()
        .is_some_and(|a| !a.is_empty())
    {
        body.push_str("\n[Teams files available in original client]");
    }
    Ok(Some(Item {
        account_scope: scope(account),
        remote: RemoteMessage::GraphChat {
            account: account.id.clone(),
            chat: chat.into(),
            message: id.into(),
        },
        sender: value["from"]["user"]["displayName"]
            .as_str()
            .unwrap_or("Teams user")
            .into(),
        recipients: if user.is_empty() {
            vec![]
        } else {
            vec![format!("Teams chat {chat}")]
        },
        subject: value["subject"].as_str().unwrap_or("Teams message").into(),
        body,
        timestamp: timestamp(value["createdDateTime"].as_str().unwrap_or("")),
        unread: None,
        message_id: String::new(),
        references: String::new(),
        deleted,
        partial: false,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
    };
    type Requests = Arc<Mutex<Vec<String>>>;
    fn account(provider: Provider) -> AccountConfig {
        let mut a = AccountConfig::new("graph-test", provider);
        a.username = "owner@example.invalid".into();
        a.client_id = "fixture-client".into();
        a.enabled = true;
        a.allow_send = true;
        a
    }
    fn server(
        responses: Vec<(u16, Value, Option<u64>)>,
    ) -> (Url, Requests, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let base = Url::parse(&format!("http://{address}/v1.0/")).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = requests.clone();
        let server = std::thread::spawn(move || {
            for (status, value, retry) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut headers = String::new();
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    headers.push_str(&line);
                    if line == "\r\n" {
                        break;
                    }
                }
                let length = headers
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|v| v.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                headers.push_str(&String::from_utf8(body).unwrap());
                observed.lock().unwrap().push(headers);
                let body = if status == 202 {
                    String::new()
                } else {
                    value
                        .to_string()
                        .replace("__BASE__", &format!("http://{address}/v1.0/"))
                };
                write!(stream,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",body.len()).unwrap();
                if let Some(retry) = retry {
                    write!(stream, "Retry-After: {retry}\r\n").unwrap();
                }
                write!(stream, "\r\n{body}").unwrap();
            }
        });
        (base, requests, server)
    }
    fn graph(base: Url, a: &AccountConfig) -> Graph {
        Graph {
            client: Client::builder()
                .timeout(Duration::from_secs(2))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap(),
            token: Some((
                scope(a),
                Zeroizing::new("fictional-access-token".into()),
                Instant::now() + Duration::from_secs(60),
            )),
            base,
        }
    }
    #[test]
    fn mail_translation_preserves_reply_to_flags_html_and_immutable_path_segments() {
        let a = account(Provider::Office365);
        let value = serde_json::json!({"id":"immutable/+?","from":{"emailAddress":{"address":"from@example.invalid","name":"Fiction"}},"replyTo":[{"emailAddress":{"address":"reply@example.invalid"}}],"subject":"A subject","receivedDateTime":"2026-10-18T10:00:00Z","isRead":false,"body":{"contentType":"html","content":"<p>Café<script>bad()</script><img src='https://example.invalid/x'>hello</p>"}});
        let item = mail_item(&a, &value).unwrap().unwrap();
        assert_eq!(item.recipients, vec!["reply@example.invalid"]);
        assert_eq!(item.unread, Some(true));
        assert!(!item.body.contains("<p>"));
        assert!(!item.body.contains("bad()"));
        let (base, requests, server) = server(vec![
            (202, Value::Null, None),
            (200, serde_json::json!({"isRead":true}), None),
        ]);
        let mut graph = graph(base, &a);
        let op = Submission {
            operation: 1,
            draft: crate::composer::Draft {
                target: item.message(MessageId(1), &a).target.unwrap(),
                body: "Reply text 東京".into(),
                attachments: vec![],
                revision: 1,
            },
        };
        assert_eq!(graph.send(&a, &item, &op), SendResult::Accepted);
        graph.unread(&a, &item.remote, false).unwrap();
        server.join().unwrap();
        let requests = requests.lock().unwrap();
        assert!(requests[0].starts_with("POST /v1.0/me/messages/immutable%2F+%3F/reply "));
        let body: Value =
            serde_json::from_str(requests[0].split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["message"]["body"]["contentType"], "Text");
        assert_eq!(
            body["message"]["toRecipients"][0]["emailAddress"]["address"],
            "reply@example.invalid"
        );
        assert!(requests[0].contains("ImmutableId"));
        assert!(requests[1].contains("\"isRead\":true"));
    }
    #[test]
    fn opaque_delta_links_are_persisted_and_foreign_origins_are_rejected() {
        let a = account(Provider::Office365);
        let (base, requests, server) = server(vec![(
            200,
            serde_json::json!({"value":[],"@odata.nextLink":"https://untrusted.invalid/steal"}),
            None,
        )]);
        let mut graph = graph(base.clone(), &a);
        assert!(
            graph
                .sync(&a, Value::Null)
                .unwrap_err()
                .to_string()
                .contains("unsafe")
        );
        server.join().unwrap();
        assert_eq!(requests.lock().unwrap().len(), 1);
        for url in [
            "https://graph.microsoft.com.evil.invalid/v1.0/x",
            "https://graph.microsoft.com/v1.0/x",
            "http://user@127.0.0.1/v1.0/x",
        ] {
            assert!(graph.continuation(url).is_err());
        }
        let mut next = base;
        next.set_path("/v1.0/opaque");
        next.set_query(Some("token=a%2Bb%2F%3D"));
        assert_eq!(
            graph.continuation(next.as_str()).unwrap().as_str(),
            next.as_str()
        );
    }
    #[test]
    fn throttling_auth_and_server_failure_do_not_claim_send_acceptance() {
        let a = account(Provider::Office365);
        let responses = vec![
            (429, serde_json::json!({}), Some(90)),
            (401, serde_json::json!({}), None),
        ];
        let (base, _, server) = server(responses);
        let mut graph = graph(base.clone(), &a);
        let error = graph.get(&a, base.clone()).unwrap_err();
        assert_eq!(error.downcast_ref::<Throttled>().unwrap().0, 90);
        assert!(
            graph
                .get(&a, base)
                .unwrap_err()
                .to_string()
                .contains("expired")
        );
        assert!(graph.token.is_none());
        server.join().unwrap();
        let (base, _, server) = self::server(vec![(500, serde_json::json!({}), None)]);
        let mut graph = self::graph(base, &a);
        let item=mail_item(&a,&serde_json::json!({"id":"id","from":{"emailAddress":{"address":"sender@example.invalid"}},"body":{"content":"body"}})).unwrap().unwrap();
        let op = Submission {
            operation: 1,
            draft: crate::composer::Draft {
                target: item.message(MessageId(1), &a).target.unwrap(),
                body: "Fictional reply".into(),
                attachments: vec![],
                revision: 1,
            },
        };
        assert_eq!(graph.send(&a, &item, &op), SendResult::Uncertain);
        server.join().unwrap();
    }
    #[test]
    fn teams_skips_own_and_system_messages_uses_chat_reply_and_keeps_read_local() {
        let mut a = account(Provider::Teams);
        a.resources = vec!["chat/+id".into()];
        let message = serde_json::json!({"id":"message","messageType":"message","from":{"user":{"id":"son","displayName":"Fictional Son"}},"body":{"contentType":"text","content":"Hello!"},"createdDateTime":"2026-10-18T10:00:00Z","lastModifiedDateTime":"2026-10-18T10:00:00Z"});
        assert!(chat_item(&a, "chat", "son", &message).unwrap().is_none());
        let item = chat_item(&a, "chat/+id", "owner", &message)
            .unwrap()
            .unwrap();
        assert!(item.unread.is_none());
        assert!(item.message(MessageId(1), &a).remote.is_none());
        let (base, requests, server) =
            server(vec![(201, serde_json::json!({"id":"accepted"}), None)]);
        let mut graph = graph(base, &a);
        let op = Submission {
            operation: 1,
            draft: crate::composer::Draft {
                target: item.message(MessageId(1), &a).target.unwrap(),
                body: "Hello back".into(),
                attachments: vec![],
                revision: 1,
            },
        };
        assert_eq!(graph.send(&a, &item, &op), SendResult::Accepted);
        server.join().unwrap();
        assert!(requests.lock().unwrap()[0].starts_with("POST /v1.0/chats/chat%2F+id/messages "));
    }
    #[test]
    fn delta_pages_resume_the_exact_opaque_cursor_until_checkpoint() {
        let a = account(Provider::Office365);
        let (base, requests, server) = server(vec![
            (
                200,
                serde_json::json!({"value":[],"@odata.nextLink":"__BASE__opaque?token=a%2Bb%2F%3D"}),
                None,
            ),
            (
                200,
                serde_json::json!({"value":[],"@odata.deltaLink":"__BASE__checkpoint?delta=opaque"}),
                None,
            ),
        ]);
        let mut graph = graph(base, &a);
        let first = graph.sync(&a, Value::Null).unwrap();
        assert!(first.status.contains("Catching up"));
        let second = graph.sync(&a, first.cursor).unwrap();
        assert!(
            second.cursor["inbox"]
                .as_str()
                .unwrap()
                .contains("checkpoint?delta=opaque")
        );
        server.join().unwrap();
        let requests = requests.lock().unwrap();
        assert!(requests[0].contains("receivedDateTime"));
        assert!(requests[1].starts_with("GET /v1.0/opaque?token=a%2Bb%2F%3D "));
    }
    struct Store(Mutex<String>);
    impl CredentialStore for Store {
        fn load(&self, _: &str) -> Result<Zeroizing<String>> {
            Ok(Zeroizing::new(self.0.lock().unwrap().clone()))
        }
        fn store(&self, _: &str, value: &str) -> Result<()> {
            *self.0.lock().unwrap() = value.into();
            Ok(())
        }
        fn remove(&self, _: &str) -> Result<()> {
            Ok(())
        }
    }
    #[test]
    fn refresh_rotates_keyring_credentials_and_verifies_the_configured_identity() {
        let a = account(Provider::Office365);
        let store=Store(Mutex::new(serde_json::json!({"client_id":a.client_id,"tenant":a.tenant,"refresh_token":"fictional-old"}).to_string()));
        let (base, requests, server) = server(vec![
            (
                200,
                serde_json::json!({"access_token":"fictional-access","refresh_token":"fictional-new","expires_in":3600}),
                None,
            ),
            (
                200,
                serde_json::json!({"id":"owner","userPrincipalName":a.username}),
                None,
            ),
        ]);
        let mut graph = graph(base.clone(), &a);
        graph.token = None;
        let token = graph
            .refresh_token(&a, &store, base.join("token").unwrap())
            .unwrap();
        assert_eq!(token.as_str(), "fictional-access");
        assert!(store.0.lock().unwrap().contains("fictional-new"));
        server.join().unwrap();
        let requests = requests.lock().unwrap();
        assert!(requests[0].contains("grant_type=refresh_token"));
        assert!(requests[1].starts_with("GET /v1.0/me "));
    }
}
