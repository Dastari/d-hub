# Use real accounts

The live preview supports iCloud mail, Microsoft mail and selected Teams chats. Live profiles start empty and never load fixtures. The daemon owns provider sessions, sends, read synchronization and optional AI filtering; drawers/readers share it. Real account authentication and a first test reply are owner setup steps. Development tests use fictional mail and local fake servers.

Build with a current stable Rust toolchain and system OpenSSL development libraries:

```sh
cargo build --release --locked
install -Dm755 target/release/d-hub "$HOME/.local/bin/d-hub"
```

Install your distribution's libsecret helper if needed (`secret-tool`; Arch package `libsecret`) and unlock your desktop keyring. Passwords and refresh tokens go through Secret Service, never TOML or command arguments. Configure metadata before launching the daemon; after initialization its database owns settings.

## iCloud

Use your full iCloud email address and an Apple app-specific password. These example addresses are placeholders:

```sh
d-hub account add icloud personal
d-hub config set accounts.personal.username YOUR_ICLOUD_EMAIL
d-hub account auth personal
d-hub config set accounts.personal.enabled true
```

Authentication prompts privately. Apple documents IMAP TLS on port 993 and SMTP STARTTLS on port 587. The implementation fixes the hosts to Apple's published servers; certificate validation stays enabled. See [Apple server settings](https://support.apple.com/en-us/102525).

By default only INBOX is watched. Set `accounts.personal.resources` to newline-separated mailbox names for additional folders. Initial sync imports the latest 50 messages per mailbox. Subsequent polls walk new UIDs in bounded batches, retain UIDVALIDITY, import flags for the latest 200 messages and detect removal of previously tracked UIDs. UIDVALIDITY changes invalidate the original remote identities; they cannot redirect a retained draft. Bodies use PEEK, so downloading does not mark mail read. Cross-mailbox IMAP move/copy aliases are not yet tracked; watching several folders can produce a separate row for a moved/copied message.

SMTP replies use the captured recipient set, From identity, In-Reply-To and References. Only explicit sends submit mail. SMTP acceptance is not delivery confirmation. Automatic Sent-folder APPEND is deliberately absent until the owner's account verifies whether iCloud saves sent copies; the current build does not promise a Sent copy.

## Microsoft mail and Teams

Register a public-client application in Microsoft Entra with device flow enabled. Tenant policy may require administrator consent. Configure separate account entries for mail and Teams, even when they share a login.

```sh
d-hub account add office365 work
d-hub config set accounts.work.username YOUR_MICROSOFT_EMAIL_OR_UPN
d-hub config set accounts.work.client_id YOUR_PUBLIC_CLIENT_ID
d-hub config set accounts.work.tenant YOUR_TENANT_ID_OR_DOMAIN
d-hub account auth work
d-hub config set accounts.work.enabled true
```

Delegated mail scopes are User.Read, Mail.ReadWrite, Mail.Send and offline_access. Token refresh verifies `/me` against the configured email/UPN before accessing communications, and rotates refresh credentials through the keyring. Existing authentication from earlier prototypes may need renewed consent for User.Read.

