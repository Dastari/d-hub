//! One background gateway per daemon-owned account. No credentials/session IDs leave this job.
use super::*;
use std::{
    io::ErrorKind,
    net::{TcpStream, ToSocketAddrs},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
};
use tungstenite::{
    Message as Frame, WebSocket, client::IntoClientRequest, protocol::WebSocketConfig,
    stream::MaybeTlsStream,
};

pub(super) const URL: &str = "wss://gateway.discord.gg/?v=9&encoding=json";
const LIMIT: usize = 16 * 1024 * 1024;
pub(super) struct Dispatch {
    pub kind: String,
    pub data: Value,
}
#[derive(Clone)]
pub(super) struct Status {
    pub text: String,
    pub fatal: bool,
}
pub(super) struct Job {
    pub events: Receiver<Dispatch>,
    status: Arc<Mutex<Status>>,
    stop: Arc<AtomicBool>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}
impl Job {
    pub fn status(&self) -> Status {
        self.status.lock().expect("gateway status").clone()
    }
    pub fn start(user: String, token: Zeroizing<String>, url: String) -> Self {
        let (tx, events) = mpsc::sync_channel(64);
        let status = Arc::new(Mutex::new(Status {
            text: "Discord connecting…".into(),
            fatal: false,
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let copy = status.clone();
        let cancelled = stop.clone();
        std::thread::spawn(move || {
            let mut session = Session::default();
            let mut delay = Duration::from_secs(1);
            while !cancelled.load(Ordering::Acquire) {
                let result = connected(&user, &token, &url, &mut session, &tx, &copy, &cancelled);
                if cancelled.load(Ordering::Acquire) || matches!(result, Err(Failure::Stopped)) {
                    break;
                }
                let (text, fatal) = match result {
                    Err(Failure::Fatal(message)) => (message, true),
                    _ => (
                        "Discord disconnected · reconnecting; history/offline catch-up pending"
                            .into(),
                        false,
                    ),
                };
                *copy.lock().expect("gateway status") = Status { text, fatal };
                if fatal {
                    break;
                }
                let until = Instant::now() + delay;
                while Instant::now() < until && !cancelled.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(50));
                }
                delay = if session.established {
                    Duration::from_secs(1)
                } else {
                    (delay * 2).min(Duration::from_secs(60))
                };
            }
        });
        Self {
            events,
            status,
            stop,
        }
    }
}
#[derive(Default)]
struct Session {
    id: Option<Zeroizing<String>>,
    url: Option<String>,
    sequence: Option<u64>,
    established: bool,
}
enum Failure {
    Retry,
    Fatal(String),
    Stopped,
}
type Socket = WebSocket<MaybeTlsStream<TcpStream>>;
fn connect(url: &str) -> Result<Socket> {
    let endpoint = reqwest::Url::parse(url)?;
    let host = endpoint.host_str().context("Missing gateway host")?;
    let port = endpoint
        .port_or_known_default()
        .context("Missing gateway port")?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut tcp = None;
    for address in (host, port)
        .to_socket_addrs()
        .context("Discord lookup failed")?
        .take(4)
    {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        if let Ok(stream) =
            TcpStream::connect_timeout(&address, remaining.min(Duration::from_secs(3)))
        {
            tcp = Some(stream);
            break;
        }
    }
    let stream = tcp.context("Discord connection failed")?;
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut request = url.into_client_request()?;
    request
        .headers_mut()
        .insert("Origin", "https://discord.com".parse()?);
    request.headers_mut().insert(
        "User-Agent",
        concat!("d-hub/", env!("CARGO_PKG_VERSION")).parse()?,
    );
    let config = WebSocketConfig::default()
        .write_buffer_size(0)
        .max_write_buffer_size(65_536)
        .max_message_size(Some(LIMIT))
        .max_frame_size(Some(LIMIT));
    let (socket, _) = tungstenite::client_tls_with_config(request, stream, Some(config), None)
        .map_err(|_| anyhow::anyhow!("Discord TLS/WebSocket connection failed"))?;
    let stream = match socket.get_ref() {
        MaybeTlsStream::NativeTls(stream) => stream.get_ref(),
        #[cfg(test)]
        MaybeTlsStream::Plain(stream) => stream,
        _ => bail!("Discord requires validated TLS"),
    };
    stream.set_read_timeout(Some(Duration::from_millis(100)))?;
    Ok(socket)
}
fn send(socket: &mut Socket, value: &Value) -> std::result::Result<(), Failure> {
    let body = Zeroizing::new(value.to_string());
    socket
        .send(Frame::Text(body.as_str().into()))
        .map_err(|_| Failure::Retry)
}
fn resume_url(value: &str, initial: &str) -> Option<String> {
    let mut url = reqwest::Url::parse(value).ok()?;
    let valid = url.scheme() == "wss"
        && url
            .host_str()
            .is_some_and(|h| h == "gateway.discord.gg" || h.ends_with(".discord.gg"))
        && url.port_or_known_default() == Some(443)
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none()
        && url.path() == "/";
    #[cfg(test)]
    let valid = valid || value == initial;
    #[cfg(not(test))]
    let _ = initial;
    if !valid {
        return None;
    }
    url.set_query(Some("v=9&encoding=json"));
    Some(url.to_string())
}
fn enqueue(tx: &SyncSender<Dispatch>, kind: &str, data: Value) -> std::result::Result<(), Failure> {
    tx.try_send(Dispatch {
        kind: kind.into(),
        data,
    })
    .map_err(|e| match e {
        mpsc::TrySendError::Full(_) => Failure::Retry, // Resume from the last enqueued sequence; never silently drop an arrival.
        mpsc::TrySendError::Disconnected(_) => Failure::Stopped,
    })
}
fn connected(
    user: &str,
    token: &str,
    initial: &str,
    session: &mut Session,
    tx: &SyncSender<Dispatch>,
    status: &Mutex<Status>,
    stop: &AtomicBool,
) -> std::result::Result<(), Failure> {
    let mut socket =
        connect(session.url.as_deref().unwrap_or(initial)).map_err(|_| Failure::Retry)?;
    let mut interval = None;
    let mut beat = Instant::now();
    let mut awaiting_ack = false;
    let handshake_deadline = Instant::now() + Duration::from_secs(15);
    let mut authenticated = false;
    let resuming = session.id.is_some() && session.sequence.is_some();
    loop {
        if stop.load(Ordering::Acquire) {
            let _ = socket.close(None);
            return Err(Failure::Stopped);
        }
        if !authenticated && Instant::now() >= handshake_deadline {
            return Err(Failure::Retry);
        }
        if let Some(period) = interval
            && Instant::now() >= beat
        {
            if awaiting_ack {
                return Err(Failure::Retry);
            }
            send(&mut socket, &json!({"op":1,"d":session.sequence}))?;
            awaiting_ack = true;
            beat = Instant::now() + period;
        }
        let value = match socket.read() {
            Ok(Frame::Text(text)) => {
                serde_json::from_str::<Value>(&text).map_err(|_| Failure::Retry)?
            }
            Ok(Frame::Ping(_)) | Ok(Frame::Pong(_)) => {
                socket.flush().map_err(|_| Failure::Retry)?;
                continue;
            }
            Ok(Frame::Close(close)) => {
                match close.map(|c| u16::from(c.code)) {
                    Some(4004 | 4010..=4016) => return Err(Failure::Fatal(
                        "Discord session rejected · repeat QR sign-in; no messages were accepted"
                            .into(),
                    )),
                    Some(1000 | 1001 | 4003 | 4007 | 4009) => {
                        *session = Session::default();
                    }
                    _ => {}
                }
                return Err(Failure::Retry);
            }
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                continue;
            }
            _ => return Err(Failure::Retry),
        };
        match value["op"].as_u64() {
            Some(10) if interval.is_none() => {
                let ms = value["d"]["heartbeat_interval"]
                    .as_u64()
                    .filter(|v| (50..=120_000).contains(v))
                    .ok_or(Failure::Retry)?;
                let period = Duration::from_millis(ms);
                interval = Some(period);
                let mut random = [0u8; 1];
                openssl::rand::rand_bytes(&mut random).map_err(|_| Failure::Retry)?;
                beat = Instant::now() + period.mul_f64(f64::from(random[0]) / 256.0);
                let payload = if resuming {
                    json!({"op":6,"d":{"token":token,"session_id":session.id.as_deref().map(|v| v.as_str()),"seq":session.sequence}})
                } else {
                    json!({"op":2,"d":{"token":token,"capabilities":0,"properties":{"os":"linux","browser":"d-hub","device":"d-hub"},"compress":false}})
                };
                send(&mut socket, &payload)?;
            }
            Some(11) => awaiting_ack = false,
            Some(1) if interval.is_some() => {
                send(&mut socket, &json!({"op":1,"d":session.sequence}))?;
                // Requested beats do not extend the regular acknowledgement deadline.
            }
            Some(7) => return Err(Failure::Retry),
            Some(9) => {
                if value["d"] != true {
                    *session = Session::default();
                }
                return Err(Failure::Retry);
            }
            Some(0) if interval.is_some() => {
                let sequence = value["s"].as_u64().ok_or(Failure::Retry)?;
                let kind = value["t"].as_str().ok_or(Failure::Retry)?;
                if session.sequence.is_some_and(|old| sequence <= old) {
                    continue;
                }
                let data = &value["d"];
                if kind == "READY" {
                    if data["user"]["id"].as_str() != Some(user) {
                        return Err(Failure::Fatal("Discord gateway identity mismatch · account retained; repeat QR sign-in".into()));
                    }
                    let id = data["session_id"]
                        .as_str()
                        .filter(|s| !s.is_empty() && s.len() <= 256)
                        .ok_or(Failure::Retry)?;
                    let url = resume_url(
                        data["resume_gateway_url"].as_str().unwrap_or(initial),
                        initial,
                    )
                    .ok_or(Failure::Retry)?;
                    // Guilds, relationships and unrelated profile data are never queued or stored.
                    let channels = data["private_channels"]
                        .as_array()
                        .map(|rows| {
                            rows.iter()
                                .take(4096)
                                .map(channel_fields)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    enqueue(tx, "READY", json!({"private_channels":channels}))?;
                    session.id = Some(Zeroizing::new(id.into()));
                    session.url = Some(url);
                    session.established = true;
                    authenticated = true;
                } else if kind == "RESUMED" && resuming {
                    authenticated = true;
                    session.established = true;
                } else if session.established && (authenticated || resuming) {
                    match kind {
                        "CHANNEL_CREATE" | "CHANNEL_UPDATE" | "CHANNEL_DELETE"
                            if data["guild_id"].is_null() =>
                        {
                            enqueue(tx, kind, channel_fields(data))?
                        }
                        "MESSAGE_CREATE" | "MESSAGE_UPDATE" | "MESSAGE_DELETE"
                            if data["guild_id"].is_null() =>
                        {
                            enqueue(tx, kind, message_fields(data))?
                        }
                        _ => {}
                    }
                }
                session.sequence = Some(sequence);
                if authenticated {
                    *status.lock().expect("gateway status") = Status { text: "Discord DMs connected · read dots local · history/offline catch-up pending".into(), fatal: false };
                }
            }
            _ => {}
        }
    }
}
fn channel_fields(value: &Value) -> Value {
    let recipients: Vec<_> = value["recipients"]
        .as_array()
        .into_iter()
        .flatten()
        .take(11)
        .map(|v| json!({"id":v["id"]}))
        .collect();
    let ids: Vec<_> = value["recipient_ids"]
        .as_array()
        .into_iter()
        .flatten()
        .take(11)
        .cloned()
        .collect();
    json!({"id":value["id"],"type":value["type"],"recipients":recipients,"recipient_ids":ids,"name":text(value["name"].as_str().unwrap_or_default())})
}
fn message_fields(value: &Value) -> Value {
    let author = &value["author"];
    let attachments: Vec<_> = value["attachments"]
        .as_array()
        .into_iter()
        .flatten()
        .take(10)
        .map(|v| json!({"filename":text(v["filename"].as_str().unwrap_or("file"))}))
        .collect();
    json!({"id":value["id"],"channel_id":value["channel_id"],"author":{"id":author["id"],"username":text(author["username"].as_str().unwrap_or_default()),"global_name":text(author["global_name"].as_str().unwrap_or_default())},"type":value["type"],"content":text(value["content"].as_str().unwrap_or_default()),"attachments":attachments})
}

#[cfg(test)]
impl Job {
    pub(super) fn fixture() -> Self {
        Self {
            events: mpsc::sync_channel(1).1,
            status: Arc::new(Mutex::new(Status {
                text: "Fictional connected".into(),
                fatal: false,
            })),
            stop: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    fn listener() -> (TcpListener, String) {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://{}/?v=9&encoding=json", server.local_addr().unwrap());
        (server, url)
    }
    fn accept(listener: &TcpListener) -> WebSocket<TcpStream> {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(_) => {
                    assert!(Instant::now() < deadline, "gateway accept timed out");
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        tungstenite::accept(stream).unwrap()
    }
    fn write(socket: &mut WebSocket<TcpStream>, value: Value) {
        socket.send(Frame::Text(value.to_string().into())).unwrap();
    }
    fn read(socket: &mut WebSocket<TcpStream>) -> Value {
        loop {
            if let Frame::Text(body) = socket.read().unwrap() {
                let v: Value = serde_json::from_str(&body).unwrap();
                if v["op"] == 1 {
                    write(socket, json!({"op":11}));
                } else {
                    return v;
                }
            }
        }
    }
    fn ready(url: &str, user: &str) -> Value {
        json!({"op":0,"s":1,"t":"READY","d":{"user":{"id":user},"session_id":"private-resume-session","resume_gateway_url":url,"private_channels":[]}})
    }
    fn until(mut check: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !check() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn wrong_gateway_identity_is_fatal_without_forwarding_data_or_credentials() {
        let (server, url) = listener();
        let copy = url.clone();
        let thread = std::thread::spawn(move || {
            let mut socket = accept(&server);
            write(
                &mut socket,
                json!({"op":10,"d":{"heartbeat_interval":1000}}),
            );
            read(&mut socket);
            write(&mut socket, ready(&copy, "wrong-user"));
        });
        let job = Job::start(
            "123".into(),
            Zeroizing::new("private-fixture-token".into()),
            url,
        );
        until(|| job.status().fatal);
        assert!(job.status().text.contains("identity mismatch"));
        assert!(!job.status().text.contains("private-fixture-token"));
        assert!(job.events.try_recv().is_err());
        drop(job);
        thread.join().unwrap();
    }
    #[test]
    fn missing_heartbeat_ack_reconnects_with_the_last_sequence() {
        let (server, url) = listener();
        let copy = url.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let mut socket = accept(&server);
            write(&mut socket, json!({"op":10,"d":{"heartbeat_interval":100}}));
            read(&mut socket);
            write(&mut socket, ready(&copy, "123"));
            // Deliberately ignore ACKs, while continuing to read the client's frames.
            while socket.read().is_ok() {}
            let mut socket = accept(&server);
            write(
                &mut socket,
                json!({"op":10,"d":{"heartbeat_interval":1000}}),
            );
            let resume = read(&mut socket);
            assert_eq!(resume["op"], 6);
            assert_eq!(resume["d"]["seq"], 1);
            write(&mut socket, json!({"op":0,"s":2,"t":"RESUMED","d":{}}));
            tx.send(()).unwrap();
            while let Ok(Frame::Text(_)) = socket.read() {
                write(&mut socket, json!({"op":11}));
            }
        });
        let job = Job::start("123".into(), Zeroizing::new("fictional-token".into()), url);
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(job);
        thread.join().unwrap();
    }
    #[test]
    fn full_queue_resumes_from_last_enqueued_event_instead_of_losing_new_dm() {
        let (server, url) = listener();
        let copy = url.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let thread = std::thread::spawn(move || {
            let mut socket = accept(&server);
            write(
                &mut socket,
                json!({"op":10,"d":{"heartbeat_interval":1000}}),
            );
            read(&mut socket);
            write(&mut socket, ready(&copy, "123"));
            for seq in 2..=65 {
                write(
                    &mut socket,
                    json!({"op":0,"s":seq,"t":"MESSAGE_CREATE","d":{"id":seq.to_string(),"channel_id":"456","author":{"id":"789"},"content":"fixture"}}),
                );
            }
            drop(socket);
            let mut socket = accept(&server);
            write(
                &mut socket,
                json!({"op":10,"d":{"heartbeat_interval":1000}}),
            );
            let resume = read(&mut socket);
            assert_eq!(resume["op"], 6);
            assert_eq!(resume["d"]["seq"], 64);
            write(
                &mut socket,
                json!({"op":0,"s":65,"t":"MESSAGE_CREATE","d":{"id":"65","channel_id":"456","author":{"id":"789"},"content":"fixture"}}),
            );
            write(&mut socket, json!({"op":0,"s":66,"t":"RESUMED","d":{}}));
            tx.send(()).unwrap();
            while let Ok(Frame::Text(_)) = socket.read() {
                write(&mut socket, json!({"op":11}));
            }
        });
        let job = Job::start("123".into(), Zeroizing::new("fictional-token".into()), url);
        until(|| job.status().text.contains("disconnected"));
        let first: Vec<_> = job.events.try_iter().collect();
        assert_eq!(first.len(), 64);
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let event = job.events.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(event.data["id"], "65");
        drop(job);
        thread.join().unwrap();
    }
    #[test]
    fn resume_url_rejects_foreign_hosts_credentials_and_downgrade() {
        for url in [
            "wss://discord.gg.attacker.invalid/",
            "ws://gateway.discord.gg/",
            "wss://user:pass@gateway.discord.gg/",
            "wss://gateway.discord.gg/private",
            "wss://gateway.discord.gg:8443/",
        ] {
            assert!(resume_url(url, URL).is_none());
        }
        assert_eq!(
            resume_url("wss://gateway-us-east1.discord.gg/", URL).unwrap(),
            "wss://gateway-us-east1.discord.gg/?v=9&encoding=json"
        );
    }
}
