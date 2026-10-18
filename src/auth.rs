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
    mut notify: impl FnMut(String),
) -> Result<()> {
    if account.client_id.trim().is_empty() {
        bail!(
            "set accounts.{}.client_id to a registered public-client application ID first",
            account.id
        );
    }
    let client = http_client(15_000)?;
    let base = format!(
        "https://login.microsoftonline.com/{}/oauth2/v2.0",
        account.tenant
    );
    let response = client
        .post(format!("{base}/devicecode"))
        .form(&[
            ("client_id", account.client_id.as_str()),
            ("scope", graph_scopes(account.provider)),
        ])
        .send()
        .context("Microsoft device-login request failed")?;
    if !response.status().is_success() {
        bail!(
            "Microsoft refused device login (HTTP {}); check app registration, public-client flow and tenant policy",
            response.status()
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
        std::thread::sleep(Duration::from_secs(interval));
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
            _ => bail!("Microsoft sign-in failed; check app registration and consent"),
        }
    }
    bail!("Microsoft device login expired; start authentication again")
}