Mail defaults to Inbox. `resources` accepts newline-separated Graph folder IDs or supported well-known names. Initial delta sync covers seven days; opaque next/delta links are persisted with the imported state. Invalid delta cursors restart that window with identity deduplication. Mail uses immutable IDs, remote isRead updates, text/HTML conversion and the reply endpoint. Replies explicitly carry the frozen recipients and text body. Graph saves accepted replies to Sent Items. [Delta contract](https://learn.microsoft.com/en-us/graph/api/message-delta?view=graph-rest-1.0), [reply contract](https://learn.microsoft.com/en-us/graph/api/message-reply?view=graph-rest-1.0).

```sh
d-hub account add teams work-chat
d-hub config set accounts.work-chat.username YOUR_MICROSOFT_EMAIL_OR_UPN
d-hub config set accounts.work-chat.client_id YOUR_PUBLIC_CLIENT_ID
d-hub config set accounts.work-chat.tenant YOUR_TENANT_ID_OR_DOMAIN
d-hub config set accounts.work-chat.resources YOUR_TEAMS_CHAT_ID
d-hub account auth work-chat
d-hub config set accounts.work-chat.enabled true
```

Teams uses delegated User.Read, Chat.ReadWrite and offline_access. This slice supports explicit chat IDs, including one-to-one/group chats; it does not discover all chats or support channels. Supply multiple IDs on separate lines using `config set …resources --file PATH` or the TUI. Initial context is the latest 50 messages per chat; later polls page changes since the saved modification timestamp. Own and system messages are excluded. Read dots/dismissals are local: Teams client unread state is not synchronized. Replies send plain text to the captured chat. [Chat listing contract](https://learn.microsoft.com/en-us/graph/api/chat-list-messages?view=graph-rest-1.0), [chat send contract](https://learn.microsoft.com/en-us/graph/api/chatmessage-post?view=graph-rest-1.0).

## Start the service and drawer

On your desktop, XDG_RUNTIME_DIR must be set and private. No service is enabled automatically:

```sh
d-hub --config "$HOME/.config/d-hub/config.toml" daemon --live --foreground --data-dir "$HOME/.local/state/d-hub/live"
```

In another terminal:

```sh
d-hub drawer
d-hub service status --json
```

Running `d-hub` without a subcommand opens the live drawer. The default live socket is `$XDG_RUNTIME_DIR/d-hub/service-live.sock`; pass `--socket PATH` to drawer/service/config/account/profile/ai-auth for another private socket. `daemon --demo` defaults to service-demo.sock; existing examples with explicit demo sockets still work.

Use a fresh live directory. Live mode refuses demo databases and legacy fixture JSON. A per-user ownership lock prevents two live profiles from starting provider workers simultaneously. Closing the drawer leaves the foreground daemon running; `service stop` shuts it down. A restart preserves drafts, local dismissals, sync checkpoints and outbox operations.

For initial inspection, leave sending disabled. Once you have checked account identity and the displayed reply recipient, enable it through the drawer's settings or CLI:

```sh
d-hub config --socket "$XDG_RUNTIME_DIR/d-hub/service-live.sock" set accounts.personal.allow_send true
```

F5/Ctrl+Enter/Alt+Enter sends the typed reply; Enter adds a newline. The header says LIVE and the footer says SEND. Up/Down saves and restores per-message drafts; Left/Right cycles sources; Alt+arrows moves the cursor. Escape enters navigation; `e` dismisses, `D` includes dismissed, `u` toggles unread, `f` cycles Work/Home/custom profiles, `,` opens settings, and `:connections` shows account status. A shared reader never changes the drawer recipient.

Only perform a first real send to an explicitly chosen test recipient/conversation. Check remote read changes, delivery and Sent behavior in the original client. Automated tests do not establish live account or Ghostty acceptance.

## Settings, reconnect and send outcomes

All persistent account/profile/AI settings share TUI/CLI validation. Defaults: enabled=false, allow_send=false, poll_seconds=30 (range 15–3600), allow_ai=false. Active service settings must be changed through the drawer or `config --socket`; editing the seed TOML does not change them. Account/profile convenience commands also accept `--socket`:

```sh
d-hub account --socket "$XDG_RUNTIME_DIR/d-hub/service-live.sock" list
d-hub account --socket "$XDG_RUNTIME_DIR/d-hub/service-live.sock" auth personal
d-hub profile --socket "$XDG_RUNTIME_DIR/d-hub/service-live.sock" add family
d-hub service refresh
```

Missing credentials, rejected consent, offline servers and throttling remain visible. Polls retry with per-account backoff; Graph Retry-After applies across queued sync/read work. Other accounts and typing remain usable. `account forget` disables the selected account before removing its keyring credential when invoked against the active service. An already-submitted request may still finish; its outcome is retained.

Every send is committed before a worker can execute it. An interrupted or ambiguous operation stays uncertain and cannot retry automatically. Verify the outcome in the original client, then explicitly record what you found:

```sh
d-hub service snapshot --json
d-hub service resolve OPERATION_NUMBER accepted
# Only after confirming that it was NOT sent:
d-hub service resolve OPERATION_NUMBER not-sent
```

The drawer also supports `:resolve accepted` / `:resolve not-sent`. Accepted clears/dismisses only the captured incoming item. Not-sent retains the draft and unlocks deliberate retry. These are owner-reported outcomes, not automatic provider reconciliation.

Changing account identity/resources does not authorize old drafts or queued read changes against the replacement connection. Preserve the old account entry and add a new ID for a different login. On IPC loss the drawer freezes and saves private recovery data; restart the service and explicitly reopen/restore. Automatic client reconnect is still pending. Protocol 2 prevents older demo-only clients from connecting and mislabelling real sends; unsent protocol-1 recovery files remain explicitly importable with target/conflict checks.

## AI and current limits

Jev/OpenAI decision filtering runs once in the daemon. Store the key through `ai-auth --socket PATH` or `:auth ai`, configure provider/model/prompts, then enable both `ai.enabled` and `accounts.ID.allow_ai`. Bounded text only; attachments are excluded. Spam and focus prompts remain independent. AI failures leave local rules available and cannot modify or send drafts.

The active feed is a recent 200-item window. Pinned targets, selected items, saved drafts and pending read updates are protected from eviction. Normalized remote records and local dismissal state remain in the private database; a full archive browser/retention UI is pending. Cached body previews are limited to 16,000 bytes; oversized IMAP messages fetch headers only beyond one MiB. The original client is needed for complete oversized content and files.

Attachment names/availability are shown, but download, upload, clipboard images, reply-all, richer conversation history, notifications/sounds with Omarchy DND, automatic reconnect, systemd packaging and the Omarchy companion remain unfinished. Personal Discord DMs/replies remain an unresolved integration requirement; notification previews and bot channels do not satisfy it. The optional standalone notification bridge remains separate and read-only. No alerts or sounds are emitted by the new live adapters.
