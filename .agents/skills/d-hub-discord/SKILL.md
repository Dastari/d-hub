---
name: d-hub-discord
description: Implement d-hub personal Discord QR setup, DM integration and explicit bot capabilities with private credentials and honest connection state.
---

# Discord integration

Read the Discord scope in [integration design](../../../docs/INTEGRATIONS.md). The owner requires personal DMs, especially messages from their son; channels are lower priority. Phase 1 is new arrivals in the shared feed plus quick replies. History is phase 2 (or 3), fetched on chat open and paginated on demand; do not require bulk history at startup.

## Owner-selected personal client route

On 18 October 2026 the owner explicitly requested personal-account setup with QR sign-in like Concord. This supersedes the earlier project restriction against implementing personal login. Continue the independent personal client implementation without asking the same scope question again. Authentication is implemented; DM sync/replies are still pending.

Discord's [self-bot policy](https://support.discord.com/hc/en-us/articles/115002192352-Automated-User-Accounts-Self-Bots) does not make this an officially supported integration; record its unofficial status honestly. Do not silently substitute bots or desktop notifications for personal DMs.

QR setup uses an ephemeral RSA-2048/OAEP-SHA256 key, nonce proof, key fingerprint verification, mobile approval, encrypted ticket exchange and verified user identity. The UI shows the identity before explicit keyring save. Keep expiry/cancel/retry deterministic; clear stale codes. Credentials, private keys, tickets and QR session state must never enter TOML, SQLite snapshots, recovery files, logs or stdout. Never extract a token from a desktop/browser client, accept personal tokens through argv, bypass CAPTCHA/MFA or fall back to plaintext storage. Separate personal credential references from bot/mail references and pin the verified user ID.

One explicit sign-in owner per account; closing/cancelling before save prevents storing a session. Once an explicit keyring save begins, report its completion/failure honestly. A successful login does not imply a connected DM transport, read sync or sending capability. Account/mode/identity changes must not reuse stale results or redirect drafts.

## Messages and capabilities

Use [Discord developer docs](https://discord.com/developers/docs/intro) for documented resources and verify the chosen personal-client protocol independently. Map events/history into stable account/conversation/message IDs. Handle edits/deletions, reconnect/resume, duplicates, pagination and rate limits. Keep remote unread semantics separate from local seen state. Historical backfill never emits fresh alerts.

For bot mode establish identity, access, intents, message-content eligibility and permissions. Messages sent as a bot must never appear to come from the human owner. Bot capability does not complete personal DM acceptance.

Reply relationships and recipients come from the frozen target. Persist the outbox before writes and treat ambiguous acceptance as uncertain. Use distinct capability reporting for read, reply and upload.

## Verification and provenance

Use fictional cryptographic exchanges, loopback fake WebSocket/HTTP servers and an in-memory credential store. Verify nonce/fingerprint/order checks, identity mismatch, cancellation, expiry, heartbeat loss, keyring failures, QR decoding and draft preservation. Actual owner scanning/sign-in and real messages/sends remain explicit setup/testing steps.

Concord is a GPL-3.0 protocol/reference project; no source is copied or linked into d-hub. Reusing its code requires a separate license decision. Independent protocol facts do not establish official support or live acceptance.
