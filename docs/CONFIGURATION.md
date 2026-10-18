# d-hub configuration and themes

Use one typed settings registry to describe keys, types, defaults, validation, secrecy, help text and whether a setting needs a restart. The same registry powers the settings TUI, CLI, configuration validation and generated reference. The examples are proposed contracts, not implemented commands.

## Implemented settings

The strict TOML schema remains `schema_version = 1`; older flat configs load with defaults for new fields. The shared `Settings` validation/persistence path powers `d-hub settings`, navigation `,`, and `config list|get|set|reset|validate|export`. In settings, Enter edits a value inside its row (Enter saves single values, Ctrl+s saves multiline prompts), Escape cancels, and Left/Right cycles choices. CLI `config set KEY VALUE` accepts `--file PATH` instead of VALUE for multiline prompts or JSON collections. Unknown fields and invalid values fail atomically. Files are private. For standalone demo/settings surfaces, close the running surface before CLI file edits. For a shared drawer, use `config --socket PATH …` so the service coordinates revisions and writes.

| Keys | Default / behaviour |
| --- | --- |
| `focus` | `all`, or any configured profile ID; `personal` displays Home; `home` is an input alias unless a distinct home profile exists |
| `source` | `all`; navigation cycles all, icloud, discord, teams, email, office365; Email combines both mail providers |
| `theme`, `reduced_motion`, `insert_on_focus` | midnight, false, true |
| `reply_first` | true: Up/Down saves/restores replies per incoming message, Left/Right switches sources, Alt+arrows edits text; false restores traditional insert arrows |
| `padding_horizontal`, `padding_vertical` | 0, range 0–16 per side; content below 28×16 uses a resize fallback |
| `mark_read_on_select`, `dismiss_after_reply` | true; explicit selection/open marks read; accepted replies dismiss their original item |
| `show_dismissed`, `show_spam` | false; navigation D and S include hidden items for inspection |
| `profiles` | JSON list of `{id,name,prompt,terms,fixture_focus}`; Work and Home initially; maximum 32 |
| `profiles.ID.name`, `.prompt`, `.terms` | Display name, independent decision prompt, newline-separated optional local include terms |
| `spam.enabled`, `.prompt`, `.threshold`, `.local_terms` | true, independent promotion/spam question, 0.85, no local exclusion terms |
| `ai.enabled`, `.provider`, `.model` | false, jev, jev-latest; changing provider selects its documented default model |
| `ai.threshold`, `.timeout_ms`, `.hold_pending` | 0.6, 5000 (100–30000), false |
| `accounts` | JSON account list, maximum 32, no secrets; `account add PROVIDER [ID] [--name NAME]`, `:account add [PROVIDER [ID]]` or Settings › Accounts › + Add account creates metadata; omitted IDs use the first free `provider`, `provider-2`, … |
| `accounts.ID.name` | Empty (shows the ID); optional one-line display name up to 80 bytes, shown in the feed, composer and settings. `account rename ID NAME` / `:account rename ID NAME`. Renaming never changes the ID, credentials, drafts, sync cursors, AI decisions or a running connection |
| `accounts.ID.username`, `.tenant`, `.client_id`, `.resources` | Empty username; Office 365 and Teams start with the built-in work app registration (other providers: organizations, empty app ID); newline-separated resources: mail folders for iCloud/Office 365 (**Folders**, default INBOX/inbox), chat IDs for Teams (**Chats**) |
| `accounts.ID.enabled`, `.allow_send`, `.poll_seconds` | false, false, 30; opt-in live connections/sends, polling range 15–3600 seconds |
| `accounts.ID.allow_ai` | false; both this and global AI enablement are required |
| `accounts.ID.discord_user_id` | Empty until QR setup; verified Discord identity pin, validated as a snowflake |
| `accounts.ID.discord_mode` | personal; personal history/replies unavailable; explicit notifications opts into the limited bridge; bot setup is separate and does not satisfy personal DMs |
| `accounts.ID.notification_sender`, `.notification_app` | Empty sender, Discord; notification capture requires exact matching title/sender |

`profile list|add ID|remove ID` also manages profiles. Remove an active profile to return to All. Profiles and global spam are independent questions; spam exclusion applies in All as well. Local terms work offline. Without a valid AI result, local terms or fixture tags provide fallback, otherwise unclassified messages remain accessible. `ai.hold_pending` optionally holds permitted live items until a result; errors release them to fallback. AI filtering never changes a locked draft.

