# Use real accounts

The live preview supports iCloud mail, Microsoft mail and selected Teams chats. Live profiles start empty and never load fixtures. The daemon owns provider sessions, sends, read synchronization and optional AI filtering; drawers/readers share it. Real account authentication and a first test reply are owner setup steps. Development tests use fictional mail and local fake servers.

Build with a current stable Rust toolchain and system OpenSSL development libraries:

```sh
cargo build --release --locked
install -Dm755 target/release/d-hub "$HOME/.local/bin/d-hub"
```

Install your distribution's libsecret helper if needed (`secret-tool`; Arch package `libsecret`) and unlock your desktop keyring. Passwords and refresh tokens go through Secret Service, never TOML or command arguments. Once the live service has started, its database owns settings. `config`, `account` and `profile` commands then talk to the running service automatically. While it is stopped they refuse rather than edit a file it will never read. `--config PATH` edits only a first-run seed file. The drawer's Settings › Accounts pages offer the same steps.

Every non-Discord account has a read-only connection check: `d-hub account test ID` on the command line, or **Test connection** on its account page. It signs in, lists folders or Teams chats, and runs the exact first sync query, without importing, marking read or sending. Run it before enabling an account; failures show the server's own reason (for example an AADSTS code or IMAP response).

## iCloud

Use the iCloud Mail address itself (ending @icloud.com, @me.com or @mac.com) as the username, not an Apple ID that belongs to another provider such as Gmail. The same address is used for IMAP, SMTP and From, so a custom-domain alias does not work yet. Create an app-specific password at account.apple.com › Sign-In and Security. These example addresses are placeholders:

```sh
d-hub account add icloud personal
d-hub config set accounts.personal.username YOUR_ICLOUD_EMAIL
d-hub account auth personal
d-hub account test personal
d-hub config set accounts.personal.enabled true
```

The check signs in to IMAP, lists your folders (use these exact names in `resources`), examines the synced folders read-only and signs in to SMTP without sending.

Authentication prompts privately. Apple documents IMAP TLS on port 993 and SMTP STARTTLS on port 587. The implementation fixes the hosts to Apple's published servers; certificate validation stays enabled. See [Apple server settings](https://support.apple.com/en-us/102525).

By default only INBOX is watched. Set **Folders** on the account page (or `accounts.personal.resources`) to newline-separated mailbox names for additional folders. The account worker keeps one signed-in IMAP connection and reuses it for polls, read changes and the pre-send check; it signs in again only when the server closes the connection or the username or password changes. Initial sync imports the latest 50 messages per mailbox. Subsequent polls walk new UIDs in bounded batches, retain UIDVALIDITY, import flags for the latest 200 messages and detect removal of previously tracked UIDs. UIDVALIDITY changes invalidate the original remote identities; they cannot redirect a retained draft. Bodies use PEEK, so downloading does not mark mail read. Cross-mailbox IMAP move/copy aliases are not yet tracked; watching several folders can produce a separate row for a moved/copied message.

SMTP replies use the captured recipient set, From identity, In-Reply-To and References. Only explicit sends submit mail. SMTP acceptance is not delivery confirmation. Automatic Sent-folder APPEND is deliberately absent until the owner's account verifies whether iCloud saves sent copies; the current build does not promise a Sent copy.

## Microsoft mail and Teams

New Office 365 and Teams accounts start with the owner's work app registration: client ID `67441acc-d050-453d-bfe3-cb6234d1efb1` in tenant `afa5c75a-9ba4-40d3-849e-b36efb2fc71f` (firstassist.com.au). That registration is single-tenant, so it signs in only accounts in that directory. Accounts created before v0.4.1 keep their saved values; set `client_id` and `tenant` to the values above.

The registration must have:

1. Authentication › Advanced settings › **Allow public client flows: Yes**. Without it device login fails with AADSTS7000218.
2. API permissions › Microsoft Graph › Delegated: User.Read, Mail.ReadWrite, Mail.Send, Chat.ReadWrite (offline_access is requested automatically). These are additional to any application roles the registration already holds. If the tenant blocks user consent, an administrator must choose *Grant admin consent*, otherwise sign-in reports AADSTS65001.

