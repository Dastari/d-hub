//! Independent remote-auth v2 implementation. No private values are serializable/debuggable.
use anyhow::{Context, Result, bail};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use openssl::{
    encrypt::Decrypter,
    hash::MessageDigest,
    pkey::{PKey, Private},
    rsa::{Padding, Rsa},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub id: String,
    pub username: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Hello,
    Nonce,
    Fingerprint,
    Scan,
    Approval,
    Exchange,
}
pub enum Effect {
    Send(Value),
    Code(String),
    Scanned(Identity),
    Exchange(Zeroizing<String>),
    Cancelled,
    HeartbeatAck,
}
pub struct Protocol {
    key: PKey<Private>,
    fingerprint: String,
    stage: Stage,
    pub heartbeat_ms: u64,
    pub timeout_ms: u64,
    pub user: Option<Identity>,
}
pub fn snowflake(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && value.bytes().all(|b| b.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|v| v > 0)
}
impl Protocol {
    pub fn new() -> Result<Self> {
        let key =
            PKey::from_rsa(Rsa::generate(2048).context("Could not create Discord sign-in key")?)?;
        let fingerprint = URL_SAFE_NO_PAD.encode(Sha256::digest(key.public_key_to_der()?));
        Ok(Self {
            key,
            fingerprint,
            stage: Stage::Hello,
            heartbeat_ms: 40_000,
            timeout_ms: 120_000,
            user: None,
        })
    }
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn decrypt(&self, encoded: &str) -> Result<Zeroizing<Vec<u8>>> {
        if encoded.len() > 512 {
            bail!("Invalid encrypted sign-in response");
        }
        let ciphertext = STANDARD
            .decode(encoded)
            .map_err(|_| anyhow::anyhow!("Invalid encrypted sign-in response"))?;
        if ciphertext.len() != 256 {
            bail!("Invalid encrypted sign-in response");
        }
        let mut decrypt = Decrypter::new(&self.key)?;
        decrypt.set_rsa_padding(Padding::PKCS1_OAEP)?;
        decrypt.set_rsa_oaep_md(MessageDigest::sha256())?;
        decrypt.set_rsa_mgf1_md(MessageDigest::sha256())?;
        let mut plaintext = Zeroizing::new(vec![0; decrypt.decrypt_len(&ciphertext)?]);
        let length = decrypt
            .decrypt(&ciphertext, &mut plaintext)
            .map_err(|_| anyhow::anyhow!("Discord sign-in decryption failed"))?;
        plaintext.truncate(length);
        Ok(plaintext)
    }
    pub fn accept(&mut self, text: &str) -> Result<Effect> {
        if text.len() > 16_384 {
            bail!("Sign-in response exceeded its size limit");
        }
        let value: Value = serde_json::from_str(text)
            .map_err(|_| anyhow::anyhow!("Invalid Discord sign-in response"))?;
        let op = value["op"]
            .as_str()
            .context("Discord sign-in operation missing")?;
        match (op, self.stage) {
            ("cancel", _) => Ok(Effect::Cancelled),
            ("heartbeat_ack", stage) if stage != Stage::Hello => Ok(Effect::HeartbeatAck),
            ("hello", Stage::Hello) => {
                self.heartbeat_ms = value["heartbeat_interval"]
                    .as_u64()
                    .filter(|ms| (1_000..=60_000).contains(ms))
                    .context("Invalid sign-in heartbeat interval")?;
                self.timeout_ms = value["timeout_ms"]
                    .as_u64()
                    .unwrap_or(120_000)
                    .clamp(5_000, 180_000);
                self.stage = Stage::Nonce;
                Ok(Effect::Send(
                    json!({"op":"init","encoded_public_key":STANDARD.encode(self.key.public_key_to_der()?)}),
                ))
            }
            ("nonce_proof", Stage::Nonce) => {
                let encoded = value["encrypted_nonce"]
                    .as_str()
                    .context("Discord nonce missing")?;
                let nonce = self.decrypt(encoded)?;
                if nonce.is_empty() {
                    bail!("Empty Discord sign-in nonce");
                }
                self.stage = Stage::Fingerprint;
                Ok(Effect::Send(
                    json!({"op":"nonce_proof","nonce":URL_SAFE_NO_PAD.encode(nonce)}),
                ))
            }
            ("pending_remote_init", Stage::Fingerprint) => {
                let fingerprint = value["fingerprint"]
                    .as_str()
                    .context("Discord QR fingerprint missing")?;
                if fingerprint.len() != self.fingerprint.len()
                    || !openssl::memcmp::eq(fingerprint.as_bytes(), self.fingerprint.as_bytes())
                {
                    bail!("Discord QR key fingerprint mismatch");
                }
                self.stage = Stage::Scan;
                Ok(Effect::Code(format!(
                    "https://discord.com/ra/{}",
                    self.fingerprint
                )))
            }
            ("pending_ticket", Stage::Scan) => {
                let payload = self.decrypt(
                    value["encrypted_user_payload"]
                        .as_str()
                        .context("Discord identity missing")?,
                )?;
                let text = std::str::from_utf8(&payload)
                    .map_err(|_| anyhow::anyhow!("Invalid Discord identity"))?;
                let parts: Vec<_> = text.splitn(4, ':').collect();
                if parts.len() != 4
                    || !snowflake(parts[0])
                    || parts[3].is_empty()
                    || parts[3].len() > 128
                {
                    bail!("Invalid Discord identity");
                }
                let user = Identity {
                    id: parts[0].into(),
                    username: crate::domain::safe_text(parts[3]),
                };
                self.user = Some(user.clone());
                self.stage = Stage::Approval;
                Ok(Effect::Scanned(user))
            }
            ("pending_login", Stage::Approval) => {
                let ticket = value["ticket"]
                    .as_str()
                    .filter(|t| !t.is_empty() && t.len() <= 4096)
                    .context("Discord login ticket missing")?;
                self.stage = Stage::Exchange;
                Ok(Effect::Exchange(Zeroizing::new(ticket.into())))
            }
            _ => bail!("Unexpected Discord sign-in step"),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use openssl::{
        encrypt::Encrypter,
        pkey::{PKey, Public},
    };
    pub fn encrypt(key: &PKey<Public>, data: &[u8]) -> String {
        let mut encrypt = Encrypter::new(key).unwrap();
        encrypt.set_rsa_padding(Padding::PKCS1_OAEP).unwrap();
        encrypt.set_rsa_oaep_md(MessageDigest::sha256()).unwrap();
        encrypt.set_rsa_mgf1_md(MessageDigest::sha256()).unwrap();
        let mut buffer = vec![0; encrypt.encrypt_len(data).unwrap()];
        let len = encrypt.encrypt(data, &mut buffer).unwrap();
        buffer.truncate(len);
        STANDARD.encode(buffer)
    }
    pub fn initialize(protocol: &mut Protocol) -> PKey<Public> {
        let Effect::Send(init) = protocol
            .accept(r#"{"op":"hello","heartbeat_interval":1000,"timeout_ms":30000}"#)
            .unwrap()
        else {
            panic!("Missing init");
        };
        let key = PKey::public_key_from_der(
            &STANDARD
                .decode(init["encoded_public_key"].as_str().unwrap())
                .unwrap(),
        )
        .unwrap();
        let Effect::Send(proof) = protocol
            .accept(
                &json!({"op":"nonce_proof","encrypted_nonce":encrypt(&key,b"fictional-nonce")})
                    .to_string(),
            )
            .unwrap()
        else {
            panic!("Missing proof");
        };
        assert_eq!(proof["nonce"], URL_SAFE_NO_PAD.encode(b"fictional-nonce"));
        key
    }
    #[test]
    fn rsa_nonce_fingerprint_identity_and_ticket_form_an_ordered_exchange() {
        let mut protocol = Protocol::new().unwrap();
        let key = initialize(&mut protocol);
        let fingerprint = URL_SAFE_NO_PAD.encode(Sha256::digest(key.public_key_to_der().unwrap()));
        let Effect::Code(code) = protocol
            .accept(&json!({"op":"pending_remote_init","fingerprint":fingerprint}).to_string())
            .unwrap()
        else {
            panic!("Missing code");
        };
        assert_eq!(code, format!("https://discord.com/ra/{fingerprint}"));
        let payload = encrypt(&key, b"123456789:0:avatar:Fictional user");
        let Effect::Scanned(user) = protocol
            .accept(&json!({"op":"pending_ticket","encrypted_user_payload":payload}).to_string())
            .unwrap()
        else {
            panic!("Missing identity");
        };
        assert_eq!(user.id, "123456789");
        assert_eq!(user.username, "Fictional user");
        let Effect::Exchange(ticket) = protocol
            .accept(r#"{"op":"pending_login","ticket":"fictional-ticket"}"#)
            .unwrap()
        else {
            panic!("Missing ticket");
        };
        assert_eq!(ticket.as_str(), "fictional-ticket");
        let token = protocol
            .decrypt(&encrypt(&key, b"fictional-session"))
            .unwrap();
        assert_eq!(&*token, b"fictional-session");
    }
    #[test]
    fn mismatched_short_fingerprints_wrong_keys_and_out_of_order_steps_are_rejected() {
        let mut protocol = Protocol::new().unwrap();
        assert!(
            protocol
                .accept(r#"{"op":"pending_login","ticket":"never-accept"}"#)
                .is_err()
        );
        let key = initialize(&mut protocol);
        for fingerprint in ["short", &"x".repeat(43)] {
            assert!(
                protocol
                    .accept(
                        &json!({"op":"pending_remote_init","fingerprint":fingerprint}).to_string()
                    )
                    .is_err()
            );
        }
        let other = PKey::from_rsa(Rsa::generate(2048).unwrap()).unwrap();
        let other = PKey::public_key_from_der(&other.public_key_to_der().unwrap()).unwrap();
        assert!(protocol.decrypt(&encrypt(&other, b"wrong-key")).is_err());
        assert!(protocol.decrypt("bad-base64").is_err());
        assert!(protocol.accept(&"x".repeat(16_385)).is_err());
        assert!(protocol.decrypt(&encrypt(&key, b"correct-key")).is_ok());
    }
    #[test]
    fn server_cancel_and_heartbeat_do_not_advance_identity_and_untrusted_names_are_safe() {
        let mut protocol = Protocol::new().unwrap();
        let key = initialize(&mut protocol);
        assert!(matches!(
            protocol.accept(r#"{"op":"heartbeat_ack"}"#).unwrap(),
            Effect::HeartbeatAck
        ));
        protocol
            .accept(
                &json!({"op":"pending_remote_init","fingerprint":protocol.fingerprint()})
                    .to_string(),
            )
            .unwrap();
        let Effect::Scanned(user)=protocol.accept(&json!({"op":"pending_ticket","encrypted_user_payload":encrypt(&key,b"123456789:0:avatar:Name\x1b[31m")}).to_string()).unwrap() else {panic!("Missing identity");};
        assert_eq!(user.username, "Name");
        assert!(matches!(
            protocol.accept(r#"{"op":"cancel"}"#).unwrap(),
            Effect::Cancelled
        ));
    }
}
