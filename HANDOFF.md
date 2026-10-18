# d-hub implementation handoff

The owner wants a TUI communications hub for Omarchy/Ghostty. The public repository is dastari/d-hub. Live iCloud, Microsoft mail and selected Teams chat transports now run in the shared daemon alongside optional AI decisions. The offline demo remains separate. Read [live setup](docs/LIVE_SETUP.md) and [verification](docs/VERIFICATION.md); real account access/sending has not been exercised by development tools. The owner selected unofficial personal QR setup; authentication is now implemented, while DM events/replies remain the next required integration. Default launch now starts/reuses the missing live service.

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