For another directory or a personal account, register an app in [Microsoft Entra](https://entra.microsoft.com) with the settings above. Choose *Accounts in this organizational directory only* for work or school, or the option that includes personal Microsoft accounts for Outlook.com; no redirect URI is needed. Then set `client_id` to its Application (client) ID and `tenant` to match: the Directory (tenant) ID or domain for a single-tenant app (`organizations` fails with AADSTS50194), `consumers` (or `common` for an app that allows both) for personal accounts. **Teams chats require a work or school account**: Microsoft Graph's chat APIs do not support personal accounts.

Configure separate account entries for mail and Teams, even when they share a login; each signs in once.

```sh
d-hub account add office365 work
d-hub config set accounts.work.username YOUR_MICROSOFT_EMAIL_OR_UPN
d-hub account auth work
d-hub account test work
d-hub config set accounts.work.enabled true
```

`account auth` prints a microsoft.com/devicelogin address and code (the TUI shows them in the notice line). The check confirms the signed-in identity and granted scopes, lists folders with unread counts, and runs the seven-day delta query that the first sync uses.

Delegated mail scopes are User.Read, Mail.ReadWrite, Mail.Send and offline_access. Token refresh verifies `/me` against the configured email/UPN before accessing communications, and rotates refresh credentials through the keyring. Existing authentication from earlier prototypes may need renewed consent for User.Read.

Mail defaults to Inbox. **Folders** on the account page (`resources`) accepts newline-separated well-known names such as `inbox` or `archive`, or the folder IDs the check lists after each folder name; Graph does not accept display names. Initial delta sync covers seven days; opaque next/delta links are persisted with the imported state. Invalid delta cursors restart that window with identity deduplication. Mail uses immutable IDs, remote isRead updates and text/HTML conversion. A reply asks Outlook for its reply draft (createReply), puts the text above Outlook's quoted original, replaces the draft's recipients with the frozen ones and sends that draft. If the draft cannot be created or updated nothing is sent, the draft is deleted and the composer keeps the text; only the final send can be uncertain. Graph saves accepted replies to Sent Items. [Delta contract](https://learn.microsoft.com/en-us/graph/api/message-delta?view=graph-rest-1.0), [createReply contract](https://learn.microsoft.com/en-us/graph/api/message-createreply?view=graph-rest-1.0), [send contract](https://learn.microsoft.com/en-us/graph/api/message-send?view=graph-rest-1.0).

```sh
d-hub account add teams work-chat
d-hub config set accounts.work-chat.username YOUR_MICROSOFT_EMAIL_OR_UPN
d-hub config set accounts.work-chat.client_id YOUR_PUBLIC_CLIENT_ID
d-hub config set accounts.work-chat.tenant YOUR_TENANT_ID_OR_DOMAIN
d-hub account auth work-chat
d-hub account test work-chat
d-hub config set accounts.work-chat.resources YOUR_TEAMS_CHAT_ID
d-hub account test work-chat
d-hub config set accounts.work-chat.enabled true
```

The first check lists your 50 most recent chats as `people or topic · type · ID`; copy the IDs to sync. IDs pasted from Teams web addresses (`19%3A…%40thread.v2`) are decoded automatically. The second check reads each selected chat with the same filtered query sync uses. Teams uses delegated User.Read, Chat.ReadWrite and offline_access. This slice syncs the chat IDs you choose, including one-to-one/group chats; it does not follow every chat automatically or support channels. Supply multiple IDs on separate lines using `config set …resources --file PATH` or the TUI. Initial context is up to the latest 50 changed messages per chat from the last week; later polls page changes since the saved modification timestamp. A change to a message created before the poll began (an edit, reaction or deletion) updates that message in place: it never arrives as new, and a reaction to an old message never offered is ignored. Quoted replies show the quoted sender and preview as a `>` line; shared files, cards and images show a short marker. Own and system messages are excluded. Read dots/dismissals are local: Teams client unread state is not synchronized. Replies send plain text to the captured chat. [Chat listing contract](https://learn.microsoft.com/en-us/graph/api/chat-list-messages?view=graph-rest-1.0), [chat send contract](https://learn.microsoft.com/en-us/graph/api/chatmessage-post?view=graph-rest-1.0).

## Start the service and drawer

On your desktop, XDG_RUNTIME_DIR must be set and private. `d-hub` now starts its missing default live service automatically; it does not install a systemd unit. For explicit foreground operation:

```sh
d-hub --config "$HOME/.config/d-hub/config.toml" daemon --live --foreground --data-dir "$HOME/.local/state/d-hub/live"
```

In another terminal:

```sh
d-hub drawer
d-hub service status --json
```

Running `d-hub` without a subcommand starts/reuses the live service and opens the drawer. An empty profile opens account setup. The default live socket is `$XDG_RUNTIME_DIR/d-hub/service-live.sock`; pass `--socket PATH` to drawer/service/config/account/profile/ai-auth for another private socket. `daemon --demo` defaults to service-demo.sock; existing examples with explicit demo sockets still work.

Use a fresh live directory. Live mode refuses demo databases and legacy fixture JSON. A per-user ownership lock prevents two live profiles from starting provider workers simultaneously. Closing the drawer leaves the foreground daemon running; `service stop` shuts it down. A restart preserves drafts, local dismissals, sync checkpoints and outbox operations.

For initial inspection, leave sending disabled. Once you have checked account identity and the displayed reply recipient, enable it through the drawer's settings or CLI:

```sh
d-hub config --socket "$XDG_RUNTIME_DIR/d-hub/service-live.sock" set accounts.personal.allow_send true
```

Enter (or F5) sends the typed reply; Shift+Enter, Ctrl+Enter, Alt+Enter or Ctrl+J adds a newline. The footer shows ^J instead of ⇧⏎ when the terminal cannot report Shift+Enter. The header says LIVE and the footer says SEND. Up/Down saves and restores per-message drafts; Left/Right cycles sources; Alt+arrows moves the cursor. Delete dismisses and marks read while no reply is started; PageUp/PageDown scroll the preview. Escape enters navigation; `e` dismisses, `D` includes dismissed, `u` toggles unread, `f` cycles Work/Home/custom profiles, `,` opens settings, and `:connections` shows account status. A shared reader never changes the drawer recipient.

Only perform a first real send to an explicitly chosen test recipient/conversation. Check remote read changes, delivery and Sent behavior in the original client. Automated tests do not establish live account or Ghostty acceptance.

## Settings, reconnect and send outcomes

All persistent account/profile/AI settings share TUI/CLI validation. Defaults: enabled=false, allow_send=false, poll_seconds=30 (range 15–3600), allow_ai=false. Settings commands use the running live service by default; `--socket` selects another service explicitly:

```sh
d-hub account --socket "$XDG_RUNTIME_DIR/d-hub/service-live.sock" list
d-hub account --socket "$XDG_RUNTIME_DIR/d-hub/service-live.sock" auth personal
d-hub profile --socket "$XDG_RUNTIME_DIR/d-hub/service-live.sock" add family
d-hub service refresh
```

Missing credentials, rejected consent, offline servers and throttling remain visible. Polls retry with per-account backoff; Graph Retry-After applies across queued sync/read work. Other accounts and typing remain usable. `account forget` disables the selected account before removing its keyring credential when invoked against the active service. An already-submitted request may still finish; its outcome is retained.

Every send is committed before a worker can execute it. A send that never reaches the server (DNS, refused connection, TLS handshake) fails with the draft retained, ready to retry. An interrupted or ambiguous operation stays uncertain and cannot retry automatically. Verify the outcome in the original client, then explicitly record what you found:

```sh
d-hub service snapshot --json
d-hub service resolve OPERATION_NUMBER accepted
# Only after confirming that it was NOT sent:
d-hub service resolve OPERATION_NUMBER not-sent
```

The drawer also supports `:resolve accepted` / `:resolve not-sent`. Accepted dismisses the captured incoming item and the older messages stacked with it. Not-sent retains the draft and unlocks deliberate retry. These are owner-reported outcomes, not automatic provider reconciliation.

Changing account identity/resources does not authorize old drafts or queued read changes against the replacement connection. Preserve the old account entry and add a new ID for a different login. On IPC loss the drawer freezes and saves private recovery data; restart the service and explicitly reopen/restore. Automatic client reconnect is still pending. Protocol 2 prevents older demo-only clients from connecting and mislabelling real sends; unsent protocol-1 recovery files remain explicitly importable with target/conflict checks.

## AI and current limits

Jev/OpenAI decision filtering runs once in the daemon. Store the key through `ai-auth --socket PATH` or `:auth ai`, configure provider/model/prompts, then enable both `ai.enabled` and `accounts.ID.allow_ai`. Bounded text only; attachments are excluded. Spam and focus prompts remain independent. With `spam.for_you` on, each request also asks whether a person wrote the message and whether it is meant for the owner or a group they are in (see [Integrations](INTEGRATIONS.md#typed-decisions-and-summaries)). No-reply and automated mail is hidden locally without any request. AI failures leave local rules available and cannot modify or send drafts.

The active feed is a recent 200-item window. Pinned targets, selected items, saved drafts and pending read updates are protected from eviction. Normalized remote records and local dismissal state remain in the private database; a full archive browser/retention UI is pending. Cached body previews are limited to 16,000 bytes; oversized IMAP messages fetch headers only beyond one MiB. The original client is needed for complete oversized content and files.

Attachment names/availability are shown, but download, upload, clipboard images, reply-all, richer conversation history, notifications/sounds with Omarchy DND, automatic reconnect, systemd packaging and the Omarchy companion remain unfinished. [Personal Discord QR sign-in](DISCORD_SETUP.md) is implemented; new personal/group DM reception and text replies are implemented with separate enable/send controls. History, offline catch-up and remote read markers remain unfinished; notification previews and bot channels do not satisfy personal DMs. The optional standalone notification bridge remains separate and read-only. No alerts or sounds are emitted by the new live adapters.
