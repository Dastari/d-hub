# d-hub implementation plan

Deliver vertical slices with an offline demo first. Provider restrictions and desktop integration are independent risks; a blocked Discord capability must not stop the working mail drawer.

Deliver a standalone Rust application with an optional Omarchy companion plugin. The daemon owns accounts, sync, storage, drafts, sends, AI and alert policy; the plugin displays unread/connection status, activates the Ghostty drawer and offers focus-profile actions through the same backend. Keep the drawer and full reader as TUIs. A native QML communications drawer would require a separate product decision.

## Implementation status · 18 October 2026

The offline TUI, reply-first composer and shared service foundation are implemented. The owner's latest request prioritizes migration to real data, so live provider work proceeds independently of the Omarchy plugin/desktop acceptance.

Implemented live slice: empty isolated live profiles; schema-2 account-scoped remote identities, local state and transactional cursors/outbox; daemon-owned iCloud IMAP/SMTP, Microsoft mail delta/read/reply, selected Teams chat sync/reply with local read dots; MIME/HTML text; keyring OAuth refresh and identity verification; per-account enable/send controls; daemon AI decisions; explicit uncertain-send resolution. Protocol 2 prevents old demo-only clients mislabelling live sends. See [setup](LIVE_SETUP.md) and [verification](VERIFICATION.md).

Milestones 4/5 and the Teams portion of 9 have working core transports, **not completed exit criteria**: owner authentication/read/send/Sent-copy checks are pending; attachments, archive/history browsing, retention, chat discovery/channels and target-desktop tests remain open. Milestone 2 still needs attachment staging and automatic reconnect. Omarchy plugin, DND-aware alerts/sounds, rich clipboard/images and packaging remain planned. Personal QR authentication and automatic missing-service startup are now implemented; see [Discord setup](DISCORD_SETUP.md). Do not mark the full implementation complete.

## Owner requirements and next slices

Personal Discord DMs, especially from the owner's son, remain essential; neither channels/bots nor notification previews satisfy this. The owner selected unofficial QR login, now implemented; personal DM sync/replies remain the next transport work. Initial new arrivals/quick replies precede on-demand history in phase 2/3.

Next: complete owner live-mail/Teams acceptance with an explicitly selected test recipient; deliver real attachment bytes and received-file access; add archive/history navigation and retention; improve reconnect; then desktop/plugin/DND integration and packaging. Continue independent core work alongside the selected personal Discord DM route and pending target-desktop checks. Work/Home/custom profiles, global spam prompts, source colours, local dismissal, per-message drafts and all typed settings remain shared between TUI and CLI.

## Discord delivery phases · owner clarification 18 October 2026

These phases describe Discord scope, independently of the overall milestone numbers below.

1. **New DMs and quick replies:** receive personal messages, especially from the owner’s son, into the shared feed in arrival order. Retain messages received by d-hub for immediate context; do not require historical backfill before showing or replying to a new arrival. Replies use a verified, immutable account/conversation target. The owner has selected unofficial personal QR setup; authentication is implemented, while the DM event/reply transport remains pending; notification previews alone do not complete this phase.
2. **Chat context on demand:** when the owner explicitly opens a chat, show cached context and fetch a bounded recent page, then older pages on scroll/request. No startup-wide history download. Provide loading, retry and end-of-history states; opening a chat without network access still shows cached content. Reject late results for another conversation, deduplicate overlap and keep the reader position stable. Historical messages stay in the conversation view and never become fresh feed arrivals, alerts or automatic reply targets. History cannot redirect a locked draft. External AI processing of additional history needs its own configured consent/context policy.
3. **Broader Discord workspace:** add conversation navigation/search and richer personal-client features. Server channels remain lower priority. The owner also accepts moving history to phase 3 if needed; phase 2 is the planned target, not an initial-release gate.

