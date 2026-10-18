use super::{
    Transport,
    protocol::{Identity, Protocol, snowflake},
};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    io::ErrorKind,
    net::{TcpStream, ToSocketAddrs},
    sync::atomic::{AtomicU8, Ordering},
    time::{Duration, Instant},
};
use tungstenite::{
    Message, WebSocket, client::IntoClientRequest, protocol::WebSocketConfig,
    stream::MaybeTlsStream,
};
use zeroize::Zeroizing;
const GATEWAY: &str = "wss://remote-auth-gateway.discord.gg/?v=2";
const LOGIN: &str = "https://discord.com/api/v10/users/@me/remote-auth/login";
const ME: &str = "https://discord.com/api/v10/users/@me";
pub struct Network {
    socket: WebSocket<MaybeTlsStream<TcpStream>>,
    http: reqwest::blocking::Client,
}
impl Network {
    pub fn connect(cancel: &AtomicU8) -> Result<Self> {
        let deadline = Instant::now() + Duration::from_secs(12);
        let mut connected = None;
        for address in ("remote-auth-gateway.discord.gg", 443)
            .to_socket_addrs()
            .context("Discord gateway lookup failed")?
            .take(4)
        {
            if cancel.load(Ordering::Acquire) != 0 {
                bail!("Sign-in cancelled");
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            if let Ok(stream) =
                TcpStream::connect_timeout(&address, remaining.min(Duration::from_secs(4)))
            {
                connected = Some(stream);
                break;
            }
        }
        let stream = connected.context("Could not connect to Discord sign-in")?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        let mut request = GATEWAY.into_client_request()?;
        request
            .headers_mut()
            .insert("Origin", "https://discord.com".parse()?);
        request.headers_mut().insert(
            "User-Agent",
            concat!("d-hub/", env!("CARGO_PKG_VERSION")).parse()?,
        );
        let config = WebSocketConfig::default()
            .read_buffer_size(4096)
            .write_buffer_size(0)
            .max_write_buffer_size(65_536)
            .max_message_size(Some(16_384))
            .max_frame_size(Some(16_384));
        let (socket, _) = tungstenite::client_tls_with_config(request, stream, Some(config), None)
            .map_err(|_| anyhow::anyhow!("Discord TLS/WebSocket sign-in failed"))?;
        match socket.get_ref() {
            MaybeTlsStream::NativeTls(s) => s
                .get_ref()
                .set_read_timeout(Some(Duration::from_millis(200)))?,
            _ => bail!("Discord sign-in requires validated TLS"),
        }
        Ok(Self {
            socket,
            http: crate::auth::http_client(10_000)?,
        })
    }
}
impl Transport for Network {
    fn close(&mut self) {
        let _ = self.socket.close(None);
    }
    fn receive(&mut self) -> Result<Option<String>> {
        match self.socket.read() {
            Ok(Message::Text(value)) => Ok(Some(value.to_string())),
            Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {
                self.socket
                    .flush()
                    .map_err(|_| anyhow::anyhow!("Discord connection interrupted"))?;
                Ok(None)
            }
            Ok(Message::Close(Some(close))) if u16::from(close.code) == 4003 => {
                bail!("Discord QR expired; press r for a new code")
            }
            Ok(Message::Close(_)) => bail!("Discord sign-in connection closed; press r to retry"),
            Ok(_) => bail!("Unexpected Discord sign-in frame"),
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                Ok(None)
            }
            Err(_) => bail!("Discord sign-in connection interrupted"),
        }
    }
    fn send(&mut self, value: &Value) -> Result<()> {
        self.socket
            .send(Message::Text(value.to_string().into()))
            .map_err(|_| anyhow::anyhow!("Discord sign-in connection interrupted"))
    }
    fn exchange(
        &mut self,
        ticket: &str,
        fingerprint: &str,
        protocol: &Protocol,
    ) -> Result<Zeroizing<String>> {
        exchange_http(&self.http, LOGIN, ticket, fingerprint, protocol)
    }
    fn identity(&mut self, token: &str) -> Result<Identity> {
        identity_http(&self.http, ME, token)
    }
}
fn exchange_http(
    http: &reqwest::blocking::Client,
    endpoint: &str,
    ticket: &str,
    fingerprint: &str,
    protocol: &Protocol,
) -> Result<Zeroizing<String>> {
    let response = http
        .post(endpoint)
        .header("Origin", "https://discord.com")
        .header("X-Fingerprint", fingerprint)
        .header("User-Agent", concat!("d-hub/", env!("CARGO_PKG_VERSION")))
        .json(&json!({"ticket":ticket}))
        .send()
        .map_err(|_| anyhow::anyhow!("Discord ticket exchange interrupted"))?;
    let status = response.status();
    if status.as_u16() == 429 {
        bail!("Discord rate limited sign-in; wait before retrying");
    }
    let value = crate::auth::response_json(response)
        .map_err(|_| anyhow::anyhow!("Invalid Discord ticket exchange response"))?;
    if !status.is_success() {
        if value.get("captcha_key").is_some() {
            bail!(
                "Discord requires CAPTCHA verification; finish verification in the official client, then retry QR sign-in"
            );
        }
        bail!("Discord refused QR sign-in; retry from the official mobile app");
    }
    let encrypted = value["encrypted_token"]
        .as_str()
        .context("Encrypted Discord session missing")?;
    let decoded = protocol.decrypt(encrypted)?;
    let token =
        std::str::from_utf8(&decoded).map_err(|_| anyhow::anyhow!("Invalid Discord session"))?;
    if token.is_empty()
        || token.len() > 4096
        || token
            .bytes()
            .any(|c| c.is_ascii_whitespace() || c.is_ascii_control())
    {
        bail!("Invalid Discord session");
    }
    Ok(Zeroizing::new(token.into()))
}
fn identity_http(
    http: &reqwest::blocking::Client,
    endpoint: &str,
    token: &str,
) -> Result<Identity> {
    let response = http
        .get(endpoint)
        .header("Authorization", token)
        .header("User-Agent", concat!("d-hub/", env!("CARGO_PKG_VERSION")))
        .send()
        .map_err(|_| anyhow::anyhow!("Could not verify Discord account"))?;
    if !response.status().is_success() {
        bail!("Discord session verification failed");
    }
    let value = crate::auth::response_json(response)
        .map_err(|_| anyhow::anyhow!("Invalid Discord account response"))?;
    let id = value["id"]
        .as_str()
        .filter(|v| snowflake(v))
        .context("Discord user ID missing")?;
    let username = value["username"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 128)
        .context("Discord username missing")?;
    Ok(Identity {
        id: id.into(),
        username: crate::domain::safe_text(username),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discord_auth::protocol::tests::{encrypt, initialize};
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
    };
    fn http_server(
        status: u16,
        response: Value,
    ) -> (String, Arc<Mutex<String>>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/test", listener.local_addr().unwrap());
        let observed = Arc::new(Mutex::new(String::new()));
        let capture = observed.clone();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                request.push_str(&line);
                if line == "\r\n" {
                    break;
                }
            }
            let length = request
                .lines()
                .find_map(|l| {
                    l.to_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|n| n.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            request.push_str(std::str::from_utf8(&body).unwrap());
            *capture.lock().unwrap() = request;
            let body = response.to_string();
            write!(stream,"HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        });
        (url, observed, handle)
    }
    fn client() -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(2))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    }
    #[test]
    fn fake_http_exchanges_encrypted_session_and_verifies_user_without_leaking_it() {
        let mut protocol = Protocol::new().unwrap();
        let key = initialize(&mut protocol);
        let (url, captured, server) = http_server(
            200,
            json!({"encrypted_token":encrypt(&key,b"fictional-session")}),
        );
        let token = exchange_http(
            &client(),
            &url,
            "fictional-ticket",
            protocol.fingerprint(),
            &protocol,
        )
        .unwrap();
        assert_eq!(token.as_str(), "fictional-session");
        server.join().unwrap();
        let request = captured.lock().unwrap();
        assert!(request.starts_with("POST /test "));
        assert!(request.contains("x-fingerprint:"));
        assert!(request.contains("origin: https://discord.com"));
        assert!(request.contains("fictional-ticket"));
        assert!(!request.contains("fictional-session"));
        drop(request);
        let (url, captured, server) =
            http_server(200, json!({"id":"123456789","username":"Fictional"}));
        let identity = identity_http(&client(), &url, &token).unwrap();
        assert_eq!(identity.id, "123456789");
        server.join().unwrap();
        assert!(
            captured
                .lock()
                .unwrap()
                .contains("authorization: fictional-session")
        );
    }
    #[test]
    fn captcha_rate_limit_and_bad_tokens_are_errors_without_echoing_provider_bodies() {
        let mut protocol = Protocol::new().unwrap();
        let key = initialize(&mut protocol);
        for (status, body, expected) in [
            (
                400,
                json!({"captcha_key":["captcha-required"],"ticket":"private-ticket"}),
                "CAPTCHA",
            ),
            (429, json!({"token":"private-token"}), "rate limited"),
            (
                200,
                json!({"encrypted_token":encrypt(&key,b"invalid\nheader")}),
                "Invalid Discord session",
            ),
        ] {
            let (url, _, server) = http_server(status, body);
            let error = exchange_http(
                &client(),
                &url,
                "fictional-ticket",
                protocol.fingerprint(),
                &protocol,
            )
            .unwrap_err()
            .to_string();
            server.join().unwrap();
            assert!(error.contains(expected));
            assert!(!error.contains("private-"));
            assert!(!error.contains("fictional-ticket"));
        }
    }
    #[test]
    fn fake_websocket_handles_partial_reads_ping_and_expired_close() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut ws = tungstenite::accept(stream).unwrap();
            ws.send(Message::Text(r#"{"op":"hello"}"#.into())).unwrap();
            ws.send(Message::Ping(vec![1, 2].into())).unwrap();
            assert!(matches!(ws.read().unwrap(), Message::Pong(_)));
            ws.close(Some(tungstenite::protocol::CloseFrame {
                code: tungstenite::protocol::frame::coding::CloseCode::from(4003),
                reason: "fictional".into(),
            }))
            .unwrap();
        });
        let stream = TcpStream::connect(address).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (socket, _) =
            tungstenite::client_tls_with_config(format!("ws://{address}/"), stream, None, None)
                .unwrap();
        let mut network = Network {
            socket,
            http: client(),
        };
        assert!(network.receive().unwrap().unwrap().contains("hello"));
        assert!(network.receive().unwrap().is_none());
        assert!(
            network
                .receive()
                .unwrap_err()
                .to_string()
                .contains("expired")
        );
        server.join().unwrap();
    }
}
