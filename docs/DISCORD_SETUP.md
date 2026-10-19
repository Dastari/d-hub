# Personal Discord QR setup

The owner selected Concord-style personal QR sign-in on 18 October 2026. Version 0.3.0 implements authentication, new personal/group DM reception and text replies in the shared service. Sign-in saves a credential; enabling the account starts reception, and allow_send separately permits replies. Live owner transport acceptance is still a setup check.

Discord's QR flow is approved from the official mobile app. Its [QR login FAQ](https://support.discord.com/hc/en-us/articles/360039213771-QR-Code-Login-FAQ) explains scanning and approval. d-hub is an unofficial personal client, and [Discord's automated-account policy](https://support.discord.com/hc/en-us/articles/115002192352-Automated-User-Accounts-Self-Bots) remains an account risk. No CAPTCHA/MFA bypass is implemented.

## Start and sign in

Build/update the executable, then launch:

```sh
cargo build --release --locked
./target/release/d-hub
```

The default launch starts the live background service when missing and opens the drawer. On an empty profile it opens account setup. Existing services are reused; closing the drawer leaves the owner running. No systemd unit or desktop configuration is installed.

From settings, press `:` and enter these commands:

```text
account add discord personal
auth personal
```

A personal account is the default Discord mode. If it already exists, use `:auth personal` directly. Standalone setup and equivalent CLI entry points are also available:

```sh
d-hub settings
d-hub account add discord personal
d-hub account auth personal
# For metadata already owned by the running service:
d-hub account --socket "$XDG_RUNTIME_DIR/d-hub/service-live.sock" auth personal
```

Standalone settings/metadata commands seed a new service profile. Once a live service exists, use its drawer/settings or the `--socket` convenience commands.

Install libsecret's `secret-tool` and unlock your desktop keyring first (Arch package: `libsecret`). The screen reports a missing helper or keyring failure; it never writes a session to a file as a fallback. XDG_RUNTIME_DIR is required for the per-account sign-in lock.

1. The terminal displays a black-and-white QR code with a quiet zone. In the Discord mobile app, use **Scan QR Code** and scan the code generated in your own terminal.
2. Approve the sign-in on your phone. d-hub verifies the encrypted identity and the session's current user.
3. Check the displayed username/user ID. Press **Enter** to store this identity in the keyring.
4. Close the success screen. Run `:account enable personal` (using your account label), then `:connections`. Wait for **Discord DMs connected** before asking someone to send a new test DM. Existing QR credentials are reused; reauthenticate only if status requests it.
5. To permit replies, set `accounts.personal.allow_send` to true in settings. Replies remain off by default. Shift+Enter or Ctrl+J adds a newline; Enter or F5 submits a reply to the captured conversation. Accepted replies dismiss their original incoming message; uncertain results retain the draft and require checking Discord before an explicit resolution.

**Esc** cancels before saving. Expired, cancelled or failed sessions clear the QR; **r** creates a fresh session/key/code. A keyring save already explicitly started must finish before closing. The code is never cropped or scaled to fit a terminal: resize if the screen reports insufficient room. Unicode half-block rendering requires a normal monospace terminal; actual phone-camera/Ghostty acceptance remains unverified.

The verified user ID is stored as `accounts.ID.discord_user_id` in the shared typed settings; the session token is stored only under a separate personal Discord credential reference. Reauthentication rejects a different user for an already pinned account. Add a new d-hub account ID for another Discord user. Changing profiles, opening a reader or receiving mail during setup cannot modify the draft target.

Do not use `--secret-stdin` or `--json` for personal login. Username/password and pasted-token login are not implemented in this slice. CAPTCHA or additional verification produces a visible error; complete required checks through Discord's official client, then retry. Unofficial endpoint/header requirements may change.

## Launch behavior and troubleshooting

`d-hub` and `d-hub drawer` automatically start the default live service. Data defaults to `$XDG_STATE_HOME/d-hub/live` or `~/.local/state/d-hub/live`; the socket is `$XDG_RUNTIME_DIR/d-hub/service-live.sock`. A startup lock prevents duplicate service launches. The process uses argv arrays and detached standard streams; startup errors are returned to the terminal. Runtime state remains private.

Use `d-hub drawer --no-start-service` for connect-only behavior. An explicit custom `--socket` also remains connect-only, since it may point to a demo or separately managed service. Noninteractive invocations do not start a hidden service. `d-hub service stop` stops the background owner; the next normal launch restarts it.

If an older daemon is already running, the QR setup checks its capability before storing metadata. Update the binary, run `d-hub service stop`, then relaunch `d-hub` to use the new setup. Existing drafts and service settings are retained. No owner sign-in or Discord communications were accessed by development tests.

## Protocol and implementation provenance

Concord's QR implementation was inspected at `0bdb321e7b8c9543fa2063035d17ac3b70ef2c25` as a protocol reference. d-hub independently implements a strict handshake state machine with OpenSSL RSA-OAEP-SHA256, a blocking bounded WebSocket worker, fixed Discord endpoints, verified user identity and an explicit local keyring-save step. No Concord source or dependency was imported; its GPL license has not been applied to this repository.

The remote-auth v2 endpoint is `wss://remote-auth-gateway.discord.gg/?v=2`. A generated public key's SHA-256 fingerprint binds the mobile QR URI to this session. Ticket exchange uses Discord's encrypted-session endpoint; `GET /users/@me` verifies the approved user before storage. TLS certificate validation stays enabled and HTTP redirects are disabled. Private keys and plaintext session buffers are short-lived; session state is absent from persistent application snapshots and IPC.

Tests use fake RSA exchanges, synthetic identities, loopback WebSocket/HTTP servers, a memory keyring and a QR decoder reading the actual rendered half-blocks. They establish protocol translation and recovery behavior, not live Discord compatibility or mobile-camera acceptance.

## Reception, replies and current limits

For an existing signed-in account, update with :update, then use :account enable personal in settings/navigation. From the composer press Escape before :. Account IDs are local labels. New DM arrival order comes from the daemon's receipt time and stable local IDs; duplicated/resumed events do not create duplicate feed rows or reset an existing arrival's time. Own outgoing messages and guild/channel traffic are excluded. The Gateway maintains heartbeats while REST requests run, verifies the READY user against the QR identity, reconnects/resumes temporary interruptions and uses a bounded queue with replay on overflow. Sign-out, permission, identity and rate-limit failures are visible through :connections. :account disable personal stops reception; it does not erase existing messages/drafts.

Equivalent shared-service configuration (replace personal with your label):

```sh
d-hub config --socket "$XDG_RUNTIME_DIR/d-hub/service-live.sock" set accounts.personal.enabled true
d-hub config --socket "$XDG_RUNTIME_DIR/d-hub/service-live.sock" set accounts.personal.allow_send true
```

The first command enables receiving; the second independently permits sending. No settings or connections are enabled automatically by an update. Enabling an unsigned account reports the required QR setup before accessing the keyring or network. Edits update captured content; deletions remove reply authority and cannot redirect a draft. Group-DM membership is rechecked before replying; a changed recipient set retains the original draft. Replies disable automatic mention parsing/pings and use a stable operation nonce without automatic retry of an ambiguous send.

This slice receives new DMs while the background owner is connected. Messages sent before enablement, during a full daemon stop/update, or after an expired non-resumable session are not fetched retrospectively. A temporary resumable Gateway disconnect can replay missed events; a fresh session cannot recover them yet. No startup-wide history download is performed. Read dots remain local; Discord read acknowledgements, on-demand history/offline catch-up, full client navigation and attachment transfers remain pending. Existing received messages persist in the private local cache. Personal-client protocol compatibility must still be checked on the owner's real account; automated tests use fake servers/credentials only.

Gateway/REST protocol facts were checked against Discord's [Gateway lifecycle](https://docs.discord.com/developers/events/gateway), [close codes](https://docs.discord.com/developers/topics/opcodes-and-status-codes), [message resource](https://docs.discord.com/developers/resources/message) and [private channel types](https://docs.discord.com/developers/resources/channel). Those documents describe Discord applications; they do not make the owner-selected personal route officially supported. Concord's pinned Gateway reference was inspected for personal v9 payload facts only. No Concord code or dependency is included. Uncompressed JSON, bounded message sizes, validated TLS and fixed Discord origins are used; resume URLs cannot redirect credentials to foreign hosts.
