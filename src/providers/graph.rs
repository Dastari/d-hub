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
    /// Scopes Microsoft granted at the last refresh, for setup checks.
    granted: Option<String>,
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
/// The saved delta cursor is no longer valid; that folder restarts from its initial query.
#[derive(Debug)]
pub struct CursorExpired;
impl std::fmt::Display for CursorExpired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Microsoft sync cursor expired; restarting that folder")
    }
}
impl std::error::Error for CursorExpired {}
impl Graph {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: crate::auth::http_client(20_000)?,
            token: None,
            base: Url::parse("https://graph.microsoft.com/v1.0/")?,
            granted: None,
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
            bail!(
                "Microsoft session expired or consent missing{}; repeat account auth",
                crate::auth::microsoft_detail(response)
            );
        }
        let mut value = crate::auth::response_json(response)?;
        self.granted = value["scope"].as_str().map(|scope| {
            scope
                .split_whitespace()
                .map(|s| s.trim_start_matches("https://graph.microsoft.com/"))
                .collect::<Vec<_>>()
                .join(" ")
        });
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
        // Keep the transport error (without its URL) so callers can tell "never connected" apart.
        request.send().map_err(|e| {
            anyhow::Error::new(e.without_url()).context("Microsoft connection interrupted")
        })
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
        if status.is_success() {
            return Ok(response);
        }
        if status.as_u16() == 410 {
            return Err(CursorExpired.into());
        }
        let value = crate::auth::response_json(response).unwrap_or_default();
        let detail = crate::auth::microsoft_error(&value);
        // Outlook can report an expired delta token as 400 with a sync-state code instead of 410.
        if status.as_u16() == 400
            && value["error"]["code"].as_str().is_some_and(|code| {
                let code = code.to_ascii_lowercase();
                code.contains("syncstate") || code.contains("resync")
            })
        {
            return Err(CursorExpired.into());
        }
        let detail = detail.map_or_else(String::new, |d| format!(" · {d}"));
        if status.as_u16() == 403 {
            bail!(
                "Microsoft refused permission{detail}; verify delegated scopes and tenant consent"
            );
        }
        bail!(
            "Microsoft request failed (HTTP {}{detail})",
            status.as_u16()
        )
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
            let url = match old {
                Some(url) => self.continuation(url)?,
                None => self.initial_mail_url(&folder),
            };
            let value = match self.get(account, url) {
                Ok(v) => v,
                // Restart only this folder; earlier folders keep this pass's items and cursors,
                // and the reset is saved only with the restarted folder's first page.
                Err(e) if old.is_some() && e.is::<CursorExpired>() => {
                    self.get(account, self.initial_mail_url(&folder))?
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
    /// First delta query for a folder: the last week, with the fields the feed uses.
    fn initial_mail_url(&self, folder: &str) -> Url {
        let mut url = self.url(&["me", "mailFolders", folder, "messages", "delta"]);
        // Whole seconds with Z: Graph's OData date parser rejects nanosecond fractions.
        let since = (chrono::Utc::now() - chrono::Duration::days(7))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        url.query_pairs_mut()
            .append_pair(
                "$select",
                "id,from,replyTo,toRecipients,ccRecipients,subject,body,receivedDateTime,isRead,internetMessageId,hasAttachments,inferenceClassification",
            )
            .append_pair("$filter", &format!("receivedDateTime ge {since}"));
        url
    }
    fn chat_messages_url(&self, chat: &str, high: &str) -> Url {
        let mut url = self.url(&["chats", chat, "messages"]);
        url.query_pairs_mut()
            .append_pair("$top", "50")
            .append_pair("$orderby", "lastModifiedDateTime desc");
        if !high.is_empty() {
            url.query_pairs_mut()
                .append_pair("$filter", &format!("lastModifiedDateTime gt {high}"));
        }
        url
    }
    /// Read-only setup check: sign in, then the folders or chats this account would sync.
    pub fn probe(&mut self, account: &AccountConfig) -> Vec<check::Step> {
        use check::Step;
        let mut steps = vec![];
        if let Err(e) = self.token(account) {
            steps.push(Step::failed("Sign in", e));
            return steps;
        }
        let mut signed_in = format!("Signed in as {}", account.username);
        if let Some(granted) = &self.granted {
            signed_in.push_str(&format!(" · granted {granted}"));
        }
        steps.push(Step::ok("Sign in", signed_in));
        if account.provider == Provider::Teams {
            self.probe_chats(account, &mut steps);
        } else {
            self.probe_mail(account, &mut steps);
        }
        steps
    }
    fn probe_mail(&mut self, account: &AccountConfig, steps: &mut Vec<check::Step>) {
        use check::Step;
        let mut url = self.url(&["me", "mailFolders"]);
        url.query_pairs_mut()
            .append_pair("$select", "id,displayName,unreadItemCount")
            .append_pair("$top", "100");
        steps.push(match self.get(account, url) {
            Ok(value) => {
                let folders: Vec<_> = value["value"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|f| {
                        // Custom folders are synced by ID; Graph does not accept display names.
                        format!(
                            "{} ({} unread) · {}",
                            f["displayName"].as_str().unwrap_or("?"),
                            f["unreadItemCount"].as_u64().unwrap_or(0),
                            f["id"].as_str().unwrap_or("?")
                        )
                    })
                    .collect();
                Step::ok("Folders", format!("{} folders", folders.len())).with_items(folders)
            }
            Err(e) => Step::failed("Folders", e),
        });
        let folders = if account.resources.is_empty() {
            vec!["inbox".to_owned()]
        } else {
            account.resources.clone()
        };
        for folder in &folders {
            let mut url = self.url(&["me", "mailFolders", folder]);
            url.query_pairs_mut()
                .append_pair("$select", "displayName,totalItemCount");
            let name = format!("Folder {folder}");
            steps.push(match self.get(account, url) {
                Ok(value) => Step::ok(
                    name,
                    format!(
                        "{} · {} messages",
                        value["displayName"].as_str().unwrap_or(folder),
                        value["totalItemCount"].as_u64().unwrap_or(0)
                    ),
                ),
                Err(e) => Step::failed(name, e),
            });
        }
        // The exact first query sync sends, so a rejected filter shows up here.
        let url = self.initial_mail_url(&folders[0]);
        steps.push(match self.get(account, url) {
            Ok(value) => Step::ok(
                "Mail sync query",
                format!(
                    "accepted · {} messages from the last week on the first page",
                    value["value"].as_array().map_or(0, Vec::len)
                ),
            ),
            Err(e) => Step::failed("Mail sync query", e),
        });
    }
    fn probe_chats(&mut self, account: &AccountConfig, steps: &mut Vec<check::Step>) {
        use check::Step;
        let mut url = self.url(&["me", "chats"]);
        url.query_pairs_mut()
            .append_pair("$select", "id,topic,chatType")
            .append_pair("$expand", "members")
            .append_pair("$top", "50");
        steps.push(match self.get(account, url) {
            Ok(value) => {
                let chats: Vec<_> = value["value"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|chat| {
                        let others: Vec<_> = chat["members"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter(|m| {
                                !m["email"]
                                    .as_str()
                                    .is_some_and(|e| e.eq_ignore_ascii_case(&account.username))
                            })
                            .filter_map(|m| m["displayName"].as_str())
                            .collect();
                        let label = chat["topic"]
                            .as_str()
                            .filter(|t| !t.is_empty())
                            .map_or_else(|| others.join(", "), str::to_owned);
                        format!(
                            "{label} · {} · {}",
                            chat["chatType"].as_str().unwrap_or("chat"),
                            chat["id"].as_str().unwrap_or("?")
                        )
                    })
                    .collect();
                Step::ok(
                    "Chats",
                    format!(
                        "{} recent chats · add IDs to resources to sync them",
                        chats.len()
                    ),
                )
                .with_items(chats)
            }
            Err(e) => Step::failed("Chats", e),
        });
        if account.resources.is_empty() {
            steps.push(Step::failed(
                "Synced chats",
                "None selected; add chat IDs from the list to resources",
            ));
        }
        // The exact query sync sends, including its filter.
        let since = (chrono::Utc::now() - chrono::Duration::days(1))
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        for chat in &account.resources {
            let name = format!("Chat {chat}");
            steps.push(
                match self.get(account, self.chat_messages_url(chat, &since)) {
                    Ok(value) => Step::ok(
                        name,
                        format!(
                            "readable · {} messages changed in the last day",
                            value["value"].as_array().map_or(0, Vec::len)
                        ),
                    ),
                    Err(e) => Step::failed(name, e),
                },
            );
        }
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
            let next = saved["next"].as_str();
            // Messages created before this pass began were already offered; a reaction or
            // edit changes their lastModifiedDateTime, which must update them in place
            // rather than import them as new. The first sync offers only the last week.
            let since = match (next, saved["since"].as_str()) {
                (Some(_), Some(since)) => since.to_owned(),
                _ => high.clone(),
            };
            let baseline = Some(since.as_str())
                .filter(|s| !s.is_empty())
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                .map(|t| t.to_utc())
                .unwrap_or_else(|| chrono::Utc::now() - chrono::Duration::days(7));
            let url = match next {
                Some(next) => self.continuation(next)?,
                None => self.chat_messages_url(chat, &high),
            };
            let value = self.get(account, url)?;
            for message in value["value"]
                .as_array()
                .context("Microsoft chat messages missing")?
            {
                let modified = message["lastModifiedDateTime"].as_str().unwrap_or("");
                if later(modified, &high) {
                    high = modified.into();
                }
                let created = message["createdDateTime"]
                    .as_str()
                    .and_then(|t| chrono::DateTime::parse_from_rfc3339(t).ok());
                let existing = created.is_none_or(|t| t.to_utc() <= baseline);
                if let Some(mut item) = chat_item(account, chat, own, message)? {
                    item.partial = existing;
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
            cursor[chat] = serde_json::json!({"high":high,"next":next,"since":since});
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
            } if id == &account.id => {
                if self.token(account).is_err() {
                    return SendResult::Failed(
                        "Microsoft authentication unavailable; repeat account auth".into(),
                    );
                }
                return match self.mail_reply(account, message, submission) {
                    Ok((url, body)) => self.post(account, url, body, 202),
                    Err(e) => SendResult::Failed(format!(
                        "Microsoft could not prepare the reply; not sent ({})",
                        safe_text(&e.to_string())
                    )),
                };
            }
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
        self.post(account, url, Some(body), expected)
    }
    /// Outlook's own reply draft: the text goes above the quoted original, as in Outlook,
    /// and only the draft's recipients are replaced. Returns the draft's send request.
    /// Nothing is sent here; a failure removes the draft.
    fn mail_reply(
        &mut self,
        account: &AccountConfig,
        message: &str,
        submission: &Submission,
    ) -> Result<(Url, Option<Value>)> {
        let url = self.url(&["me", "messages", message, "createReply"]);
        let response = self.request(account, Method::POST, url, Some(serde_json::json!({})))?;
        let draft = crate::auth::response_json(self.check(response)?)?;
        let id = draft["id"]
            .as_str()
            .context("Microsoft reply draft ID missing")?
            .to_owned();
        let prepared = (|| {
            let original = draft["body"]["content"].as_str().unwrap_or("");
            let html = draft["body"]["contentType"]
                .as_str()
                .is_some_and(|t| t.eq_ignore_ascii_case("html"));
            let content = if html {
                quote_above_html(&submission.draft.body, original)
            } else {
                format!("{}\r\n\r\n{original}", submission.draft.body)
            };
            let body = serde_json::json!({
                "body": {"contentType": if html { "HTML" } else { "Text" }, "content": content},
                "toRecipients": submission.draft.target.recipients.iter()
                    .map(|r| serde_json::json!({"emailAddress":{"address":r}}))
                    .collect::<Vec<_>>(),
            });
            let response = self.request(
                account,
                Method::PATCH,
                self.url(&["me", "messages", &id]),
                Some(body),
            )?;
            self.check(response).map(drop)
        })();
        if let Err(e) = prepared {
            self.discard(account, &id);
            return Err(e);
        }
        Ok((self.url(&["me", "messages", &id, "send"]), None))
    }
    fn discard(&mut self, account: &AccountConfig, draft: &str) {
        let url = self.url(&["me", "messages", draft]);
        let _ = self.request(account, Method::DELETE, url, None);
    }
    fn post(
        &mut self,
        account: &AccountConfig,
        url: Url,
        body: Option<Value>,
        expected: u16,
    ) -> SendResult {
        match self.request(account, Method::POST, url, body) {
            Ok(response) if response.status().as_u16() == expected => SendResult::Accepted,
            // Never connected (DNS, refused, TLS handshake): nothing reached Microsoft.
            Err(e)
                if e.downcast_ref::<reqwest::Error>()
                    .is_some_and(reqwest::Error::is_connect) =>
            {
                SendResult::Failed("Could not reach Microsoft; reply not sent".into())
            }
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
/// Puts the reply first inside the draft's HTML body, before Outlook's quoted original.
fn quote_above_html(reply: &str, original: &str) -> String {
    let escaped = reply
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace("\r\n", "\n")
        .replace('\n', "<br>");
    let block = format!("<div>{escaped}</div><br>");
    let lower = original.to_ascii_lowercase();
    match lower
        .find("<body")
        .and_then(|start| lower[start..].find('>').map(|end| start + end + 1))
    {
        Some(at) => format!("{}{block}{}", &original[..at], &original[at..]),
        None => format!("{block}{original}"),
    }
}
/// Compares Graph timestamps as instants; their fractional digits vary, so text order is wrong.
fn later(candidate: &str, than: &str) -> bool {
    let parse = |t: &str| chrono::DateTime::parse_from_rfc3339(t).ok();
    match (parse(candidate), parse(than)) {
        (Some(a), Some(b)) => a > b,
        (Some(_), None) => true,
        _ => false,
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
    let listed = |field: &str| -> Vec<String> {
        value[field]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(email)
            .collect()
    };
    let (to, cc) = (listed("toRecipients"), listed("ccRecipients"));
    let signals = crate::domain::Signals {
        automated: false,
        bulk: value["inferenceClassification"].as_str() == Some("other"),
        addressed: if value.get("toRecipients").is_none() {
            crate::domain::Addressed::Unknown
        } else {
            crate::domain::addressed(&account.username, &to, &cc)
        },
        audience: value
            .get("toRecipients")
            .map(|_| (to.len() + cc.len()) as u32),
    };
    Ok(Some(Item {
        account_scope: scope(account),
        signals,
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
    body.truncate(body.trim_end().len());
    for attachment in value["attachments"].as_array().into_iter().flatten() {
        let name = safe_text(attachment["name"].as_str().unwrap_or(""));
        match attachment["contentType"].as_str().unwrap_or("") {
            // A quoted or forwarded message, not a file; Teams supplies a short preview.
            kind @ ("messageReference" | "forwardedMessageReference") => {
                let quoted: Value = attachment["content"]
                    .as_str()
                    .and_then(|c| serde_json::from_str(c).ok())
                    .unwrap_or_default();
                let sender = quoted["messageSender"]["user"]["displayName"]
                    .as_str()
                    .unwrap_or("Teams user");
                let preview = quoted["messagePreview"].as_str().unwrap_or("");
                let label = if kind == "messageReference" {
                    sender.to_owned()
                } else {
                    format!("Forwarded from {sender}")
                };
                let quote = text(&format!("{label}: {preview}"))
                    .lines()
                    .map(|line| format!("> {line}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                body = if body.trim().is_empty() {
                    quote
                } else {
                    format!("{quote}\n{body}")
                };
            }
            "reference" => body.push_str(&format!(
                "\n[File: {} · open in Teams to download]",
                if name.is_empty() {
                    "unnamed file"
                } else {
                    &name
                }
            )),
            kind if kind.starts_with("application/vnd.microsoft.card.") => {
                body.push_str("\n[Card · open in Teams to view]")
            }
            kind if kind.starts_with("image/") => {
                body.push_str("\n[Image · open in Teams to view]")
            }
            _ => body.push_str("\n[Attachment · open in Teams to view]"),
        }
    }
    // One-to-one chat IDs end in @unq.gbl.spaces and group/meeting chats in @thread.v2; the
    // configured chat ID is all a message carries, so this is evidence, not a guarantee.
    let signals = crate::domain::Signals {
        automated: user.is_empty() && value["from"]["application"].is_object(),
        bulk: false,
        addressed: if chat.ends_with("@unq.gbl.spaces") {
            crate::domain::Addressed::Direct
        } else if chat.contains("@thread.") {
            crate::domain::Addressed::Group
        } else {
            crate::domain::Addressed::Unknown
        },
        audience: None,
    };
    Ok(Some(Item {
        account_scope: scope(account),
        signals,
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
                let body = if matches!(status, 202 | 204) {
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
            granted: None,
        }
    }
    #[test]
    fn mail_and_chat_items_record_addressing_focused_inbox_and_app_senders() {
        use crate::domain::Addressed;
        let a = account(Provider::Office365);
        let mail = serde_json::json!({"id":"m","from":{"emailAddress":{"address":"news@example.invalid","name":"News"}},
            "toRecipients":[{"emailAddress":{"address":"list@example.invalid"}}],"ccRecipients":[],
            "inferenceClassification":"other","subject":"Weekly","receivedDateTime":"2026-10-18T10:00:00Z","body":{"contentType":"text","content":"Hi"}});
        let signals = mail_item(&a, &mail).unwrap().unwrap().signals;
        assert_eq!(
            (signals.addressed, signals.audience, signals.bulk),
            (Addressed::NotListed, Some(1), true)
        );
        let mut focused = mail.clone();
        focused["toRecipients"] =
            serde_json::json!([{"emailAddress":{"address":"OWNER@example.invalid"}}]);
        focused["inferenceClassification"] = "focused".into();
        let signals = mail_item(&a, &focused).unwrap().unwrap().signals;
        assert_eq!((signals.addressed, signals.bulk), (Addressed::To, false));
        // A delta page without recipient fields (an older cursor) knows nothing either way.
        let mut old = mail.clone();
        old.as_object_mut().unwrap().remove("toRecipients");
        assert_eq!(
            mail_item(&a, &old).unwrap().unwrap().signals.addressed,
            Addressed::Unknown
        );

        let teams = account(Provider::Teams);
        let message = serde_json::json!({"id":"1","messageType":"message","createdDateTime":"2026-10-18T10:00:00Z",
            "from":{"user":{"id":"son","displayName":"Son"}},"body":{"contentType":"text","content":"hi"}});
        let one = chat_item(&teams, "19:a_b@unq.gbl.spaces", "owner", &message)
            .unwrap()
            .unwrap();
        assert_eq!(one.signals.addressed, Addressed::Direct);
        let group = chat_item(&teams, "19:abc@thread.v2", "owner", &message)
            .unwrap()
            .unwrap();
        assert_eq!(group.signals.addressed, Addressed::Group);
        let mut app_message = message.clone();
        app_message["from"] =
            serde_json::json!({"user":null,"application":{"id":"bot","displayName":"Workflow"}});
        let item = chat_item(&teams, "19:abc@thread.v2", "owner", &app_message)
            .unwrap()
            .unwrap();
        assert!(item.signals.automated);
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
            (
                201,
                serde_json::json!({"id":"draft/1","body":{"contentType":"text","content":"From: Fiction\r\n\r\nCafé hello"}}),
                None,
            ),
            (200, serde_json::json!({"id":"draft/1"}), None),
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
        // Outlook's reply draft keeps the quoted original under the reply text.
        assert!(requests[0].starts_with("POST /v1.0/me/messages/immutable%2F+%3F/createReply "));
        assert!(requests[0].contains("ImmutableId"));
        assert!(requests[1].starts_with("PATCH /v1.0/me/messages/draft%2F1 "));
        let body: Value =
            serde_json::from_str(requests[1].split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["body"]["contentType"], "Text");
        assert_eq!(
            body["body"]["content"],
            "Reply text 東京\r\n\r\nFrom: Fiction\r\n\r\nCafé hello"
        );
        assert_eq!(
            body["toRecipients"][0]["emailAddress"]["address"],
            "reply@example.invalid"
        );
        assert_eq!(body["toRecipients"].as_array().unwrap().len(), 1);
        assert!(requests[2].starts_with("POST /v1.0/me/messages/draft%2F1/send "));
        assert!(requests[3].contains("\"isRead\":true"));
    }
    #[test]
    fn a_reply_draft_that_cannot_be_prepared_is_removed_and_never_sent() {
        let a = account(Provider::Office365);
        let item = mail_item(&a, &serde_json::json!({"id":"original","from":{"emailAddress":{"address":"sender@example.invalid"}},"body":{"content":"body"}})).unwrap().unwrap();
        let op = Submission {
            operation: 1,
            draft: crate::composer::Draft {
                target: item.message(MessageId(1), &a).target.unwrap(),
                body: "Fictional reply".into(),
                attachments: vec![],
                revision: 1,
            },
        };
        let (base, requests, server) = server(vec![
            (
                201,
                serde_json::json!({"id":"draft","body":{"contentType":"html","content":"<html><body><p>Original</p></body></html>"}}),
                None,
            ),
            (
                400,
                serde_json::json!({"error":{"code":"ErrorInvalidRecipients","message":"Fictional refusal"}}),
                None,
            ),
            (204, Value::Null, None),
        ]);
        let result = graph(base, &a).send(&a, &item, &op);
        server.join().unwrap();
        assert!(
            matches!(&result, SendResult::Failed(reason) if reason.contains("not sent") && reason.contains("ErrorInvalidRecipients")),
            "{result:?}"
        );
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert!(requests[2].starts_with("DELETE /v1.0/me/messages/draft "));
        assert!(requests.iter().all(|r| !r.contains("/send ")));
        // The original's own refusal stops before any draft exists.
        let (base, requests, server) = self::server(vec![(403, serde_json::json!({}), None)]);
        let result = self::graph(base, &a).send(&a, &item, &op);
        server.join().unwrap();
        assert!(matches!(result, SendResult::Failed(_)));
        assert_eq!(requests.lock().unwrap().len(), 1);
    }
    #[test]
    fn html_reply_text_is_escaped_and_placed_above_the_quoted_original() {
        assert_eq!(
            quote_above_html(
                "a <b> & \"c\"\nnext",
                "<html><BODY class=x><p>Original</p></BODY></html>"
            ),
            "<html><BODY class=x><div>a &lt;b&gt; &amp; &quot;c&quot;<br>next</div><br><p>Original</p></BODY></html>"
        );
        assert_eq!(
            quote_above_html("hi", "<p>Original</p>"),
            "<div>hi</div><br><p>Original</p>"
        );
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
        // A failure while preparing the draft sends nothing; one on the send itself may follow acceptance.
        let (base, _, server) = self::server(vec![
            (
                201,
                serde_json::json!({"id":"draft","body":{"contentType":"text","content":""}}),
                None,
            ),
            (200, serde_json::json!({}), None),
            (500, serde_json::json!({}), None),
        ]);
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
    fn teams_reactions_and_edits_to_earlier_messages_are_updates_not_arrivals() {
        let mut a = account(Provider::Teams);
        a.resources = vec!["chat".into()];
        let at = |days: i64, minutes: i64| {
            (chrono::Utc::now() - chrono::Duration::days(days) + chrono::Duration::minutes(minutes))
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        };
        let message = |id: &str, created: &str, modified: &str| serde_json::json!({"id":id,"messageType":"message","from":{"user":{"id":"friend","displayName":"Fictional Friend"}},"body":{"contentType":"text","content":id},"createdDateTime":created,"lastModifiedDateTime":modified});
        let (recent, old_reacted) = (at(0, -10), at(30, 0));
        let first_high = at(0, -5);
        let (base, requests, server) = server(vec![
            (200, serde_json::json!({"id":"owner"}), None),
            (
                200,
                serde_json::json!({"value":[
                    message("recent", &recent, &first_high),
                    message("month-old", &old_reacted, &at(0, -6)),
                ]}),
                None,
            ),
            (200, serde_json::json!({"id":"owner"}), None),
            (
                200,
                serde_json::json!({"value":[
                    message("new", &at(0, -1), &at(0, -1)),
                    message("recent", &recent, &at(0, -2)),
                ]}),
                None,
            ),
        ]);
        let mut graph = graph(base, &a);
        let first = graph.sync(&a, Value::Null).unwrap();
        let partial = |batch: &Batch, id: &str| {
            batch
                .items
                .iter()
                .find(|i| matches!(&i.remote, RemoteMessage::GraphChat { message, .. } if message == id))
                .unwrap()
                .partial
        };
        // The first sync offers the last week; a month-old message someone reacted to is not new.
        assert!(!partial(&first, "recent"));
        assert!(partial(&first, "month-old"));
        assert_eq!(first.cursor["chat"]["high"], first_high);
        let second = graph.sync(&a, first.cursor).unwrap();
        server.join().unwrap();
        assert!(!partial(&second, "new"));
        assert!(
            partial(&second, "recent"),
            "an edit or reaction updates in place"
        );
        let requests = requests.lock().unwrap();
        assert!(
            requests[3].contains("lastModifiedDateTime+gt+"),
            "{}",
            requests[3]
        );
    }
    #[test]
    fn teams_quotes_files_and_cards_are_described_separately() {
        let a = account(Provider::Teams);
        let quote = serde_json::json!({"messageId":"1","messagePreview":"Are we still on for Friday?","messageSender":{"user":{"displayName":"Fictional Boss"}}}).to_string();
        let value = serde_json::json!({"id":"m","messageType":"message","from":{"user":{"id":"friend","displayName":"Fictional Friend"}},"body":{"contentType":"html","content":"<attachment id=\"1\"></attachment><p>Yes, 3pm</p>"},"createdDateTime":"2026-10-18T10:00:00Z","attachments":[{"id":"1","contentType":"messageReference","content":quote}]});
        let item = chat_item(&a, "chat", "owner", &value).unwrap().unwrap();
        assert_eq!(
            item.body,
            "> Fictional Boss: Are we still on for Friday?\nYes, 3pm"
        );
        assert!(!item.body.contains("file"), "a quoted reply has no files");
        let mut value = value;
        value["attachments"] = serde_json::json!([
            {"id":"f","contentType":"reference","name":"Budget.xlsx","contentUrl":"https://example.invalid/f"},
            {"id":"c","contentType":"application/vnd.microsoft.card.adaptive","content":"{}"},
        ]);
        let item = chat_item(&a, "chat", "owner", &value).unwrap().unwrap();
        assert_eq!(
            item.body,
            "Yes, 3pm\n[File: Budget.xlsx · open in Teams to download]\n[Card · open in Teams to view]"
        );
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
    fn mail_value(id: &str) -> Value {
        serde_json::json!({"id":id,"from":{"emailAddress":{"address":"sender@example.invalid"}},"subject":id,"receivedDateTime":"2026-10-18T10:00:00Z","isRead":false,"body":{"contentType":"text","content":"Fictional"}})
    }
    #[test]
    fn an_expired_folder_cursor_restarts_only_that_folder_and_keeps_earlier_items() {
        let mut a = account(Provider::Office365);
        a.resources = vec!["inbox".into(), "archive".into()];
        for expired in [
            (410, serde_json::json!({})),
            // Outlook's alternative: 400 with a sync-state error code.
            (
                400,
                serde_json::json!({"error":{"code":"SyncStateNotFound","message":"Sync state not found"}}),
            ),
        ] {
            let (base, requests, server) = server(vec![
                (
                    200,
                    serde_json::json!({"value":[mail_value("kept")],"@odata.deltaLink":"__BASE__inbox-next"}),
                    None,
                ),
                (expired.0, expired.1, None),
                (
                    200,
                    serde_json::json!({"value":[mail_value("restarted")],"@odata.deltaLink":"__BASE__archive-new"}),
                    None,
                ),
            ]);
            let cursor = serde_json::json!({
                "inbox": format!("{base}inbox-old"),
                "archive": format!("{base}archive-old"),
            });
            let mut graph = graph(base.clone(), &a);
            let batch = graph.sync(&a, cursor).unwrap();
            server.join().unwrap();
            let subjects: Vec<_> = batch.items.iter().map(|i| i.subject.as_str()).collect();
            assert_eq!(subjects.len(), 2, "{subjects:?}");
            assert!(subjects.contains(&"kept") && subjects.contains(&"restarted"));
            assert_eq!(batch.cursor["inbox"], format!("{base}inbox-next"));
            assert_eq!(batch.cursor["archive"], format!("{base}archive-new"));
            // The restart is a fresh query whose date has whole seconds and a Z suffix.
            let restart = requests.lock().unwrap()[2]
                .lines()
                .next()
                .unwrap()
                .to_owned();
            assert!(restart.starts_with("GET /v1.0/me/mailFolders/archive/messages/delta?"));
            let date = restart
                .split("receivedDateTime+ge+")
                .nth(1)
                .and_then(|rest| rest.split(' ').next())
                .unwrap();
            assert_eq!(date.len(), "2026-10-11T03%3A03%3A00Z".len(), "{date}");
            assert!(date.ends_with('Z') && !date.contains('.'), "{date}");
        }
    }
    #[test]
    fn microsoft_explanations_reach_the_status_without_controls_or_trace_ids() {
        let token_error = serde_json::json!({"error":"invalid_client","error_description":"AADSTS700016: Application with identifier 'x' was not found.\r\nTrace ID: 123\r\nCorrelation ID: 456"});
        assert_eq!(
            crate::auth::microsoft_error(&token_error).unwrap(),
            "AADSTS700016: Application with identifier 'x' was not found."
        );
        let graph_error = serde_json::json!({"error":{"code":"ErrorAccessDenied","message":"Access is denied.\u{1b}[31m"}});
        let detail = crate::auth::microsoft_error(&graph_error).unwrap();
        assert!(detail.starts_with("ErrorAccessDenied: Access is denied."));
        assert!(!detail.contains('\u{1b}'));
        let long = serde_json::json!({"error":{"code":"X","message":"y".repeat(500)}});
        assert!(crate::auth::microsoft_error(&long).unwrap().chars().count() <= 201);
        assert!(crate::auth::microsoft_error(&serde_json::json!({})).is_none());

        let (base, _, server) = server(vec![(403, graph_error, None)]);
        let mut graph = graph(base.clone(), &a_teams());
        let error = graph.get(&a_teams(), base).unwrap_err().to_string();
        server.join().unwrap();
        assert!(error.contains("ErrorAccessDenied"), "{error}");
        assert!(error.contains("tenant consent"), "{error}");
    }
    fn a_teams() -> AccountConfig {
        account(Provider::Teams)
    }
    #[test]
    fn a_send_that_never_connects_fails_instead_of_becoming_uncertain() {
        let a = account(Provider::Office365);
        let closed = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = Url::parse(&format!("http://{}/v1.0/", closed.local_addr().unwrap())).unwrap();
        drop(closed);
        let mut graph = graph(base, &a);
        let item = mail_item(&a, &mail_value("never")).unwrap().unwrap();
        let op = Submission {
            operation: 1,
            draft: crate::composer::Draft {
                target: item.message(MessageId(1), &a).target.unwrap(),
                body: "Fictional reply".into(),
                attachments: vec![],
                revision: 1,
            },
        };
        assert!(matches!(graph.send(&a, &item, &op), SendResult::Failed(_)));
    }
    #[test]
    fn mail_probe_reports_folders_the_synced_folder_and_the_real_sync_query() {
        let a = account(Provider::Office365);
        let (base, requests, server) = server(vec![
            (
                200,
                serde_json::json!({"value":[{"id":"1","displayName":"Inbox","unreadItemCount":3},{"id":"2","displayName":"Archive","unreadItemCount":0}]}),
                None,
            ),
            (
                200,
                serde_json::json!({"displayName":"Inbox","totalItemCount":120}),
                None,
            ),
            (
                200,
                serde_json::json!({"value":[mail_value("a")],"@odata.deltaLink":"__BASE__d"}),
                None,
            ),
        ]);
        let steps = graph(base, &a).probe(&a);
        server.join().unwrap();
        assert!(steps.iter().all(|s| s.ok), "{steps:?}");
        assert_eq!(
            steps[1].items,
            vec!["Inbox (3 unread) · 1", "Archive (0 unread) · 2"]
        );
        assert_eq!(steps[2].detail, "Inbox · 120 messages");
        assert!(steps[3].detail.contains("1 messages"));
        let requests = requests.lock().unwrap();
        assert!(requests.iter().all(|r| r.starts_with("GET ")), "read-only");
        assert!(requests[2].contains("/messages/delta?"));
    }
    #[test]
    fn teams_probe_lists_chats_by_people_and_flags_missing_or_refused_chats() {
        let mut a = account(Provider::Teams);
        let chats = serde_json::json!({"value":[
            {"id":"19:abc@unq.gbl.spaces","chatType":"oneOnOne","topic":null,"members":[
                {"displayName":"Owner","email":"OWNER@example.invalid"},
                {"displayName":"Jules","email":"jules@example.invalid"}]},
            {"id":"19:team@thread.v2","chatType":"group","topic":"Launch","members":[]}
        ]});
        let (base, _, server) = server(vec![(200, chats.clone(), None)]);
        let steps = graph(base, &a).probe(&a);
        server.join().unwrap();
        assert_eq!(
            steps[1].items,
            vec![
                "Jules · oneOnOne · 19:abc@unq.gbl.spaces",
                "Launch · group · 19:team@thread.v2"
            ]
        );
        assert!(!steps[2].ok && steps[2].name == "Synced chats");

        a.resources = vec!["19:team@thread.v2".into()];
        let refused =
            serde_json::json!({"error":{"code":"Forbidden","message":"Missing scope permissions"}});
        let (base, requests, server) = self::server(vec![(200, chats, None), (403, refused, None)]);
        let steps = graph(base, &a).probe(&a);
        server.join().unwrap();
        let chat = steps.last().unwrap();
        assert!(
            !chat.ok && chat.detail.contains("Missing scope permissions"),
            "{chat:?}"
        );
        let query = requests.lock().unwrap()[1]
            .lines()
            .next()
            .unwrap()
            .to_owned();
        assert!(
            query.contains("/chats/19:team@thread.v2/messages?"),
            "{query}"
        );
        assert!(query.contains("lastModifiedDateTime+gt+"), "{query}");
    }
}