Credentials use `account auth ID` or `:auth ID`; `ai-auth` or `:auth ai` stores the selected AI key. Secret Service requires `secret-tool` and an unlocked keyring. CLI optionally reads secrets through `--secret-stdin`; never pass credentials as arguments. Microsoft uses delegated device login instead of password/stdin. `account forget ID` erases the credential, including after metadata removal. Bot credential storage does not imply an implemented bot transport. Personal Discord QR sign-in is implemented with an explicit identity/keyring-save screen; username/password and pasted-token login remain unavailable. See [Discord setup](DISCORD_SETUP.md). Personal DM reception requires `accounts.ID.enabled = true`; text replies separately require `.allow_send = true`. TUI commands `:account enable ID` / `:account disable ID` (also connect/disconnect) and the account page's Connect/Disconnect use the same typed setting path. The account page's Remove asks for confirmation, refuses while a send from that account is pending/uncertain, removes the metadata and erases its stored sign-in where the keyring is available; messages already in the feed stay until dismissed and provider content is never deleted. CLI `account remove` still removes metadata only; use `account forget` first to erase credentials. Existing sign-ins are reused. These opt-ins are never changed automatically by an update. Discord dots remain local; history/offline catch-up and attachment transfers are pending.

`demo` uses an isolated temporary profile or `demo --data-dir DIR`; its settings default to `DIR/config.toml`. Other commands use global `--config PATH` or XDG configuration. `notifications` uses `$XDG_STATE_HOME/d-hub/notifications` (normally `~/.local/state/d-hub/notifications`) or `--data-dir DIR`, separate from demo. Standalone demo/notification state still uses private atomic JSON. The explicitly launched offline service uses private SQLite WAL storage, importing demo JSON once without deleting it; it owns settings after initialization. Attached settings edits and `config --socket` share the same typed schema and SQLite transaction path. The ordinary TOML file is initialization/export, not a concurrent writer to service-owned settings. Live iCloud and Microsoft mail acknowledge revision-checked remote read changes. Teams read dots remain local. Account/profile/AI-auth convenience commands accept `--socket` for active service metadata. See [live setup](LIVE_SETUP.md).

The sections below describe the broader future contract. `examples/config.toml` remains a design fixture and cannot be loaded as this implementation’s schema. Environment/profile overrides, arbitrary theme files, keyboard remapping and coordination with standalone configuration writers remain pending. `config reset` uses built-in defaults; custom profile/account fields without a default should be edited or removed explicitly. `config list --json` reports origin at the containing TOML section level.

## Files and precedence

Use XDG paths: configuration in `$XDG_CONFIG_HOME/d-hub` (normally `~/.config/d-hub`), state/database in `$XDG_STATE_HOME/d-hub`, disposable cache in `$XDG_CACHE_HOME/d-hub`, and user-private runtime socket under `$XDG_RUNTIME_DIR/d-hub`. Keep themes and profile definitions under configuration. Separate live message/draft state from portable settings.

Resolve values in this order, highest precedence first: explicit CLI flags or `--set`, documented environment overrides, selected profile overrides, persisted user configuration, defaults. Report each effective value's origin. `config set` changes persisted values; launch overrides do not. Define which overrides belong to the daemon versus only the current window; reject attempts to silently mutate global settings with a per-window flag.

Use versioned TOML with strict key/type validation, atomic writes and migrations with recoverable backups. Preserve unrelated values when editing. Reject invalid edits as a unit and show the error without corrupting a running configuration. The TUI should show when a CLI override masks a saved value.

## Settings interface

Search by name, description or shortcut. Categories include Accounts, Focus, AI, Composer, Attachments, Navigation, Appearance, Windows, Notifications, Sounds, Privacy and Storage. Categories in settings do not introduce source tabs into the communications feed.

Provide toggles, enum selectors, validated fields, lists, file pickers and per-value reset. Include effective-value inspection, preview/apply/cancel for themes, sound/notification tests, shortcut capture and conflict detection. Account authentication is an explicit flow; secret values are masked and stored outside TOML. Show disabled capabilities and their reasons.

## Proposed command surface

```text
d-hub [drawer] [--profile NAME] [--focus NAME] [--theme NAME] [--set KEY=VALUE]
d-hub demo
d-hub daemon [--foreground]
d-hub preview MESSAGE_ID
d-hub client [mail|teams|discord] [--account ID]       # later full clients
d-hub settings [SECTION]
d-hub config list|get KEY|set KEY VALUE|reset KEY|validate|export
d-hub accounts list|add|auth|status|remove
d-hub focus list|show NAME|set NAME
d-hub themes list|inspect NAME|set NAME
d-hub keys list|check
d-hub notifications test|mute|unmute
d-hub sounds list|test NAME
d-hub doctor [--json]
d-hub completion SHELL
d-hub --help
```

Every persistent feature setting is reachable by `config` even without a dedicated convenience flag. Include `--config`, `--no-color`, `--version` and structured output where relevant. `--json` must not start an interactive TUI. Emit diagnostics to stderr and machine output to stdout; document exit codes and cursor pagination for future message-list commands. Do not add a broad raw network escape hatch merely to satisfy a generic CLI template.

The Omarchy companion plugin uses this same registry and persistence path for application preferences and focus-profile actions. Do not create a second copy of d-hub settings in the plugin. Omarchy owns desktop plugin placement and enabled state; diagnostics report missing/incompatible plugin or backend capabilities without preventing standalone TUI/CLI use.

