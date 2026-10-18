use crate::{domain::Provider, policy::AccountConfig};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub trait CredentialStore {
    fn store(&self, reference: &str, secret: &str) -> Result<()>;
    fn load(&self, reference: &str) -> Result<Zeroizing<String>>;
    fn remove(&self, reference: &str) -> Result<()>;
}

pub struct SecretService;
impl SecretService {
    pub fn available() -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::env::var_os("PATH").is_some_and(|paths| {
            std::env::split_paths(&paths).any(|path| {
                path.join("secret-tool")
                    .metadata()
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            })
        })
    }
}
impl CredentialStore for SecretService {
    fn store(&self, reference: &str, secret: &str) -> Result<()> {
        if secret.is_empty() || secret.len() > 16_384 {
            bail!("credential must contain 1–16384 bytes");
        }
        let mut child = Command::new("secret-tool").args(["store", "--label=d-hub", "service", "d-hub", "credential", reference])
            .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn()
            .context("Secret Service helper unavailable; install libsecret (secret-tool) and unlock your desktop keyring")?;
        child
            .stdin
            .take()
            .context("credential input unavailable")?
            .write_all(secret.as_bytes())?;
        if !child.wait()?.success() {
            bail!("credential store failed; check the desktop Secret Service/keyring");
        }
        Ok(())
    }
    fn load(&self, reference: &str) -> Result<Zeroizing<String>> {
        let output = Command::new("secret-tool")
            .args(["lookup", "service", "d-hub", "credential", reference])
            .stderr(Stdio::null())
            .output()
            .context("Secret Service helper unavailable")?;
        if !output.status.success() {
            bail!("credential missing or keyring locked");
        }
        let value =
            String::from_utf8(output.stdout).context("invalid stored credential encoding")?;
        Ok(Zeroizing::new(value.trim_end_matches('\n').into()))
    }
    fn remove(&self, reference: &str) -> Result<()> {
        let status = Command::new("secret-tool")
            .args(["clear", "service", "d-hub", "credential", reference])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("Secret Service helper unavailable")?;
        if !status.success() {
            bail!("credential could not be removed");
        }
        Ok(())
    }
}

pub fn read_secret(stdin: bool, prompt: &str) -> Result<Zeroizing<String>> {
    if stdin {
        let mut value = Zeroizing::new(String::new());
        std::io::stdin().take(16_385).read_to_string(&mut value)?;
        if value.len() > 16_384 {
            bail!("credential exceeds 16384 bytes");
        }
        Ok(Zeroizing::new(value.trim_end_matches(['\r', '\n']).into()))
    } else {
        Ok(Zeroizing::new(rpassword::prompt_password(prompt)?))
    }
}

pub fn http_client(timeout_ms: u64) -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_millis(timeout_ms))
        .build()?)
}

pub fn response_json(response: reqwest::blocking::Response) -> Result<Value> {
    let mut bytes = Vec::new();
    response
        .take(1_048_577)
        .read_to_end(&mut bytes)
        .context("could not read provider response")?;
    if bytes.len() > 1_048_576 {
        bail!("provider response exceeded one MiB");
    }
    serde_json::from_slice(&bytes).context("provider returned invalid JSON")
}

/// Microsoft's own explanation (an AADSTS description or Graph error code and message) for
/// status lines: first line only, bounded and free of terminal controls. Error bodies carry no
/// tokens; trace and correlation IDs after the first line are dropped.
pub fn microsoft_error(value: &Value) -> Option<String> {
    let (code, text) = match value["error"].as_object() {
        Some(error) => (error.get("code"), error.get("message")),
        None => (value.get("error"), value.get("error_description")),
    };
    let code = code?.as_str()?;
    let text = text
        .and_then(Value::as_str)
        .and_then(|t| t.lines().next())
        .unwrap_or("")
        .trim();
    let detail = if text.starts_with("AADSTS") {
        text.to_owned()
    } else if text.is_empty() {
        code.to_owned()
    } else {
        format!("{code}: {text}")
    };
    let detail = crate::domain::safe_text(&detail);
    Some(match detail.char_indices().nth(200) {
        Some((end, _)) => format!("{}…", &detail[..end]),
        None => detail,
    })
}

/// Reads a failed Microsoft response's explanation, if it sent one.
pub fn microsoft_detail(response: reqwest::blocking::Response) -> String {
    response_json(response)
        .ok()
        .and_then(|value| microsoft_error(&value))
        .map_or_else(String::new, |detail| format!(" · {detail}"))
}

