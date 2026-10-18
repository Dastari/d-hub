# d-hub implementation handoff

The owner wants a TUI communications hub for Omarchy/Ghostty. The public repository is dastari/d-hub. Live iCloud, Microsoft mail and selected Teams chat transports now run in the shared daemon alongside optional AI decisions. The offline demo remains separate. Read [live setup](docs/LIVE_SETUP.md) and [verification](docs/VERIFICATION.md); real account access/sending has not been exercised by development tools. The owner selected unofficial personal QR setup; authentication and initial new-DM/text-reply transport are implemented; actual owner delivery acceptance, history/read sync and offline catch-up remain next. Default launch now starts/reuses the missing live service.

## Production delivery and updates · 18 October 2026

The owner explicitly requested GitHub production binaries, a one-line curl installer, background/frontend/manual update checks and automatic service upgrades. Version 0.2.1 adds a managed stable channel; see docs/UPDATES.md. Source builds remain untouched, demo stays offline, and auto_update is shared TUI/CLI configuration. The owner/update monitor stages verified releases off-thread; frontends flush and pause, active sends drain, locks close, then service/frontend exec uses the installed path. No automatic failed-migration rollback or restart supervisor. Production release publishing does not imply completion of remaining live account/desktop feature acceptance. The v0.2.1 release is published with both native Linux assets; actual public installer and updater HTTP/download/service-exec smoke passed in temporary empty profiles. See docs/VERIFICATION.md for links and evidence. Pre-v0.2.1 development services require a one-time save/close/stop before the first managed launch.

## Development environment

Continue in the owner's existing checkout; do not relocate the project. Rust is installed through rustup under `~/.cargo/bin`; a noninteractive shell may need that directory on PATH. The prototype was verified with `rustc 1.97.1` and `cargo 1.97.1` on 17 October 2026. Dependency versions are locked in Cargo.lock. The manifest declares Rust 1.88 as the minimum; that toolchain has not been independently tested. Development can be headless, so actual Ghostty/Omarchy checks must be recorded separately.

Eight repository-local skills are included under `.agents/skills`; see [skill inventory](docs/AGENT_SKILLS.md). Open the project directory in a new agent session for automatic discovery where supported.

## Implemented prototype and next task

`cargo run --locked -- demo` runs without network access or credentials. The prototype has one mixed chronological fixture feed, focus selection, a multiline composer with fixture attachment chips, navigation/insert modes, mouse interaction and deterministic draft locks. Mock success, failure and uncertain acceptance are explicitly labelled. `--data-dir` enables atomic demo snapshot persistence with one writer; this is not the planned SQLite daemon.

See [verification notes](docs/VERIFICATION.md) for automated and PTY checks. The milestone 2 foundation now includes SQLite migration/import, one state owner, versioned bounded Unix IPC, a sole drawer editor lease, service-owned mock completion, shared readers, setting coordination and recovery files. Schema 2 now adds normalized scoped provider records and private local state; mail HTML conversion and daemon AI workers are implemented. Continue with private attachment staging, reconnect/event delivery, owner account smoke tests and actual Ghostty acceptance. `preview --demo` still reads immutable fixtures. `daemon --demo --foreground --data-dir DIR --socket PATH`, `drawer --socket PATH` and `preview --socket PATH ID` exercise shared state. Ctrl+o in an attached drawer requests Ghostty with the same socket and opaque ID through argv, with an in-process fallback when spawning fails. Window focus, modified keys and actual second-window behaviour remain unverified on the target desktop.

The owner prioritized real-data use on 18 October: live transport work proceeded independently of desktop/plugin acceptance. Continue milestone 2a on the target desktop and retain conservative focus behavior. The offline demo remains independent of Omarchy.

## Owner decisions already established

