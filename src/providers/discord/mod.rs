//! Independent personal-DM transport. Authentication is the QR keyring credential only.
mod gateway;
use super::*;
use crate::discord_auth::protocol::snowflake;
use anyhow::Context;
use reqwest::{
    Method, Url,
    blocking::{Client, Response},
};
use serde_json::json;
use std::{
    collections::{BTreeMap, VecDeque},
    time::{Duration, Instant},
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone)]
struct Channel {
    recipients: Vec<String>,
    name: String,
    kind: u64,
}
impl Channel {
    fn parse(value: &Value, me: &str) -> Result<Self> {
        let kind = value["type"]
            .as_u64()
            .context("Discord channel type missing")?;
        if ![1, 3].contains(&kind) {
            bail!("Discord conversation is not a private DM");
        }
        let mut recipients: Vec<_> = value["recipients"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| r["id"].as_str())
            .chain(
                value["recipient_ids"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str),
            )
            .filter(|id| *id != me && snowflake(id))
            .map(str::to_owned)
            .collect();
        recipients.sort();
        recipients.dedup();
        if recipients.is_empty() || recipients.len() > 10 || kind == 1 && recipients.len() != 1 {
            bail!("Discord private recipients are unavailable");
        }
        Ok(Self {
            recipients,
            name: safe_text(value["name"].as_str().unwrap_or_default()),
            kind,
        })
    }
}
#[derive(Deserialize)]
struct Credential {
    version: u8,
    kind: String,
    user_id: String,
    token: String,
}
#[derive(Debug)]
struct Retry(String);
impl std::fmt::Display for Retry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Retry {}
impl Drop for Credential {
    fn drop(&mut self) {
        self.token.zeroize();
    }
}
fn credential(account: &AccountConfig, store: &impl CredentialStore) -> Result<Zeroizing<String>> {
    if account.discord_mode != "personal" || !snowflake(&account.discord_user_id) {
        bail!("Personal Discord requires QR sign-in and a pinned user ID");
    }
    let stored = store.load(&account.credential_ref()).map_err(|_| {
        anyhow::anyhow!("Discord credential missing or keyring locked · repeat QR sign-in")
    })?;
    let value: Credential = serde_json::from_str(&stored)
        .map_err(|_| anyhow::anyhow!("Discord credential is invalid · repeat QR sign-in"))?;
    if value.version != 1
        || value.kind != "discord_personal"
        || value.user_id != account.discord_user_id
        || value.token.is_empty()
        || value.token.len() > 4096
    {
        bail!("Discord credential identity mismatch · repeat QR sign-in");
    }
    Ok(Zeroizing::new(value.token.clone()))
}
pub struct Discord {
    client: Client,
    base: Url,
    gateway_url: String,
    token: Option<Zeroizing<String>>,
    blocked: Option<[u8; 32]>,
    job: Option<gateway::Job>,
    channels: BTreeMap<String, Channel>,
    pending: VecDeque<gateway::Dispatch>,
    retry: Option<Instant>,
}
impl Discord {
    pub fn new() -> Result<Self> {
        Ok(Self {
            client: crate::auth::http_client(5_000)?,
            base: Url::parse("https://discord.com/api/v9/")?,
            gateway_url: gateway::URL.into(),
            token: None,
            blocked: None,
            job: None,
            channels: BTreeMap::new(),
            pending: VecDeque::new(),
            retry: None,
        })
    }
    fn url(&self, parts: &[&str]) -> Url {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("fixed Discord base")
            .pop_if_empty()
            .extend(parts);
        url
    }
    fn hash(token: &str) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        Sha256::digest(token.as_bytes()).into()
    }
    fn reject_session(&mut self) {
        if let Some(token) = self.token.take() {
            self.blocked = Some(Self::hash(&token));
        }
        self.job = None;
    }
    fn request(&mut self, method: Method, parts: &[&str], body: Option<Value>) -> Result<Response> {
        if self.retry.is_some_and(|until| Instant::now() < until) {
            return Err(Retry("Discord rate limited · waiting before more requests".into()).into());
        }
        let token = self.token.as_ref().context("Discord sign-in required")?;
        let mut authorization = reqwest::header::HeaderValue::from_str(token)
            .map_err(|_| anyhow::anyhow!("Invalid Discord credential"))?;
        authorization.set_sensitive(true);
        let mut request = self
            .client
            .request(method, self.url(parts))
            .header(reqwest::header::AUTHORIZATION, authorization)
            .header(
                reqwest::header::USER_AGENT,
                concat!("d-hub/", env!("CARGO_PKG_VERSION")),
            );
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .map_err(|_| Retry("Discord request interrupted".into()))?;
        if response.status().as_u16() == 401 {
            self.reject_session();
        }
        if response.status().as_u16() == 429 {
            let header = response
                .headers()
                .get("retry-after")
                .and_then(|h| h.to_str().ok())
                .and_then(|v| v.parse::<f64>().ok());
            let body = crate::auth::response_json(response).ok();
            let seconds = header
                .or_else(|| body.as_ref().and_then(|v| v["retry_after"].as_f64()))
                .filter(|v| v.is_finite())
                .unwrap_or(30.0)
                .clamp(1.0, 86_400.0)
                .ceil() as u64;
            self.retry = Some(Instant::now() + Duration::from_secs(seconds));
            return Err(Retry(format!("Discord rate limited · retry after {seconds}s")).into());
        }
        Ok(response)
    }
    fn get(&mut self, parts: &[&str]) -> Result<Value> {
        let response = self.request(Method::GET, parts, None)?;
        if response.status().is_server_error() {
            return Err(Retry("Discord is temporarily unavailable".into()).into());
        }
        if !response.status().is_success() {
            bail!("Discord access rejected · check sign-in and conversation access");
        }
        crate::auth::response_json(response)
            .map_err(|_| anyhow::anyhow!("Discord returned an invalid response"))
    }
    fn initialize(&mut self, account: &AccountConfig, store: &impl CredentialStore) -> Result<()> {
        if self.job.as_ref().is_some_and(|j| j.status().fatal) {
            self.reject_session();
        }
        if self.job.is_some() {
            return Ok(());
        }
        let token = credential(account, store)?;
        if self.blocked == Some(Self::hash(&token)) {
            bail!("Discord session rejected · repeat QR sign-in");
        }
        self.token = Some(token.clone());
        let identity = self.get(&["users", "@me"])?;
        if identity["id"].as_str() != Some(account.discord_user_id.as_str()) {
            self.reject_session();
            bail!("Discord account identity mismatch · original account retained");
        }
        self.job = Some(gateway::Job::start(
            account.discord_user_id.clone(),
            token,
            self.gateway_url.clone(),
        ));
        Ok(())
    }
    fn channel(&mut self, id: &str, account: &AccountConfig, fresh: bool) -> Result<Channel> {
        if !snowflake(id) {
            bail!("Invalid Discord conversation ID");
        }
        if !fresh && let Some(channel) = self.channels.get(id) {
            return Ok(channel.clone());
        }
        let value = self.get(&["channels", id])?;
        if value["id"].as_str() != Some(id) {
            bail!("Discord returned another conversation");
        }
        let channel = Channel::parse(&value, &account.discord_user_id)?;
        if self.channels.len() >= 4096 {
            self.channels.clear();
        }
        self.channels.insert(id.into(), channel.clone());
        Ok(channel)
    }
    fn item(&mut self, account: &AccountConfig, event: &gateway::Dispatch) -> Result<Option<Item>> {
        let data = &event.data;
        if event.kind == "READY" {
            self.channels.clear();
            for value in data["private_channels"].as_array().into_iter().flatten() {
                if let Some(id) = value["id"].as_str().filter(|id| snowflake(id))
                    && let Ok(channel) = Channel::parse(value, &account.discord_user_id)
                {
                    self.channels.insert(id.into(), channel);
                }
            }
            return Ok(None);
        }
        if event.kind.starts_with("CHANNEL_") {
            if let Some(id) = data["id"].as_str() {
                self.channels.remove(id);
                if event.kind != "CHANNEL_DELETE"
                    && let Ok(channel) = Channel::parse(data, &account.discord_user_id)
                {
                    self.channels.insert(id.into(), channel);
                }
            }
            return Ok(None);
        }
        let id = data["id"]
            .as_str()
            .filter(|id| snowflake(id))
            .context("Invalid Discord message ID")?;
        let conversation = data["channel_id"]
            .as_str()
            .filter(|id| snowflake(id))
            .context("Invalid Discord conversation ID")?;
        let remote = RemoteMessage::DiscordDm {
            account: account.id.clone(),
            channel: conversation.into(),
            message: id.into(),
        };
        if event.kind == "MESSAGE_DELETE" {
            return Ok(Some(Item {
                account_scope: scope(account),
                remote,
                sender: String::new(),
                recipients: vec![],
                subject: "Deleted Discord message".into(),
                body: String::new(),
                timestamp: 0,
                unread: None,
                message_id: id.into(),
                references: String::new(),
                deleted: true,
                partial: true,
            }));
        }
        let full;
        let value = if event.kind == "MESSAGE_UPDATE" {
            full = self.get(&["channels", conversation, "messages", id])?;
            if full["id"].as_str() != Some(id) || full["channel_id"].as_str() != Some(conversation)
            {
                bail!("Discord edit identity mismatch");
            }
            &full
        } else {
            data
        };
        let author = value["author"]["id"]
            .as_str()
            .filter(|id| snowflake(id))
            .context("Invalid Discord author")?;
        if author == account.discord_user_id {
            return Ok(None);
        }
        if ![0, 19].contains(&value["type"].as_u64().unwrap_or(0)) {
            return Ok(None);
        }
        let mut channel = self.channel(conversation, account, false)?;
        if channel.kind == 3 {
            // Membership changes need not carry CHANNEL_UPDATE. Capture the current group at receipt.
            channel = self.channel(conversation, account, true)?;
        }
        if !channel.recipients.iter().any(|id| id == author) {
            bail!("Discord sender differs from private conversation");
        }
        let sender = value["author"]["global_name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .or_else(|| value["author"]["username"].as_str())
            .unwrap_or(author);
        let mut body = text(value["content"].as_str().unwrap_or_default());
        for attachment in value["attachments"]
            .as_array()
            .into_iter()
            .flatten()
            .take(10)
        {
            body.push_str(&format!(
                "\n[Attachment: {} · download pending]",
                text(attachment["filename"].as_str().unwrap_or("file"))
            ));
        }
        if body.is_empty() {
            body = "[Discord message has no text; rich content available in Discord]".into();
        }
        Ok(Some(Item {
            account_scope: scope(account),
            remote,
            sender: safe_text(sender),
            recipients: channel.recipients,
            subject: if channel.kind == 3 {
                format!("Group DM · {}", channel.name)
            } else {
                "Direct message".into()
            },
            body: text(&body),
            timestamp: 0,
            unread: None,
            message_id: id.into(),
            references: String::new(),
            deleted: false,
            partial: event.kind == "MESSAGE_UPDATE",
        }))
    }
    pub fn sync(&mut self, account: &AccountConfig, cursor: Value) -> Result<Batch> {
        self.sync_with(account, cursor, &SecretService)
    }
    fn sync_with(
        &mut self,
        account: &AccountConfig,
        cursor: Value,
        store: &impl CredentialStore,
    ) -> Result<Batch> {
        if self.job.is_none() {
            self.initialize(account, store)?;
        }
        let job = self.job.as_ref().expect("initialized gateway");
        let mut stopped = false;
        while self.pending.len() < 64 {
            match job.events.try_recv() {
                Ok(event) => self.pending.push_back(event),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    stopped = true;
                    break;
                }
                Err(_) => break,
            }
        }
        let connection = job.status();
        let mut status = connection.text.clone();
        let mut items = Vec::new();
        while let Some(event) = self.pending.pop_front() {
            match self.item(account, &event) {
                Ok(Some(item)) => items.push(item),
                Ok(None) => {}
                Err(error) => {
                    status = safe_text(&error.to_string());
                    if error.downcast_ref::<Retry>().is_some() {
                        self.pending.push_front(event);
                        break;
                    }
                }
            }
        }
        if connection.fatal && items.is_empty() && self.pending.is_empty() {
            self.reject_session();
            bail!("{}", connection.text);
        }
        if stopped && !connection.fatal {
            self.job = None;
            status = "Discord gateway stopped · reconnecting".into();
        }
        Ok(Batch {
            items,
            cursor,
            status,
        })
    }
    pub fn send(&mut self, account: &AccountConfig, item: &Item, op: &Submission) -> SendResult {
        if !account.enabled
            || !account.allow_send
            || item.account_scope != scope(account)
            || item.deleted
            || !op.draft.attachments.is_empty()
            || item
                .message(op.draft.target.incoming, account)
                .target
                .as_ref()
                != Some(&op.draft.target)
        {
            return SendResult::Failed(
                "Discord reply identity changed or sending is disabled".into(),
            );
        }
        if op.draft.body.trim().is_empty() || op.draft.body.encode_utf16().count() > 2000 {
            return SendResult::Failed(
                "Discord text replies must contain 1–2000 characters".into(),
            );
        }
        let RemoteMessage::DiscordDm {
            account: id,
            channel,
            message,
        } = &item.remote
        else {
            return SendResult::Failed("Invalid Discord reply target".into());
        };
        if id != &account.id || !snowflake(channel) || !snowflake(message) {
            return SendResult::Failed("Invalid Discord reply identity".into());
        }
        if self.initialize(account, &SecretService).is_err() {
            return SendResult::Failed(
                "Discord authentication unavailable · repeat QR sign-in".into(),
            );
        }
        let current = match self.channel(channel, account, true) {
            Ok(c) => c,
            Err(e) => return SendResult::Failed(safe_text(&e.to_string())),
        };
        if current.recipients != op.draft.target.recipients {
            return SendResult::Failed(
                "Discord conversation recipients changed · original draft retained".into(),
            );
        }
        // The daemon commits the outbox first. Never retry this POST automatically.
        use sha2::{Digest, Sha256};
        let digest = format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(scope(account), &item.remote, op))
                    .expect("submission serializes")
            )
        );
        let body = json!({"content":op.draft.body,"nonce":format!("dh{}", &digest[..22]),"enforce_nonce":true,
            "message_reference":{"message_id":message,"channel_id":channel,"fail_if_not_exists":true},"allowed_mentions":{"parse":[],"replied_user":false}});
        match self.request(Method::POST, &["channels", channel, "messages"], Some(body)) {
            Ok(response) if response.status().is_success() => {
                match crate::auth::response_json(response) {
                    Ok(value)
                        if value["channel_id"].as_str() == Some(channel)
                            && value["author"]["id"].as_str()
                                == Some(account.discord_user_id.as_str())
                            && value["id"].as_str().is_some_and(snowflake) =>
                    {
                        SendResult::Accepted
                    }
                    _ => SendResult::Uncertain,
                }
            }
            Ok(response) if response.status().is_client_error() => SendResult::Failed(
                "Discord rejected reply · check access, verification or sign-in".into(),
            ),
            // Errors before the request (rate-limit gate) are definitely not submitted.
            Err(_) if self.retry.is_some_and(|until| Instant::now() < until) => {
                SendResult::Failed("Discord rate limited · reply retained; retry later".into())
            }
            _ => SendResult::Uncertain,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        thread::{self, JoinHandle},
    };
    const ME: &str = "100000000000000001";
    const FRIEND: &str = "100000000000000002";
    const CHANNEL: &str = "100000000000000003";
    const MESSAGE: &str = "100000000000000004";
    const TOKEN: &str = "fictional-token-never-log";
    fn account() -> AccountConfig {
        let mut a = AccountConfig::new("fictional-discord", Provider::Discord);
        a.discord_user_id = ME.into();
        a.enabled = true;
        a.allow_send = true;
        a
    }
    struct Store(String);
    impl CredentialStore for Store {
        fn load(&self, reference: &str) -> Result<Zeroizing<String>> {
            assert_eq!(reference, "discord-personal-fictional-discord");
            Ok(Zeroizing::new(self.0.clone()))
        }
        fn store(&self, _: &str, _: &str) -> Result<()> {
            panic!("transport cannot write credentials")
        }
        fn remove(&self, _: &str) -> Result<()> {
            panic!("transport cannot remove credentials")
        }
    }
    fn store() -> Store {
        Store(json!({"version":1,"kind":"discord_personal","user_id":ME,"token":TOKEN}).to_string())
    }
    fn channel() -> Value {
        json!({"id":CHANNEL,"type":1,"recipients":[{"id":FRIEND}]})
    }
    fn message() -> Value {
        json!({"id":MESSAGE,"channel_id":CHANNEL,"author":{"id":FRIEND,"username":"Fictional friend"},"type":0,"content":"Hello 東京\u{1b}[31m"})
    }
    fn server(responses: Vec<Option<(u16, Value)>>) -> (Url, JoinHandle<Vec<(String, Value)>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = Url::parse(&format!(
            "http://{}/api/v9/",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        listener.set_nonblocking(true).unwrap();
        let handle = thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let deadline = Instant::now() + Duration::from_secs(5);
                let mut stream = loop {
                    match listener.accept() {
                        Ok((s, _)) => break s,
                        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            assert!(Instant::now() < deadline, "fake HTTP request timed out");
                            thread::sleep(Duration::from_millis(5));
                        }
                        Err(e) => panic!("{e}"),
                    }
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut bytes = Vec::new();
                while !bytes.ends_with(b"\r\n\r\n") {
                    let mut byte = [0];
                    stream.read_exact(&mut byte).unwrap();
                    bytes.push(byte[0]);
                    assert!(bytes.len() < 16_384);
                }
                let headers = String::from_utf8(bytes).unwrap();
                assert!(
                    headers
                        .to_lowercase()
                        .contains(&format!("authorization: {TOKEN}"))
                );
                let length = headers
                    .lines()
                    .find_map(|l| {
                        l.to_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                let mut body = vec![0; length];
                stream.read_exact(&mut body).unwrap();
                requests.push((
                    headers.lines().next().unwrap().into(),
                    if body.is_empty() {
                        Value::Null
                    } else {
                        serde_json::from_slice(&body).unwrap()
                    },
                ));
                if let Some((code, body)) = response {
                    let body = body.to_string();
                    write!(stream, "HTTP/1.1 {code} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                }
            }
            requests
        });
        (base, handle)
    }
    fn local(base: Url) -> Discord {
        let mut discord = Discord::new().unwrap();
        discord.client = Client::builder()
            .timeout(Duration::from_secs(2))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        discord.base = base;
        discord
    }
    fn ready(url: &str) -> Value {
        json!({"op":0,"s":1,"t":"READY","d":{"user":{"id":ME},"session_id":"fictional-gateway-session","resume_gateway_url":url,"private_channels":[channel()]}})
    }
    fn ws_send(socket: &mut tungstenite::WebSocket<TcpStream>, value: Value) {
        socket
            .send(tungstenite::Message::Text(value.to_string().into()))
            .unwrap();
    }
    fn ws_read(socket: &mut tungstenite::WebSocket<TcpStream>) -> Value {
        loop {
            let frame = socket.read().unwrap();
            if let tungstenite::Message::Text(body) = frame {
                let value: Value = serde_json::from_str(&body).unwrap();
                if value["op"] == 1 {
                    ws_send(socket, json!({"op":11}));
                } else {
                    return value;
                }
            }
        }
    }
    fn ws_accept(listener: &TcpListener) -> tungstenite::WebSocket<TcpStream> {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline, "fake gateway timed out");
                    thread::sleep(Duration::from_millis(5));
                }
                Err(e) => panic!("{e}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        tungstenite::accept(stream).unwrap()
    }
    fn drain(discord: &mut Discord, a: &AccountConfig, count: usize) -> Vec<Item> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut items = Vec::new();
        while items.len() < count {
            let batch = discord.sync_with(a, Value::Null, &store()).unwrap();
            items.extend(batch.items);
            assert!(
                Instant::now() < deadline,
                "gateway delivery timed out: {}",
                batch.status
            );
            thread::sleep(Duration::from_millis(5));
        }
        items
    }
    #[test]
    fn gateway_receives_only_incoming_dms_and_resumes_without_session_or_token_in_cursor() {
        let (base, http) = server(vec![Some((200, json!({"id":ME})))]);
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://{}/?v=9&encoding=json", listener.local_addr().unwrap());
        let copy = url.clone();
        let ws = thread::spawn(move || {
            let mut socket = ws_accept(&listener);
            ws_send(
                &mut socket,
                json!({"op":10,"d":{"heartbeat_interval":1000}}),
            );
            let identify = ws_read(&mut socket);
            assert_eq!(identify["op"], 2);
            assert_eq!(identify["d"]["token"], TOKEN);
            assert!(identify["d"].get("intents").is_none());
            ws_send(&mut socket, ready(&copy));
            let mut own = message();
            own["author"]["id"] = ME.into();
            ws_send(
                &mut socket,
                json!({"op":0,"s":2,"t":"MESSAGE_CREATE","d":own}),
            );
            let mut guild = message();
            guild["guild_id"] = "999".into();
            ws_send(
                &mut socket,
                json!({"op":0,"s":3,"t":"MESSAGE_CREATE","d":guild}),
            );
            ws_send(
                &mut socket,
                json!({"op":0,"s":4,"t":"MESSAGE_CREATE","d":message()}),
            );
            ws_send(&mut socket, json!({"op":7}));
            drop(socket);
            let mut socket = ws_accept(&listener);
            ws_send(
                &mut socket,
                json!({"op":10,"d":{"heartbeat_interval":1000}}),
            );
            let resume = ws_read(&mut socket);
            assert_eq!(resume["op"], 6);
            assert_eq!(resume["d"]["seq"], 4);
            ws_send(
                &mut socket,
                json!({"op":0,"s":5,"t":"MESSAGE_CREATE","d":message()}),
            );
            ws_send(&mut socket, json!({"op":0,"s":6,"t":"RESUMED","d":{}}));
            while let Ok(tungstenite::Message::Text(body)) = socket.read() {
                if serde_json::from_str::<Value>(&body).unwrap()["op"] == 1 {
                    ws_send(&mut socket, json!({"op":11}));
                }
            }
        });
        let mut discord = local(base);
        discord.gateway_url = url;
        let a = account();
        let items = drain(&mut discord, &a, 2);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].remote, items[1].remote);
        assert_eq!(items[0].body, "Hello 東京");
        let message = items[0].message(MessageId(42), &a);
        assert!(
            message.remote.is_none(),
            "Discord seen state is local in this slice"
        );
        assert_eq!(message.target.unwrap().conversation, CHANNEL);
        let batch = discord.sync_with(&a, Value::Null, &store()).unwrap();
        assert_eq!(batch.cursor, Value::Null);
        assert!(!serde_json::to_string(&items).unwrap().contains(TOKEN));
        drop(discord);
        ws.join().unwrap();
        http.join().unwrap();
    }
    #[test]
    fn credential_and_http_identity_failures_never_start_a_gateway_or_echo_secrets() {
        let mut a = account();
        a.discord_user_id = FRIEND.into();
        let error = credential(&a, &store()).unwrap_err().to_string();
        assert!(!error.contains(TOKEN));
        assert!(error.contains("identity mismatch"));
        let (base, http) = server(vec![Some((200, json!({"id":FRIEND,"secret":TOKEN})))]);
        let mut discord = local(base);
        let error = discord
            .sync_with(&account(), Value::Null, &store())
            .unwrap_err()
            .to_string();
        assert!(error.contains("identity mismatch"));
        assert!(!error.contains(TOKEN));
        assert!(discord.job.is_none());
        assert!(discord.blocked.is_some());
        assert!(
            discord
                .sync_with(&account(), Value::Null, &store())
                .is_err()
        );
        assert_eq!(http.join().unwrap().len(), 1);
    }
    fn outgoing() -> (AccountConfig, Item, Submission) {
        let a = account();
        let item = Item {
            account_scope: scope(&a),
            remote: RemoteMessage::DiscordDm {
                account: a.id.clone(),
                channel: CHANNEL.into(),
                message: MESSAGE.into(),
            },
            sender: "Friend".into(),
            recipients: vec![FRIEND.into()],
            subject: "DM".into(),
            body: "Incoming".into(),
            timestamp: 12,
            unread: None,
            message_id: MESSAGE.into(),
            references: String::new(),
            deleted: false,
            partial: false,
        };
        let mut composer = crate::composer::Composer::default();
        composer.apply(crate::composer::Event::Select(
            item.message(MessageId(1), &a).target,
        ));
        composer.apply(crate::composer::Event::Body("Reply 東京".into()));
        composer.apply(crate::composer::Event::Submit);
        let crate::composer::SendState::Submitting(op) = composer.send else {
            panic!("submission")
        };
        (a, item, op)
    }
    fn sender(base: Url) -> Discord {
        let mut discord = local(base);
        discord.token = Some(Zeroizing::new(TOKEN.into()));
        discord.job = Some(gateway::Job::fixture());
        discord
    }
    #[test]
    fn group_arrival_refreshes_membership_before_capturing_recipients() {
        let newcomer = "100000000000000009";
        let mut current = channel();
        current["type"] = 3.into();
        current["recipients"] = json!([{"id":FRIEND},{"id":newcomer}]);
        let (base, http) = server(vec![Some((200, current))]);
        let mut discord = sender(base);
        let mut cached = channel();
        cached["type"] = 3.into();
        discord
            .channels
            .insert(CHANNEL.into(), Channel::parse(&cached, ME).unwrap());
        let mut value = message();
        value["author"]["id"] = newcomer.into();
        let event = gateway::Dispatch {
            kind: "MESSAGE_CREATE".into(),
            data: value,
        };
        let item = discord.item(&account(), &event).unwrap().unwrap();
        assert_eq!(
            item.recipients,
            vec![FRIEND.to_owned(), newcomer.to_owned()]
        );
        assert_eq!(http.join().unwrap().len(), 1);
    }
    #[test]
    fn edit_fetches_one_message_and_a_missing_edit_does_not_block_the_next_dm() {
        let mut edited = message();
        edited["content"] = "Edited 東京".into();
        let (base, http) = server(vec![
            Some((200, edited)),
            Some((404, json!({"message":"gone"}))),
        ]);
        let mut discord = sender(base);
        discord
            .channels
            .insert(CHANNEL.into(), Channel::parse(&channel(), ME).unwrap());
        let edit = gateway::Dispatch {
            kind: "MESSAGE_UPDATE".into(),
            data: json!({"id":MESSAGE,"channel_id":CHANNEL}),
        };
        let item = discord.item(&account(), &edit).unwrap().unwrap();
        assert!(item.partial);
        assert_eq!(item.body, "Edited 東京");
        let deletion = discord
            .item(
                &account(),
                &gateway::Dispatch {
                    kind: "MESSAGE_DELETE".into(),
                    data: edit.data.clone(),
                },
            )
            .unwrap()
            .unwrap();
        assert!(deletion.partial && deletion.deleted);
        discord.pending.push_back(edit);
        discord.pending.push_back(gateway::Dispatch {
            kind: "MESSAGE_CREATE".into(),
            data: message(),
        });
        let batch = discord
            .sync_with(&account(), Value::Null, &store())
            .unwrap();
        assert_eq!(batch.items.len(), 1);
        assert_eq!(batch.items[0].body, "Hello 東京");
        assert!(discord.pending.is_empty());
        assert_eq!(http.join().unwrap().len(), 2);
    }
    #[test]
    fn reply_posts_only_to_captured_dm_after_rechecking_private_recipients() {
        let (base, http) = server(vec![
            Some((200, channel())),
            Some((
                200,
                json!({"id":"100000000000000005","channel_id":CHANNEL,"author":{"id":ME}}),
            )),
        ]);
        let mut discord = sender(base);
        let (a, item, op) = outgoing();
        assert_eq!(discord.send(&a, &item, &op), SendResult::Accepted);
        let requests = http.join().unwrap();
        assert!(
            requests[0]
                .0
                .starts_with(&format!("GET /api/v9/channels/{CHANNEL}"))
        );
        assert!(
            requests[1]
                .0
                .starts_with(&format!("POST /api/v9/channels/{CHANNEL}/messages "))
        );
        let body = &requests[1].1;
        assert_eq!(body["content"], "Reply 東京");
        assert_eq!(body["message_reference"]["message_id"], MESSAGE);
        assert_eq!(
            body["allowed_mentions"],
            json!({"parse":[],"replied_user":false})
        );
        assert_eq!(body["enforce_nonce"], true);
        assert!(body["nonce"].as_str().unwrap().len() <= 25);
    }
    #[test]
    fn reply_handles_changed_recipients_rejection_throttling_and_uncertain_acceptance() {
        let (a, item, op) = outgoing();
        let mut other = channel();
        other["recipients"][0]["id"] = "100000000000000009".into();
        let (base, http) = server(vec![Some((200, other))]);
        assert!(matches!(
            sender(base).send(&a, &item, &op),
            SendResult::Failed(_)
        ));
        assert_eq!(http.join().unwrap().len(), 1);
        for (response, expected) in [
            (
                Some((403, json!({"message":TOKEN}))),
                SendResult::Failed(String::new()),
            ),
            (
                Some((429, json!({"retry_after":10}))),
                SendResult::Failed(String::new()),
            ),
            (Some((500, json!({"message":TOKEN}))), SendResult::Uncertain),
            (None, SendResult::Uncertain),
        ] {
            let (base, http) = server(vec![Some((200, channel())), response]);
            let result = sender(base).send(&a, &item, &op);
            assert_eq!(
                std::mem::discriminant(&result),
                std::mem::discriminant(&expected)
            );
            assert!(!format!("{result:?}").contains(TOKEN));
            assert_eq!(http.join().unwrap().len(), 2);
        }
    }
}
