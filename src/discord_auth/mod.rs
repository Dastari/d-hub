//! Explicit personal-account QR setup. No messages/sends and no token in UI/IPC.
mod network;
pub mod protocol;
use crate::{
    auth::{CredentialStore, SecretService},
    domain::Provider,
    policy::AccountConfig,
};
use anyhow::{Result, bail};
use protocol::{Effect, Identity, Protocol};
use serde::Serialize;
use serde_json::Value;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub const ACTIVE: u8 = 0;
pub const SAVING: u8 = 1;
pub const CANCELLED: u8 = 2;
pub const FINISHED: u8 = 3;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Matrix {
    pub width: usize,
    pub cells: Vec<bool>,
}
impl Matrix {
    pub fn encode(content: &str) -> Result<Self> {
        let code = qrcode::QrCode::with_error_correction_level(content, qrcode::EcLevel::M)?;
        let width = code.width() + 8;
        let mut cells = vec![false; width * width];
        for y in 0..code.width() {
            for x in 0..code.width() {
                cells[(y + 4) * width + x + 4] = code[(x, y)] == qrcode::Color::Dark;
            }
        }
        Ok(Self { width, cells })
    }
    pub fn dark(&self, x: usize, y: usize) -> bool {
        y < self.width && x < self.width && self.cells[y * self.width + x]
    }
}
pub enum Event {
    Code { matrix: Matrix, expires: Instant },
    Scanned(Identity),
    Verified(Identity),
    Saving,
    Saved(Identity),
    Cancelled,
    Failed(String),
}
pub trait Transport: Send {
    fn receive(&mut self) -> Result<Option<String>>;
    fn send(&mut self, value: &Value) -> Result<()>;
    fn exchange(
        &mut self,
        ticket: &str,
        fingerprint: &str,
        protocol: &Protocol,
    ) -> Result<Zeroizing<String>>;
    fn identity(&mut self, token: &str) -> Result<Identity>;
    fn close(&mut self) {}
}
pub struct Job {
    pub account: AccountConfig,
    pub events: Receiver<Event>,
    approve: SyncSender<()>,
    gate: Arc<AtomicU8>,
}
impl Job {
    pub fn save(&self) -> bool {
        self.gate.load(Ordering::Acquire) == ACTIVE && self.approve.try_send(()).is_ok()
    }
    /// Once the keyring write starts, finish it rather than claim it was cancelled.
    pub fn cancel(&self) -> bool {
        match self
            .gate
            .compare_exchange(ACTIVE, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => true,
            Err(state) => state != SAVING,
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel();
    }
}
pub fn start(account: AccountConfig) -> Result<Job> {
    if account.provider != Provider::Discord || account.discord_mode != "personal" {
        bail!("QR sign-in is for a personal Discord account");
    }
    if !crate::policy::identifier(&account.id) {
        bail!("Invalid account ID");
    }
    if !SecretService::available() {
        bail!("Install libsecret (secret-tool) and unlock the keyring before QR setup");
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|p| !p.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("XDG_RUNTIME_DIR is required for Discord sign-in ownership")
        })?;
    let lock = std::path::PathBuf::from(runtime)
        .join("d-hub")
        .join(format!("qr-{}", account.id));
    Ok(spawn(
        account,
        move |gate| {
            let lock = crate::storage::DemoStore::open(lock).map_err(|_| {
                anyhow::anyhow!("Another window is signing in this Discord account")
            })?;
            let network = network::Network::connect(gate)?;
            Ok((Box::new(network), Some(lock)))
        },
        Box::new(SecretService),
    ))
}
type Connection = (Box<dyn Transport>, Option<crate::storage::DemoStore>);
fn spawn(
    account: AccountConfig,
    connect: impl FnOnce(&AtomicU8) -> Result<Connection> + Send + 'static,
    store: Box<dyn CredentialStore + Send>,
) -> Job {
    let (tx, events) = mpsc::sync_channel(16);
    let (approve, approval) = mpsc::sync_channel(1);
    let gate = Arc::new(AtomicU8::new(ACTIVE));
    let cancellation = gate.clone();
    let copy = account.clone();
    std::thread::spawn(move || {
        let result = (|| {
            let (mut transport, _lock) = connect(&cancellation)?;
            drive(
                &copy,
                &mut *transport,
                &*store,
                &tx,
                &approval,
                &cancellation,
            )
        })();
        let cancelled = cancellation.load(Ordering::Acquire) == CANCELLED;
        cancellation.store(FINISHED, Ordering::Release);
        if cancelled {
            let _ = tx.try_send(Event::Cancelled);
        } else if let Err(error) = result {
            let _ = tx.try_send(Event::Failed(error.to_string()));
        }
    });
    Job {
        account,
        events,
        approve,
        gate,
    }
}
fn emit(tx: &SyncSender<Event>, event: Event) -> Result<()> {
    tx.try_send(event)
        .map_err(|_| anyhow::anyhow!("Sign-in window closed or event queue full"))
}
fn active(gate: &AtomicU8) -> Result<()> {
    if gate.load(Ordering::Acquire) != ACTIVE {
        bail!("Sign-in cancelled");
    }
    Ok(())
}
fn drive(
    account: &AccountConfig,
    transport: &mut dyn Transport,
    store: &dyn CredentialStore,
    tx: &SyncSender<Event>,
    approval: &Receiver<()>,
    gate: &AtomicU8,
) -> Result<()> {
    let result = authenticate(account, transport, store, tx, approval, gate);
    transport.close();
    result
}
fn authenticate(
    account: &AccountConfig,
    transport: &mut dyn Transport,
    store: &dyn CredentialStore,
    tx: &SyncSender<Event>,
    approval: &Receiver<()>,
    gate: &AtomicU8,
) -> Result<()> {
    active(gate)?;
    let mut protocol = Protocol::new()?;
    let mut deadline = Instant::now() + Duration::from_secs(15);
    let mut heartbeat = None;
    let mut awaiting_ack = false;
    loop {
        active(gate)?;
        let now = Instant::now();
        if now >= deadline {
            bail!("Discord QR expired; press r for a new code");
        }
        if let Some(next) = heartbeat
            && now >= next
        {
            if awaiting_ack {
                bail!("Discord sign-in heartbeat timed out; press r to retry");
            }
            transport.send(&serde_json::json!({"op":"heartbeat"}))?;
            awaiting_ack = true;
            heartbeat = Some(now + Duration::from_millis(protocol.heartbeat_ms));
        }
        let Some(text) = transport.receive()? else {
            continue;
        };
        match protocol.accept(&text)? {
            Effect::Send(value) => {
                if value["op"] == "init" {
                    deadline = Instant::now() + Duration::from_millis(protocol.timeout_ms);
                    heartbeat = Some(Instant::now() + Duration::from_millis(protocol.heartbeat_ms));
                }
                transport.send(&value)?;
            }
            Effect::Code(url) => emit(
                tx,
                Event::Code {
                    matrix: Matrix::encode(&url)?,
                    expires: deadline,
                },
            )?,
            Effect::Scanned(user) => {
                if !account.discord_user_id.is_empty() && account.discord_user_id != user.id {
                    bail!(
                        "A different Discord account approved this code; use a new d-hub account ID"
                    );
                }
                emit(tx, Event::Scanned(user))?;
            }
            Effect::HeartbeatAck => awaiting_ack = false,
            Effect::Cancelled => {
                gate.store(CANCELLED, Ordering::Release);
                return Ok(());
            }
            Effect::Exchange(ticket) => {
                active(gate)?;
                let token = transport.exchange(&ticket, protocol.fingerprint(), &protocol)?;
                active(gate)?;
                let user = transport.identity(&token)?;
                if protocol
                    .user
                    .as_ref()
                    .is_none_or(|pending| pending.id != user.id)
                    || (!account.discord_user_id.is_empty() && account.discord_user_id != user.id)
                {
                    bail!("Discord identity verification mismatch; session was not saved");
                }
                transport.close();
                emit(tx, Event::Verified(user.clone()))?;
                let until = Instant::now() + Duration::from_secs(60);
                loop {
                    active(gate)?;
                    if Instant::now() >= until {
                        bail!("Account confirmation expired; retry QR sign-in");
                    }
                    match approval.recv_timeout(Duration::from_millis(100)) {
                        Ok(()) => break,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(_) => bail!("Sign-in window closed"),
                    }
                }
                if gate
                    .compare_exchange(ACTIVE, SAVING, Ordering::AcqRel, Ordering::Acquire)
                    .is_err()
                {
                    bail!("Sign-in cancelled");
                }
                emit(tx, Event::Saving)?;
                #[derive(Serialize)]
                struct Credential<'a> {
                    version: u8,
                    kind: &'static str,
                    user_id: &'a str,
                    username: &'a str,
                    token: &'a str,
                }
                let credential = Zeroizing::new(serde_json::to_string(&Credential {
                    version: 1,
                    kind: "discord_personal",
                    user_id: &user.id,
                    username: &user.username,
                    token: &token,
                })?);
                // No plaintext fallback; failures never produce a Saved event.
                store
                    .store(&account.credential_ref(), &credential)
                    .map_err(|_| {
                        anyhow::anyhow!(
                            "Could not save Discord session; unlock the keyring and retry"
                        )
                    })?;
                emit(tx, Event::Saved(user))?;
                return Ok(());
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Connecting,
    Scan,
    Approve,
    Review,
    Saving,
    Saved,
    Failed,
    Cancelled,
    RetryRequested,
}
pub struct Setup {
    pub account: String,
    pub phase: Phase,
    pub matrix: Option<Matrix>,
    pub expires: Option<Instant>,
    pub user: Option<Identity>,
    pub status: String,
    pub job: Option<Job>,
    pub seconds: Option<u64>,
}
impl Setup {
    pub fn new(account: &str) -> Self {
        Self {
            account: account.into(),
            phase: Phase::Connecting,
            matrix: None,
            expires: None,
            user: None,
            status: "Connecting to Discord…".into(),
            job: None,
            seconds: None,
        }
    }
    pub fn apply(&mut self, event: Event) -> Option<Identity> {
        match event {
            Event::Code { matrix, expires } => {
                self.phase = Phase::Scan;
                self.matrix = Some(matrix);
                self.expires = Some(expires);
                self.status = "Discord mobile app → Settings → Scan QR Code".into();
            }
            Event::Scanned(user) => {
                self.phase = Phase::Approve;
                self.user = Some(user);
                self.matrix = None;
                self.status = "Approve this sign-in in the Discord mobile app.".into();
            }
            Event::Verified(user) => {
                self.phase = Phase::Review;
                self.user = Some(user);
                self.matrix = None;
                self.expires = Some(Instant::now() + Duration::from_secs(60));
                self.status = "Identity verified. Enter saves this session in your keyring.".into();
            }
            Event::Saving => {
                self.phase = Phase::Saving;
                self.matrix = None;
                self.expires = None;
                self.status = "Saving session in desktop keyring…".into();
            }
            Event::Saved(user) => {
                self.phase = Phase::Saved;
                self.user = Some(user.clone());
                self.matrix = None;
                self.expires = None;
                self.status="Signed in · session stored in keyring. DM sync/replies are the next integration step.".into();
                return Some(user);
            }
            Event::Cancelled => {
                self.phase = Phase::Cancelled;
                self.matrix = None;
                self.expires = None;
                self.status = "Sign-in cancelled. No session saved.".into();
            }
            Event::Failed(error) => {
                self.phase = Phase::Failed;
                self.matrix = None;
                self.expires = None;
                self.status = crate::domain::safe_text(&error);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{
        Engine,
        engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    };
    use openssl::pkey::{PKey, Public};
    use protocol::tests::encrypt;
    use sha2::{Digest, Sha256};
    use std::{collections::VecDeque, sync::Mutex};
    #[derive(Default)]
    struct Memory {
        stored: Mutex<Vec<(String, String)>>,
        fail: bool,
    }
    impl CredentialStore for Arc<Memory> {
        fn store(&self, key: &str, secret: &str) -> Result<()> {
            if self.fail {
                bail!("fictional-session must never be echoed");
            }
            self.stored
                .lock()
                .unwrap()
                .push((key.into(), secret.into()));
            Ok(())
        }
        fn load(&self, _: &str) -> Result<Zeroizing<String>> {
            bail!("No lookup in QR tests")
        }
        fn remove(&self, _: &str) -> Result<()> {
            Ok(())
        }
    }
    struct Fake {
        input: VecDeque<String>,
        key: Option<PKey<Public>>,
        wrong_user: bool,
        cancel: bool,
        stall_scan: bool,
    }
    impl Fake {
        fn new() -> Self {
            Self {
                input: VecDeque::from([
                    serde_json::json!({"op":"hello","heartbeat_interval":1000,"timeout_ms":30000})
                        .to_string(),
                ]),
                key: None,
                wrong_user: false,
                cancel: false,
                stall_scan: false,
            }
        }
    }
    impl Transport for Fake {
        fn receive(&mut self) -> Result<Option<String>> {
            if self.input.is_empty() {
                std::thread::sleep(Duration::from_millis(2));
            }
            Ok(self.input.pop_front())
        }
        fn send(&mut self, value: &Value) -> Result<()> {
            match value["op"].as_str().unwrap() {
                "init" => {
                    let key = PKey::public_key_from_der(
                        &STANDARD.decode(value["encoded_public_key"].as_str().unwrap())?,
                    )?;
                    self.input.push_back(serde_json::json!({"op":"nonce_proof","encrypted_nonce":encrypt(&key,b"fake-nonce")}).to_string());
                    self.key = Some(key);
                }
                "nonce_proof" => {
                    let key = self.key.as_ref().unwrap();
                    let fingerprint =
                        URL_SAFE_NO_PAD.encode(Sha256::digest(key.public_key_to_der()?));
                    self.input.push_back(
                        serde_json::json!({"op":"pending_remote_init","fingerprint":fingerprint})
                            .to_string(),
                    );
                    if self.stall_scan {
                        return Ok(());
                    }
                    self.input.push_back(if self.cancel {serde_json::json!({"op":"cancel"})}else{serde_json::json!({"op":"pending_ticket","encrypted_user_payload":encrypt(key,b"123456789:0:avatar:Fictional")})}.to_string());
                    self.input.push_back(
                        serde_json::json!({"op":"pending_login","ticket":"fictional-ticket"})
                            .to_string(),
                    );
                }
                "heartbeat" if !self.stall_scan => self
                    .input
                    .push_back(serde_json::json!({"op":"heartbeat_ack"}).to_string()),
                "heartbeat" => {}
                _ => bail!("Unexpected fake client operation"),
            }
            Ok(())
        }
        fn exchange(
            &mut self,
            ticket: &str,
            _: &str,
            protocol: &Protocol,
        ) -> Result<Zeroizing<String>> {
            assert_eq!(ticket, "fictional-ticket");
            let encrypted = encrypt(self.key.as_ref().unwrap(), b"fictional-session");
            Ok(Zeroizing::new(String::from_utf8(
                protocol.decrypt(&encrypted)?.to_vec(),
            )?))
        }
        fn identity(&mut self, token: &str) -> Result<Identity> {
            assert_eq!(token, "fictional-session");
            Ok(Identity {
                id: if self.wrong_user {
                    "987654321"
                } else {
                    "123456789"
                }
                .into(),
                username: "Fictional".into(),
            })
        }
    }
    fn account() -> AccountConfig {
        AccountConfig::new("personal-qr", Provider::Discord)
    }
    fn next(job: &Job) -> Event {
        job.events
            .recv_timeout(Duration::from_secs(5))
            .expect("fake sign-in timed out")
    }
    fn verified(job: &Job) {
        loop {
            match next(job) {
                Event::Verified(user) => {
                    assert_eq!(user.id, "123456789");
                    return;
                }
                Event::Failed(e) => panic!("Fake login failed: {e}"),
                _ => {}
            }
        }
    }
    #[test]
    fn complete_fake_login_requires_local_confirmation_and_keeps_tokens_out_of_events() {
        let store = Arc::new(Memory::default());
        let record = store.clone();
        let job = spawn(
            account(),
            |_| Ok((Box::new(Fake::new()), None)),
            Box::new(store),
        );
        verified(&job);
        assert!(record.stored.lock().unwrap().is_empty());
        assert!(job.save());
        loop {
            match next(&job) {
                Event::Saved(user) => {
                    assert_eq!(user.id, "123456789");
                    break;
                }
                Event::Failed(e) => panic!("{e}"),
                _ => {}
            }
        }
        let stored = record.stored.lock().unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].0, "discord-personal-personal-qr");
        let credential: Value = serde_json::from_str(&stored[0].1).unwrap();
        assert_eq!(credential["kind"], "discord_personal");
        assert_eq!(credential["user_id"], "123456789");
        assert_eq!(credential["token"], "fictional-session");
    }
    #[test]
    fn cancellation_after_mobile_approval_never_stores_a_session() {
        let store = Arc::new(Memory::default());
        let record = store.clone();
        let job = spawn(
            account(),
            |_| Ok((Box::new(Fake::new()), None)),
            Box::new(store),
        );
        verified(&job);
        assert!(job.cancel());
        loop {
            if matches!(next(&job), Event::Cancelled) {
                break;
            }
        }
        assert!(record.stored.lock().unwrap().is_empty());
    }
    #[test]
    fn mismatched_verified_identity_and_pinned_accounts_cannot_be_saved() {
        for pinned in [false, true] {
            let store = Arc::new(Memory::default());
            let record = store.clone();
            let mut a = account();
            if pinned {
                a.discord_user_id = "987654321".into();
            }
            let job = spawn(
                a,
                move |_| {
                    let mut fake = Fake::new();
                    fake.wrong_user = !pinned;
                    Ok((Box::new(fake), None))
                },
                Box::new(store),
            );
            loop {
                match next(&job) {
                    Event::Failed(error) => {
                        assert!(error.contains("identity") || error.contains("different Discord"));
                        break;
                    }
                    Event::Saved(_) => panic!("Wrong user saved"),
                    _ => {}
                }
            }
            assert!(record.stored.lock().unwrap().is_empty());
        }
    }
    #[test]
    fn keyring_failure_is_visible_has_no_plaintext_fallback_and_does_not_echo_tokens() {
        let store = Arc::new(Memory {
            fail: true,
            ..Default::default()
        });
        let job = spawn(
            account(),
            |_| Ok((Box::new(Fake::new()), None)),
            Box::new(store),
        );
        verified(&job);
        assert!(job.save());
        loop {
            match next(&job) {
                Event::Failed(error) => {
                    assert!(error.contains("keyring"));
                    assert!(!error.contains("fictional-session"));
                    break;
                }
                Event::Saved(_) => panic!("Storage failure cannot be success"),
                _ => {}
            }
        }
    }
    #[test]
    fn remote_cancel_clears_code_and_does_not_store_credentials() {
        let store = Arc::new(Memory::default());
        let record = store.clone();
        let job = spawn(
            account(),
            |_| {
                let mut fake = Fake::new();
                fake.cancel = true;
                Ok((Box::new(fake), None))
            },
            Box::new(store),
        );
        let mut setup = Setup::new("personal-qr");
        loop {
            let event = next(&job);
            let cancelled = matches!(event, Event::Cancelled);
            setup.apply(event);
            if cancelled {
                break;
            }
        }
        assert!(setup.matrix.is_none());
        assert!(setup.expires.is_none());
        assert!(record.stored.lock().unwrap().is_empty());
    }
    #[test]
    fn missing_heartbeat_ack_fails_and_clears_the_code_without_a_keyring_write() {
        let store = Arc::new(Memory::default());
        let record = store.clone();
        let job = spawn(
            account(),
            |_| {
                let mut fake = Fake::new();
                fake.stall_scan = true;
                Ok((Box::new(fake), None))
            },
            Box::new(store),
        );
        let mut setup = Setup::new("personal-qr");
        loop {
            let event = next(&job);
            let failed = matches!(event, Event::Failed(_));
            setup.apply(event);
            if failed {
                break;
            }
        }
        assert!(setup.status.contains("heartbeat"));
        assert!(setup.matrix.is_none());
        assert!(record.stored.lock().unwrap().is_empty());
    }
    #[test]
    fn personal_credentials_use_a_namespace_that_cannot_collide_with_bot_or_mail_ids() {
        let personal = account();
        let mut bot = AccountConfig::new("personal-qr-discord-personal", Provider::Discord);
        bot.discord_mode = "bot".into();
        assert_ne!(personal.credential_ref(), bot.credential_ref());
        assert_ne!(
            personal.credential_ref(),
            AccountConfig::new("personal-qr", Provider::ICloud).credential_ref()
        );
    }
}