- Name: d-hub; D refers to dastari, the owner's GitHub name/online moniker.
- Target: Arch Linux with Omarchy and Ghostty; TUI throughout, including later complete clients.
- Delivery: standalone Rust application plus an optional QML Omarchy companion plugin for unread/status display, drawer activation and focus-profile actions. The daemon owns all communications work; the plugin controls the Ghostty TUI rather than implementing a native QML message drawer.
- Stack direction: Rust, Ratatui, Crossterm, terminal pixel graphics through the Kitty protocol.
- Visual direction: minimal borders, open spacing, consistent semantic colours across sources, subtle animated colour shifts and braille activity indicators, with reduced motion available.
- Sources: iCloud via IMAP, Office 365 mail, Teams and Discord, combined without source tabs.
- Editable Work/Home and arbitrary focus profiles with per-profile prompts, plus an independent global promotional/spam exclusion policy.
- Source brand colours, filled unread/hollow read dots, persistent seen state, accepted-reply dismissal, manual dismissal and show-dismissed access. Remote read syncing is implemented for iCloud/Microsoft mail; Teams markers remain local.
- Focus profiles and AI decision filtering, with Jev from TypeSafe AI and OpenAI Decisions provider options. Optional AI summaries are a separate capability.
- Reply-first navigation (default on): Up/Down explicitly saves/switches per-message drafts, Left/Right switches sources, Alt+arrows moves the text cursor. Keep typing in the composer; pending/uncertain sends block context changes. Draft keys include incoming message identity, and old saved keys are normalized from the stored target.
- Dedicated context-aware reply box, automatic latest context while inactive and empty, draft locking, file/image paste and attachments.
- Neovim-style keys alongside arrows and mouse; many configurable shortcuts and discoverable commands.
- A second Ghostty window for full preview of long messages, before later full mail/Teams/Discord TUI clients.
- Comprehensive in-app settings, command-line access, custom TUI themes, sounds and Omarchy desktop alerts.

## Working assumptions to validate without blocking the demo

- “Collapsed drawer” means a narrow ordinary Ghostty window initially. Milestone 2a tests plugin-driven activation/focus and a supported show/hide approach; monitor, width and any reserved-edge behaviour still need a Hyprland integration decision.
- “Latest” means newest eligible incoming communication in the active focus, not AI priority order. UTC timestamp with a stable tie breaker; historical backfill must not displace recent live context.
- Explicit selection freezes automatic following while the drawer is focused. Any text or attachment locks the target even if the user browses another item.
- A successful send means provider acceptance; delivery may occur later. Uncertain transport outcomes require reconciliation, never blind retry.
- Initial email reply mode is reply to sender. Reply-all is explicit and displays the complete recipient set.
- Preview opens a reader first. If later editing is available in multiple windows, use one draft editor lease and prevent stale saves/sends.

## Questions for the owner before affected live integrations

1. **Resolved Discord requirement:** personal DMs are the primary reason for d-hub, especially messages from the owner’s son. New messages must enter the shared feed in arrival order and support quick replies first. Clarified 18 October: older history is needed only on chat open and can ship in phase 2 or 3; plan phase 2 on-demand paginated context, then phase 3 broader client. Do not make historical backfill a prerequisite for new messages/replies. The owner pointed to the open-source Rust/Ratatui Concord client; its unofficial personal-account approach and pagination were reviewed in docs/INTEGRATIONS.md. Channels/bot conversations are lower priority and do not satisfy this. The supported API does not supply this inbox; the owner explicitly chose Concord-style QR login, now independently implemented. Continue DM sync/replies on that selected personal-client route. The explicit opt-in notification bridge is only interim, with no history, reply, remote unread or reliable conversation deep link. Do not silently substitute it or claim Discord complete.
2. Which Office 365 tenant/account and Teams scope: personal/group chats, channels or both? App registration and consent may need tenant administration.
3. Which messages/accounts may be sent to AI providers? Default to no external AI processing until configured per account.
4. How should the drawer appear: manually sized window, toggled special workspace, or compositor-managed side panel? Which monitor and width?
5. Should the empty composer normally start in insert mode on focus? Propose yes, so the owner can switch to the window and immediately type; Escape enters navigation mode.
6. What shortcuts and notification sounds are preferred? Ship editable defaults and a settings preview rather than blocking scaffolding.