[Concord](https://github.com/chojs23/concord) is an open-source Rust/Ratatui reference. Its history implementation uses bounded requests and cursor-based pagination; see the [source review](INTEGRATIONS.md#concord-reference-review). Its personal-account login is unofficial. Studying it establishes a technical reference, not a supported Discord API or permission to copy code, install/login, extract credentials or enable personal-token automation. Review integration constraints and licensing before implementation/reuse. The owner’s clarification defers history; it does not resolve the separate personal-account access decision or approve an official-app handoff.

## Milestone 0 Project foundations

Verify Rust/toolchain availability on the development box, read project instructions and choose compatible crate versions. Scaffold the Cargo package, formatting/linting and focused tests. Define IDs, capabilities, composer events, error types and the typed settings registry. Add fake mixed-source messages, a long HTML/plain email and harmless attachment fixtures. Keep all sample people/content fictional.

Exit: `cargo fmt --check`, appropriate clippy checks and tests pass; `d-hub --help` and `d-hub demo` run without credentials. Demo writes to an isolated temporary/profile data directory and cannot send real messages.

## Milestone 1 Drawer and composer

Implement chronological mixed feed, focus selection, previews, multiline input, attachment chips, explicit navigation/insert modes, arrows, mouse, Vim-style movement, command palette and shortcut help. Implement the composer reducer before coupling it to render state. Simulate new arrivals, send success/failure and focus changes. Add two themes with Unicode and truecolor. Prefer open spacing over borders, a consistent semantic palette for all sources, slow colour shifts and braille activity indicators; provide reduced motion.

Exit: acceptance scenarios C01–C09, U01 and K01 pass in tests/demo. The owner can switch to the window, type a reply, browse elsewhere and see that the draft remains correctly targeted.

## Milestone 2 Persistent service and full reader

Implemented foundation: private SQLite schema/import with retained JSON backup, revision-checked typed Unix IPC, sole editor lease, shared readers, service-owned mock sends, interrupted-send recovery, private disconnected-draft recovery and coordinated service settings. Reply-first arrows save/restore drafts per incoming message, preserving typing focus: Up/Down switches messages, Left/Right sources, and Alt+arrows edits text. This is the owner’s 18 October clarification; explicit navigation mode remains available. Automated and two-terminal PTY checks pass; this does not close the desktop or remaining attachment/HTML/live-worker acceptance criteria.

Add SQLite migrations, private attachment staging, durable drafts/outbox, single daemon ownership and versioned local IPC. Open full-message reader in a second Ghostty window. Add shared event replay, crash/restart recovery and conservative focus handling. Use typed launch arguments and readable HTML-to-terminal conversion.

Exit: restart preserves drafts; two windows cannot duplicate sends/alerts; reader search and long-email scrolling work on actual Ghostty; preview never changes the drawer target. If development is headless, record desktop tests as pending rather than passing them by inspection.

## Milestone 2a Omarchy companion plugin prototype

Immediately after milestone 2, add a small QML companion plugin with a manifest, an unread/connection-status bar widget, drawer activation and quick focus-profile actions. Use fixture data and the existing local IPC contract, directly or through a narrow typed CLI bridge. Keep the plugin source in this project; no remote repository is needed for the prototype. Inspect the installed Omarchy plugin API and Ghostty/Hyprland capabilities before choosing launch, focus and show/hide mechanisms.

Reuse or focus the existing drawer where supported, with a safe launch fallback that connects to the single daemon. Distinguish drawer activation from selecting a reply target. Treat unknown visibility/focus conservatively. Plugin focus-profile changes use the same settings and composer rules as TUI/CLI changes. Do not add provider sessions, database ownership, draft editing, sending or alert emission to the plugin.

Exit: O01–O05 pass on the target desktop with fake messages and stub sends. Repeated activation, plugin reload/disable, shell restart and backend reconnect preserve drafts and do not duplicate the daemon, provider work or alerts. Record Omarchy/Ghostty versions and verified focus behaviour. The demo, drawer, reader and daemon remain usable without the plugin; headless builds leave desktop checks pending without blocking independent core work.

## Milestone 3 Clipboard and attachment lifecycle

Implement Wayland rich paste for files and screenshot/image data, TUI multi-file picker, preview, remove, received-file save/copy and temporary-file lifetime. Support text paste separately. Model provider limits and progress using fake adapters first. An attachment alone must lock context.

Exit: A01–A05 pass on the target desktop; stale clipboard/files and oversized inputs fail without losing drafts. Copy-to-file-manager behaviour is actually demonstrated.

## Milestone 4 First live email adapter

Implement iCloud IMAP/SMTP with secret-store setup, incremental sync, MIME, unread flags, threading, attachments and reply sending. Test protocol translation against a fake server before owner-authorised live smoke testing. Verify sent-copy behaviour and uncertain-send handling.

Exit: real iCloud messages appear once, attachments work, read/dismiss policy is clear, reply is sent with correct identity and references during an explicit live test. SMTP uncertainty never triggers blind retries.

## Milestone 5 Office 365 mail

Add Graph delegated authentication, mail sync, pagination/delta recovery, replies, upload paths and revocation/re-auth flows. Build on the same domain contract while preserving capability differences. Document required app registration and consent.

Exit: second mail account works in the same feed; account-specific sender/recipient and attachment limits remain correct. Token expiry and throttling do not freeze the UI or break iCloud.

## Milestone 6 Settings and CLI completeness

Fill out searchable TUI settings over the common registry; complete config commands, shell completions, structured output, profile overrides, keymap editing/conflicts, theme editing/preview and diagnostics. Add account flows and all implemented options. Registry/schema support begins in milestone 0; this milestone closes coverage gaps.

Exit: every persistent setting is readable/editable through both surfaces, exports omit secrets, invalid values are rejected atomically and theme/key changes preserve drafts.

## Milestone 7 Focus intelligence and summaries

Add Jev and OpenAI Decisions adapters, bounded message/context selection, per-account enablement and typed focus questions. Add independent optional summary generation. Implement cache keys, stale-result rejection, confidence handling and local deterministic fallback. Evaluate thresholds on labelled fixtures and report provider latency/cost observations rather than assumed guarantees.

Exit: AI outage leaves a useful feed; filters never drop a draft's target; stale results cannot replace a newer classification/summary; AI has no send/tool authority.

## Milestone 8 Sounds and Omarchy alerts

Implement the shared alert policy, desktop notification actions, sound mappings, DND adapter, quiet hours, cooldowns, burst grouping and ledger. Add test actions and inactive-window behaviour, and complete Omarchy theme following through the desktop adapter. Build on milestone 2a's drawer activation and plugin lifecycle handling. Provide optional systemd user-service and companion-plugin setup instructions without enabling them silently; shell/plugin lifecycle must not own the daemon's lifetime.

Exit: desktop tests prove DND suppresses sounds and popups, one event alerts once across windows, notification activation preserves a draft and reconnect/backfill is quiet.

## Milestone 9 Teams and feasible Discord support

The owner has resolved Discord scope: personal DM arrivals and quick replies come first; older history loads only on chat open in phase 2 (or 3 if needed), and channels are lower priority. Resolve supported personal access feasibility and the remaining Teams chats/channels scope. Verify Graph permissions, tenant policy and Teams file-upload semantics before implementing the chosen scope. The owner explicitly selected Concord-style personal QR setup on 18 October. That authentication slice is implemented with private keyring storage; continue personal DM events/quick replies next, with on-demand history later. Bots remain a separate optional capability. Make unsupported capabilities visible.

Exit: supported sources join the same feed and context model with honest capability reporting. Initial Discord acceptance requires personal DM arrivals and quick reply; historical backfill is explicitly deferred. The optional read-only notification bridge and bot support do not pass the personal reply criterion. If supported personal access remains unavailable, record the blocker explicitly and retain the unmet requirement.

## Milestone 10 Complete TUI workspaces

Expand the second-window reader into full mail navigation/composition and supported Teams/Discord history/navigation. For Discord phase 2, load conversation context only on explicit chat open and paginate older messages on demand; do not make bulk historical sync an initial feed/reply dependency. Reuse the same service, settings, drafts and attachment paths. Add explicit draft ownership transfer before editing in multiple windows. Define feature scope per client; avoid promising calls, video or parity with official apps by implication.

Exit: user can move from drawer to full workspace and back without losing state or creating duplicate operations.

## Milestone 11 Packaging and release readiness

Owner-requested production delivery slice: v0.2.1 adds native Linux x86_64/aarch64 release CI, SHA-256 verified user installer, stable auto-update checks (service interval/frontend/manual), atomic replacement and draft/send-safe service/frontend handover. See UPDATES.md. Publishing this early application release is explicitly authorized; provider/desktop acceptance below remains pending, as do plugin packaging and systemd setup.


Package the Rust application for Arch and the optional QML companion plugin as separate installable components. Document their version compatibility, missing-backend diagnostics, installation/upgrades, removal and data migration. Verify the installed Omarchy plugin distribution rules; do not assume adding a plugin installs or compiles the Rust application. Measure performance, test terminal resize/Unicode/accessibility and review logs for leaked credentials/content. Choose license/repository publishing with the owner. Include troubleshooting for Ghostty capabilities, plugin/backend reconnect, clipboard, Graph consent, IMAP and DND.

Exit: install/uninstall leaves user data policy explicit, application/plugin upgrades preserve drafts/settings and recorded target-desktop checks pass. Removing or disabling the companion plugin leaves the application and its data usable. Clearly distinguish published release artifacts from the still-pending account and desktop acceptance checks.

## Recommended next agent prompt

> Read AGENTS.md, HANDOFF.md, docs/VERIFICATION.md and docs/PLAN.md. Use the relevant project skills under .agents/skills. Review the existing offline prototype and preserve its composer locks and minimal-border visual direction. Complete actual Ghostty acceptance where available, review the implemented offline owner, SQLite transactions, IPC leases and reply-first navigation. Finish milestone 2’s private attachment staging, readable HTML conversion and reconnect behaviour, plus owner desktop acceptance; migrate existing notification/AI workers into the owner before live multiwindow use. Then follow with milestone 2a’s Omarchy companion prototype. Do not connect live accounts, send real messages or silently change desktop configuration. Record automated versus actual desktop checks separately.