pub fn graph_scopes(provider: Provider) -> &'static str {
    match provider {
        Provider::Teams => {
            "offline_access https://graph.microsoft.com/User.Read https://graph.microsoft.com/Chat.ReadWrite"
        }
        _ => {
            "offline_access https://graph.microsoft.com/User.Read https://graph.microsoft.com/Mail.ReadWrite https://graph.microsoft.com/Mail.Send"
        }
    }
}

/// Explicit setup only. No provider messages are read or sent by authentication.
pub fn authenticate_graph(
    account: &AccountConfig,
    store: &impl CredentialStore,
    notify: impl FnMut(String),
) -> Result<()> {
    if account.client_id.trim().is_empty() {
        bail!(
            "set accounts.{}.client_id to a registered public-client application ID first",
            account.id
        );
    }
    let base = format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0",
        account.tenant
    );
    device_login(
        &http_client(15_000)?,
        &base,
        account,
        store,
        notify,
        std::thread::sleep,
    )
}
/// The OAuth device-code grant against `base`; `wait` paces the token polls.
fn device_login(
    client: &reqwest::blocking::Client,
    base: &str,
    account: &AccountConfig,
    store: &impl CredentialStore,
    mut notify: impl FnMut(String),
    mut wait: impl FnMut(Duration),
) -> Result<()> {
    let response = client
        .post(format!("{base}/devicecode"))
        .form(&[
            ("client_id", account.client_id.as_str()),
            ("scope", graph_scopes(account.provider)),
        ])
        .send()
        .context("Microsoft device-login request failed")?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        bail!(
            "Microsoft refused device login (HTTP {status}{}); check app registration, public-client flow and tenant",
            microsoft_detail(response)
        );
    }
    let device = response_json(response)?;
    let code = Zeroizing::new(
        device["device_code"]
            .as_str()
            .context("device code missing")?
            .to_owned(),
    );
    let user_code = device["user_code"].as_str().context("user code missing")?;
    let uri = device["verification_uri"]
        .as_str()
        .context("verification URI missing")?;
    if !uri.starts_with("https://") {
        bail!("Microsoft returned an invalid verification URI");
    }
    notify(format!("Open {uri} and enter {user_code}."));
    let mut interval = device["interval"].as_u64().unwrap_or(5).clamp(1, 60);
    let expires = Instant::now()
        + Duration::from_secs(device["expires_in"].as_u64().unwrap_or(900).min(1800));
    while Instant::now() < expires {
        wait(Duration::from_secs(interval));
        let response = client
            .post(format!("{base}/token"))
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("client_id", account.client_id.as_str()),
                ("device_code", code.as_str()),
            ])
            .send()
            .context("Microsoft token request failed")?;
        let success = response.status().is_success();
        let value = response_json(response)?;
        if success {
            let refresh = value["refresh_token"]
                .as_str()
                .context("Microsoft did not return a refresh token; offline_access is required")?;
            let stored = Zeroizing::new(serde_json::to_string(
                &serde_json::json!({"refresh_token": refresh, "tenant": account.tenant, "client_id": account.client_id, "scope": value["scope"]}),
            )?);
            store.store(&account.credential_ref(), &stored)?;
            notify("Microsoft credential stored. Message transport is not connected yet.".into());
            return Ok(());
        }
        match value["error"].as_str() {
            Some("authorization_pending") => {}
            Some("slow_down") => interval = (interval + 5).min(60),
            Some("authorization_declined" | "access_denied") => {
                bail!("Microsoft sign-in was declined")
            }
            Some("expired_token") => break,
            _ => bail!(
                "Microsoft sign-in failed{}; check app registration and consent",
                microsoft_error(&value).map_or_else(String::new, |d| format!(" · {d}"))
            ),
        }
    }
    bail!("Microsoft device login expired; start authentication again")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
    };
    #[derive(Default)]
    struct Memory(Mutex<Vec<(String, String)>>);
    impl CredentialStore for Memory {
        fn store(&self, reference: &str, secret: &str) -> Result<()> {
            self.0
                .lock()
                .unwrap()
                .push((reference.into(), secret.into()));
            Ok(())
        }
        fn load(&self, _: &str) -> Result<Zeroizing<String>> {
            bail!("unused")
        }
        fn remove(&self, _: &str) -> Result<()> {
            Ok(())
        }
    }
    /// A fictional Microsoft identity endpoint answering each request in turn.
    fn identity(responses: Vec<(u16, Value)>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!(
            "http://{}/tenant/oauth2/v2.0",
            listener.local_addr().unwrap()
        );
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = requests.clone();
        std::thread::spawn(move || {
            for (status, value) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request = String::new();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if let Some(v) = line.to_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap();
                    }
                    request.push_str(&line);
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                request.push_str(&String::from_utf8(body).unwrap());
                observed.lock().unwrap().push(request);
                let body = value.to_string();
                write!(
                    stream,
                    "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        (base, requests)
    }
    fn account(provider: Provider) -> AccountConfig {
        let mut a = AccountConfig::new("work", provider);
        a.client_id = "fixture-client".into();
        a
    }
    fn device() -> (u16, Value) {
        (
            200,
            serde_json::json!({"device_code":"fixture-device-code","user_code":"FICTION1","verification_uri":"https://microsoft.com/devicelogin","interval":5,"expires_in":900}),
        )
    }
    fn login(
        provider: Provider,
        responses: Vec<(u16, Value)>,
    ) -> (Result<()>, Memory, Vec<String>, Vec<u64>, Vec<String>) {
        let (base, requests) = identity(responses);
        let store = Memory::default();
        let (mut notes, mut waits) = (vec![], vec![]);
        let result = device_login(
            // The production client is HTTPS-only; the fictional endpoint is local HTTP.
            &reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(2))
                .build()
                .unwrap(),
            &base,
            &account(provider),
            &store,
            |note| notes.push(note),
            |wait| waits.push(wait.as_secs()),
        );
        let requests = requests.lock().unwrap().clone();
        (result, store, notes, waits, requests)
    }
    #[test]
    fn device_login_waits_for_approval_honours_slow_down_and_stores_only_the_refresh_token() {
        let pending = |error| (400, serde_json::json!({"error": error}));
        let (result, store, notes, waits, requests) = login(
            Provider::Office365,
            vec![
                device(),
                pending("authorization_pending"),
                pending("slow_down"),
                (
                    200,
                    serde_json::json!({"access_token":"fixture-access","refresh_token":"fixture-refresh","scope":"Mail.ReadWrite Mail.Send"}),
                ),
            ],
        );
        result.unwrap();
        assert_eq!(
            notes[0],
            "Open https://microsoft.com/devicelogin and enter FICTION1."
        );
        assert!(notes[1].contains("credential stored"));
        assert_eq!(waits, [5, 5, 10], "slow_down adds five seconds");
        assert!(requests[0].starts_with("POST /tenant/oauth2/v2.0/devicecode "));
        assert!(requests[0].contains("client_id=fixture-client"));
        assert!(requests[0].contains("Mail.Send"), "{}", requests[0]);
        assert!(requests[1].starts_with("POST /tenant/oauth2/v2.0/token "));
        assert!(requests[1].contains("device_code=fixture-device-code"));
        let stored = store.0.lock().unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].0, account(Provider::Office365).credential_ref());
        let saved: Value = serde_json::from_str(&stored[0].1).unwrap();
        assert_eq!(saved["refresh_token"], "fixture-refresh");
        assert_eq!(saved["client_id"], "fixture-client");
        assert!(
            !stored[0].1.contains("fixture-access"),
            "access tokens are not kept"
        );
    }
    #[test]
    fn device_login_failures_explain_themselves_and_store_nothing() {
        let declined = (400, serde_json::json!({"error":"authorization_declined"}));
        let (result, store, ..) = login(Provider::Teams, vec![device(), declined]);
        assert!(result.unwrap_err().to_string().contains("declined"));
        assert!(store.0.lock().unwrap().is_empty());

        // An app registration without public client flows enabled.
        let refused = (
            400,
            serde_json::json!({"error":"invalid_client","error_description":"AADSTS7000218: The request body must contain client_assertion.\r\nTrace ID: fictional"}),
        );
        let (result, store, _, _, requests) = login(Provider::Teams, vec![refused]);
        let error = result.unwrap_err().to_string();
        assert!(
            error.contains("AADSTS7000218") && error.contains("public-client"),
            "{error}"
        );
        assert!(!error.contains("Trace ID"), "{error}");
        assert!(requests[0].contains("Chat.ReadWrite"), "{}", requests[0]);
        assert!(store.0.lock().unwrap().is_empty());

        let mut unsafe_uri = device();
        unsafe_uri.1["verification_uri"] = "http://phish.invalid/devicelogin".into();
        let (result, ..) = login(Provider::Teams, vec![unsafe_uri]);
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("invalid verification URI")
        );

        let no_refresh = (200, serde_json::json!({"access_token":"fixture-access"}));
        let (result, store, ..) = login(Provider::Teams, vec![device(), no_refresh]);
        assert!(result.unwrap_err().to_string().contains("offline_access"));
        assert!(store.0.lock().unwrap().is_empty());

        let expired = (400, serde_json::json!({"error":"expired_token"}));
        let (result, ..) = login(Provider::Teams, vec![device(), expired]);
        assert!(result.unwrap_err().to_string().contains("expired"));
    }
}