## Source and environment notes

See docs/INTEGRATIONS.md for primary documentation links. References were checked during the planning conversation on 17 October 2026; recheck exact API schemas, availability and terminal behaviour during implementation.

The originating desktop uses Omarchy's Quickshell notifications service. Its installed `omarchy notification send` implementation uses the standard `org.freedesktop.Notifications` D-Bus interface and supports click actions. Notification service IPC exposes `dndState`; determine supported invocation from the installed version. Sound suppression needs to read DND too; desktop popup suppression alone is insufficient.

The originating Ghostty config includes Shift+Insert paste, Control+Insert copy, and an explicit Shift+Enter sequence. This is context for compatibility testing, not authority to overwrite another machine's configuration.

## Delivery boundary

Public repository creation and pushing to the owner's personal `dastari/d-hub` GitHub project are authorised. Do not publish credentials, message caches, personal machine addresses or real message fixtures. No project license has been selected. Real provider setup/sending, desktop configuration and service installation still require their own explicit setup/testing step.

## Existing launcher migration · v0.2.3

The owner encountered the installer's existing ~/.local/bin/d-hub guard while moving from a development installation. The installer now supports explicit --replace after release validation, with unique private launcher backups, atomic symlink replacement and preservation of relative/dangling link targets. The default refusal includes the complete migration command. No owner launcher was visible in this development environment, so do not claim it was migrated directly. See README.md and docs/UPDATES.md. v0.2.3 is published; both native release jobs passed, and the public installer successfully migrated an existing launcher with a verified private backup in a temporary HOME.

## Responsive settings · v0.2.4

The owner reported a tall settings drawer displaying only three entries and indistinguishable field/value/help text. The new src/tui/settings.rs renderer uses the available height with variable wrapped row heights, grouped accent headings, secondary-colour values, muted notes and a continuous selection surface. Wide name/value columns and narrow stacked rows share Unicode-safe wrapping. Page/Home/End and mouse scrolling/clicking preserve the composer. Viewport state is local, not persistent config. v0.2.4 is published after both native release jobs passed. A downloaded production binary also passed a dedicated settings PTY check at wide and tall-narrow sizes, including RGB output, Home/End/page navigation and terminal restoration. See docs/VERIFICATION.md.

## Account command repair · v0.2.5

The owner could not add accounts or complete setup commands. Reproduced on v0.2.4: bracketed pastes in settings were ignored, and Tab after account replaced it with update because setup commands were absent from the static list. src/tui/commands.rs now owns prefix completion (including providers and configured account/profile IDs), command paste routing and a horizontally scrolling command panel. Settings remain behind the panel; Esc returns there, successful account creation selects its fields and settings show validation/sign-in notices. The composer still treats normal text literally; from insert, Escape then : opens commands. Pasted commands never execute until Enter. Account metadata remains disabled and uses the shared validation/persistence path. Production publication and post-download verification are recorded in docs/VERIFICATION.md.

v0.2.5 is now published with successful native x86_64/ARM64 release checks and main CI. The actual public installer/downloaded x86_64 binary passed the expanded account-setup startup PTY in an isolated temporary HOME. Use :update to receive it; account creation and real QR authentication remain separate actions.

## Initial personal DM transport · v0.3.0

The owner completed account setup and reported no incoming DM. v0.2.5 had QR authentication but no DM transport; that gap is now implemented in src/providers/discord. The daemon-owned per-account Gateway job verifies the pinned user, keeps heartbeats independent of REST, handles reconnect/resume and bounded-queue replay, excludes own/guild messages and forwards new personal/group DMs. The account worker verifies the QR keyring credential and REST identity, resolves private recipients, handles edits/deletions and rechecks recipients before text replies. Normal outbox/draft guards and uncertain-send handling remain authoritative. Mail/Teams cache scope is unchanged; Discord scope includes mode and verified user.