Credentials use secret-store references, interactive auth or dedicated stdin input. Do not accept raw secrets on argv as the normal setup path. Exports omit secrets and account message data. Removal distinguishes disconnecting an account, deleting cached data and deleting provider content; never make the latter an implicit side effect.

## Configuration domains

| Domain | Examples |
| --- | --- |
| Account | Provider, display name, enabled, credential ref, sync interval, permitted AI fields |
| Feed | Focus, grouping, unread filter, time display, visible fields, backfill window |
| Composer | Focus-follow behaviour, insert-on-focus, reply mode, send shortcut, draft persistence |
| Attachment | Paste policy, staging limits, download location, preview behaviour |
| AI | Decision provider/model, timeout, question set, thresholds, summary provider, budget |
| Input | Keymap, mode, mouse, prefix timeout, terminal capability overrides |
| Appearance | Theme, density, border style, icons, motion, minimum layout |
| Window | Drawer sizing preference, reader launcher, preview reuse preference |
| Alert | Source/sender/focus rules, grouping, quiet hours, preview privacy, active-window policy |
| Sound | Enabled, volume, sound mappings, cooldown, DND behaviour |
| Storage | Retention, cache quota, attachment cleanup, log level/redaction |

These include invariants with configurable behaviour around them: customization must not allow a background event to send a draft to a different recipient. Do not interpret “everything configurable” as permission to disable correctness.

## Theme contract

Themes contain metadata, semantic colours and styles plus border/icon choices. Tokens should cover background, surface, text, muted text, accent, selection, unread, urgent, warning, error, success, draft lock, attachment and source label. Use distinct source colours, including Discord blurple, for dots and selected-message accents; keep success, error and other semantic colours consistent across providers. The current palettes bundle these source colours; configurable theme tokens remain future work. Each style can specify foreground/background and supported modifiers; every important status has a non-colour indication.

Support RGB hex values, Unicode and conservative fallback borders, optional Nerd Font icons, density and reduced motion. Validate tokens and missing glyph fallbacks. Allow import/export and immediate preview without writing until Apply. Theme changes must not reset navigation or draft state.

Optional Omarchy theme following should read the current desktop theme through a version-aware adapter, translate a small semantic palette and fall back to a bundled theme. Never write into packaged Omarchy files or require the development machine to run Omarchy. A stale or missing theme file is recoverable.

## Offline service paths and recovery

`daemon --demo --foreground --data-dir DIR [--socket PATH]` creates `DIR/service.sqlite3` with schema/user identity checks, atomic migration, a bounded 256-entry change ledger and mock outbox rows. Directories must be user-owned/private (0700); database, socket and recovery files are 0600. A directory lock excludes concurrent owners; a socket lock and same-user peer checks protect local IPC. The demo daemon defaults to `$XDG_RUNTIME_DIR/d-hub/service-demo.sock`; the live daemon and drawer/service commands default to service-live.sock. Use explicit socket paths for demo clients. Default `d-hub` / `drawer` starts the missing live owner on demand. `--no-start-service` and explicit sockets connect only; no systemd unit is installed.

Service settings initialize from `DIR/config.toml` or global `--config PATH` only for a new database. After that, `config --socket PATH list|get|set|reset|validate|export` talks to its owner. Without `--socket` or `--config`, config/account/profile/ai-auth use the running default live service. If a live database exists but the service is stopped, they refuse instead of editing the seed file; origin is reported as service and an unrelated TOML path is not presented as the active config. Concurrent stale settings edits are rejected rather than replacing another edit.

On disconnect, the attached drawer preserves active/saved drafts and pending intent in `$XDG_STATE_HOME/d-hub/recovery/drawer-PID-TIME.json` (normal fallback under `~/.local/state`). If neither state/home environment path exists, it uses the private socket directory. `drawer --socket PATH --recover FILE` restores unsent drafts explicitly and retains the source file. It rejects a pending/uncertain send, an invalid target or conflicting existing drafts. Saved setting edits remain available in the recovery file for deliberate review; recovery does not overwrite settings or replay operations. Automatic reconnect is pending.

## Live provider settings

Teams `resources` entries are percent-decoded when set, so chat IDs copied from Teams web addresses work. See [live setup](LIVE_SETUP.md) for enabled/allow_send defaults, account identity pinning, mailbox/chat resources, initial history bounds, provider capabilities and owner setup steps. Live and demo databases cannot change modes. Standalone TOML seeds new profiles only; edit active service preferences through its TUI or `config --socket`. No-subcommand launch opens the live drawer.

## Production updates

`auto_update` is a boolean, default true, in the shared typed settings registry and TUI. It controls startup, six-hour and frontend-open production checks for managed live installations. `:update` / `d-hub update` explicitly requests a check/install even when automatic checks are disabled. Demo never checks. Non-managed executables are never overwritten. While the owner is running, use `config --socket PATH set auto_update false`; its SQLite settings are authoritative. See [updates](UPDATES.md).
