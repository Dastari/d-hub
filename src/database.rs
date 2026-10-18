//! Transactional service snapshots. Provider-normalized tables can be added by migration.
use crate::{app::Session, composer::SendState, config::Settings, storage::DemoStore};
use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

pub const SCHEMA: u32 = 2;
const PROVIDER_SCHEMA:&str="CREATE TABLE profile (id INTEGER PRIMARY KEY CHECK(id=1), live INTEGER NOT NULL);
    CREATE TABLE remote_messages (id INTEGER PRIMARY KEY AUTOINCREMENT, identity TEXT NOT NULL UNIQUE, payload TEXT NOT NULL, local_state TEXT NOT NULL DEFAULT '{}', imported INTEGER NOT NULL DEFAULT 0);
    PRAGMA user_version=2;";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub revision: u64,
    pub kind: String,
}
pub struct Database {
    connection: Connection,
    pub live: bool,
    _owner: DemoStore,
}
impl Database {
    pub fn open(directory: &Path, initial: &Settings) -> Result<Self> {
        Self::open_mode(directory, initial, false)
    }
    pub fn open_mode(directory: &Path, initial: &Settings, live: bool) -> Result<Self> {
        initial.validate()?;
        let owner = DemoStore::open(directory.to_owned())?;
        let meta = fs::symlink_metadata(directory)?;
        if meta.file_type().is_symlink()
            || meta.uid() != nix::unistd::geteuid().as_raw()
            || meta.permissions().mode() & 0o077 != 0
        {
            bail!("service data directory must be owned by this user and private (0700)");
        }
        let path = directory.join("service.sqlite3");
        if let Ok(meta) = fs::symlink_metadata(&path)
            && meta.file_type().is_symlink()
        {
            bail!("database must not be a symlink");
        }
        OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .mode(0o600)
            .open(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let mut connection = Connection::open(&path)?;
        connection.busy_timeout(std::time::Duration::from_secs(2))?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > SCHEMA {
            bail!("unsupported service database schema {version}; database preserved");
        }
        let application: u32 =
            connection.pragma_query_value(None, "application_id", |r| r.get(0))?;
        if version != 0 && application != 0x44485542 {
            bail!("unrecognised database identity; database preserved");
        }
        if version == 1 && live {
            bail!(
                "existing demo database cannot become a live profile; choose a separate directory"
            );
        }
        if version == 0 {
            let tables: u32 = connection.query_row(
                "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                [],
                |r| r.get(0),
            )?;
            if tables != 0 {
                bail!("unrecognised existing database; no migration applied");
            }
            // Read and validate legacy input BEFORE migration; never replace the JSON backup.
            let old: Option<Session> = owner.load()?;
            if live && old.is_some() {
                bail!("live profiles cannot import demo JSON; choose a separate directory");
            }
            let legacy = old.unwrap_or_else(|| {
                if live {
                    Session {
                        messages: vec![],
                        ..Session::default()
                    }
                } else {
                    Session::default()
                }
            });
            let app = crate::app::App::new(legacy, initial.clone())?;
            let tx = connection.transaction()?;
            tx.execute_batch("CREATE TABLE state (id INTEGER PRIMARY KEY CHECK(id=1), revision INTEGER NOT NULL, session TEXT NOT NULL, settings TEXT NOT NULL);
                CREATE TABLE changes (revision INTEGER PRIMARY KEY, kind TEXT NOT NULL);
                CREATE TABLE outbox (operation INTEGER PRIMARY KEY, payload TEXT NOT NULL, status TEXT NOT NULL);
                PRAGMA application_id=0x44485542; PRAGMA user_version=1;")?;
            tx.execute_batch(PROVIDER_SCHEMA)?;
            tx.execute("INSERT INTO profile VALUES (1,?1)", [live])?;
            tx.execute(
                "INSERT INTO state VALUES (1, 1, ?1, ?2)",
                params![
                    serde_json::to_string(&app.session)?,
                    serde_json::to_string(initial)?
                ],
            )?;
            tx.execute("INSERT INTO changes VALUES (1, 'initialized')", [])?;
            sync_outbox(&tx, &app.session)?;
            tx.commit()?;
        }
        if version == 1 {
            let tx = connection.transaction()?;
            tx.execute_batch(PROVIDER_SCHEMA)?;
            tx.execute("INSERT INTO profile VALUES (1,0)", [])?;
            tx.commit()?;
        }
        let stored_live: bool =
            connection.query_row("SELECT live FROM profile WHERE id=1", [], |r| r.get(0))?;
        if stored_live != live {
            bail!("profile mode mismatch; live and demo require separate data directories");
        }
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        Ok(Self {
            connection,
            live,
            _owner: owner,
        })
    }
    pub fn imported(&self, id: crate::domain::MessageId) -> Result<bool> {
        Ok(self.connection.query_row(
            "SELECT imported FROM remote_messages WHERE id=?1",
            [id.0],
            |r| r.get(0),
        )?)
    }
    pub fn remote_id(
        &self,
        record: &crate::providers::Item,
    ) -> Result<Option<crate::domain::MessageId>> {
        let id: Option<u64> = self
            .connection
            .query_row(
                "SELECT id FROM remote_messages WHERE identity=?1",
                [serde_json::to_string(&(
                    &record.remote,
                    &record.account_scope,
                ))?],
                |r| r.get(0),
            )
            .optional()?;
        Ok(id.map(crate::domain::MessageId))
    }
    pub fn store_remote(
        &mut self,
        record: &crate::providers::Item,
    ) -> Result<crate::domain::MessageId> {
        let key = serde_json::to_string(&(&record.remote, &record.account_scope))?;
        let mut record = record.clone();
        // Chat edits, reactions and deletions arrive as partial updates of a captured message.
        let chat = matches!(
            record.remote,
            crate::read_state::RemoteMessage::DiscordDm { .. }
                | crate::read_state::RemoteMessage::GraphChat { .. }
        );
        if record.partial || chat {
            let previous: Option<String> = self
                .connection
                .query_row(
                    "SELECT payload FROM remote_messages WHERE identity=?1",
                    [&key],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(previous) = previous {
                let mut old: crate::providers::Item = serde_json::from_str(&previous)?;
                if record.partial {
                    old.unread = record.unread.or(old.unread);
                    if chat {
                        // Edits/deletions update captured content, preserving arrival time, read state and frozen recipients.
                        old.deleted |= record.deleted;
                        if !record.deleted {
                            old.body = record.body.clone();
                        }
                    }
                    record = old;
                } else if old.deleted {
                    // A replayed create cannot resurrect a deleted chat message.
                    record = old;
                }
            }
        }
        self.connection.execute("INSERT INTO remote_messages(identity,payload) VALUES (?1,?2) ON CONFLICT(identity) DO UPDATE SET payload=excluded.payload",
            params![key,serde_json::to_string(&record)?])?;
        let id = self.connection.query_row(
            "SELECT id FROM remote_messages WHERE identity=?1",
            [key],
            |r| r.get(0),
        )?;
        Ok(crate::domain::MessageId(id))
    }
    pub fn local_state(
        &self,
        id: crate::domain::MessageId,
    ) -> Result<crate::read_state::MessageState> {
        let value: String = self.connection.query_row(
            "SELECT local_state FROM remote_messages WHERE id=?1",
            [id.0],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&value)?)
    }
    pub fn remote(&self, id: crate::domain::MessageId) -> Result<crate::providers::Item> {
        let payload: String = self.connection.query_row(
            "SELECT payload FROM remote_messages WHERE id=?1",
            [id.0],
            |r| r.get(0),
        )?;
        Ok(serde_json::from_str(&payload)?)
    }
    pub fn load(&self) -> Result<(u64, Session, Settings)> {
        let (revision, session, settings): (u64, String, String) = self.connection.query_row(
            "SELECT revision,session,settings FROM state WHERE id=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        let session: Session =
            serde_json::from_str(&session).context("invalid stored service session")?;
        let settings: Settings =
            serde_json::from_str(&settings).context("invalid stored service settings")?;
        settings.validate()?;
        Ok((revision, session, settings))
    }
    pub fn commit(
        &mut self,
        expected: u64,
        session: &Session,
        settings: &Settings,
        kind: &str,
    ) -> Result<u64> {
        settings.validate()?;
        let next = expected
            .checked_add(1)
            .context("service revision exhausted")?;
        let tx = self.connection.transaction()?;
        let changed = tx.execute(
            "UPDATE state SET revision=?1, session=?2, settings=?3 WHERE id=1 AND revision=?4",
            params![
                next,
                serde_json::to_string(session)?,
                serde_json::to_string(settings)?,
                expected
            ],
        )?;
        if changed != 1 {
            bail!("stale database revision");
        }
        sync_outbox(&tx, session)?;
        for message in &session.messages {
            tx.execute(
                "UPDATE remote_messages SET imported=1 WHERE id=?1",
                [message.id.0],
            )?;
        }
        for (id, state) in &session.message_state {
            tx.execute(
                "UPDATE remote_messages SET local_state=?1 WHERE id=?2",
                params![serde_json::to_string(state)?, id.0],
            )?;
        }
        tx.execute("INSERT INTO changes VALUES (?1, ?2)", params![next, kind])?;
        tx.execute(
            "DELETE FROM changes WHERE revision < ?1",
            [next.saturating_sub(255)],
        )?;
        tx.commit()?;
        Ok(next)
    }
    pub fn changes(&self, after: u64) -> Result<Vec<Change>> {
        Ok(self
            .connection
            .prepare("SELECT revision,kind FROM changes WHERE revision > ?1 ORDER BY revision")?
            .query_map([after], |r| {
                Ok(Change {
                    revision: r.get(0)?,
                    kind: r.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }
    pub fn outbox_status(&self, operation: u64) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row(
                "SELECT status FROM outbox WHERE operation=?1",
                [operation],
                |r| r.get(0),
            )
            .optional()?)
    }
}
fn sync_outbox(tx: &rusqlite::Transaction<'_>, session: &Session) -> Result<()> {
    for sent in &session.composer.sent {
        tx.execute("INSERT INTO outbox VALUES (?1,?2,'accepted') ON CONFLICT(operation) DO UPDATE SET status='accepted'", params![sent.operation,serde_json::to_string(sent)?])?;
    }
    match &session.composer.send {
        SendState::Submitting(op) | SendState::Uncertain(op) => {
            let status = if matches!(session.composer.send, SendState::Submitting(_)) {
                "submitting"
            } else {
                "uncertain"
            };
            tx.execute("INSERT INTO outbox VALUES (?1,?2,?3) ON CONFLICT(operation) DO UPDATE SET status=excluded.status", params![op.operation,serde_json::to_string(op)?,status])?;
        }
        SendState::Failed(_) => {
            tx.execute(
                "UPDATE outbox SET status='failed' WHERE status IN ('submitting','uncertain')",
                [],
            )?;
        }
        _ => {}
    }
    Ok(())
}