Sign-in never enables connections/sends implicitly. Existing signed-in accounts need :account enable ID, then :connections until connected; allow_send separately permits text replies. :account disable ID stops reception. QR success text now explains this, and the commands complete configured IDs. Updates preserve opt-ins. New DMs use first local arrival time and stable remote IDs; duplicates do not reset time, deleted originals cannot resurrect or redirect a draft, and read dots remain local. No history/offline catch-up after full stop/update or expired resume session yet; ask for a NEW test DM only after connected. No real owner credential/Discord traffic was accessed in development. Full owner arrival/reply acceptance, read sync, on-demand history/offline recovery and attachment bytes remain open. See docs/DISCORD_SETUP.md and docs/VERIFICATION.md.

v0.3.0 is published after successful native x86_64/ARM64 release checks and main CI. The public installer/downloaded x86_64 binary passed the startup/account enablement regression in a temporary HOME. Next priority: owner verifies a NEW DM after :account enable ID and :connections reports connected, then explicitly enables allow_send for a first reply. Development has not accessed the owner's credential or real Discord session. Preserve this live-acceptance distinction and the no-history/offline-catch-up limits when continuing.

## Settings pages and multiple accounts

The owner found settings and account setup too complex. Settings are now pages (src/tui/settings.rs with actions/labels/status/render/tests submodules): an overview, global sections and an Accounts section with per-account pages for status/statistics, rename, sign in again, connect/disconnect, reply/AI permissions, provider-specific fields and confirmed removal. + Add account is a local form that suggests the next free provider ID so several accounts of one type are routine. `accounts.ID.name` is a display label only: the ID still keys credentials, drafts and cursors, and `AccountConfig::same_connection` ignores the name so renaming never restarts a worker or reclassifies messages. Empty names are not serialized. CLI parity: `account add PROVIDER [ID] [--name]`, `account rename`, `config set accounts.ID.name`. TUI removal erases the keyring sign-in; CLI `account remove` remains metadata-only. Page state is window-local. Verified with automated tests and the four PTY scripts only; actual Ghostty acceptance remains pending.

v0.4.0 is published (settings pages, several accounts per provider, stacked feed previews, Ctrl+E dismissal, local times); the public installer/downloaded binary passed the startup PTY. Owner roadmap: verify Teams/iCloud/Office 365 against real accounts next, then a full-screen multi-mode interface (the drawer stays the compact docked view), the jev/decisions API, work/personal account tags and decisions-based filtering of spam and non-direct messages.

v0.4.1 adds read-only `account test`, provider hardening and the built-in work Microsoft registration (gema-2026's app ID and tenant) for new Office 365/Teams accounts. The owner confirmed Teams is a work account. v0.4.2 edits settings values, prompts and credentials in place in their row instead of a separate screen. Real-account acceptance is next; then iCloud connection reuse, Teams edit/reaction handling, quoted-reply markers and Office 365 reply quoting.

TUI module layout: src/tui.rs is only the root (shared imports, public re-exports, hit areas). drawer.rs lays out the drawer and composer, feed.rs groups and previews feed messages, preview.rs owns the pane under the feed and the reader text (chat stack transcripts, email From/Date), input.rs dispatches keys and mouse, event_loop.rs owns terminal setup and the loop, commands.rs owns command entry/completion/execution, actions.rs sends/reader launch, auth.rs credential entry, discord_qr.rs the QR modal, theme.rs/widgets.rs/help.rs shared presentation. Tests sit beside the code they cover. Settings navigation types live in src/app/settings_page.rs. src/when.rs formats provider timestamps in local time; `Message::stack_key` and `App::stack`/`App::burst` define feed stacks and what is dismissed together. Add new surfaces as their own module rather than growing the root.
