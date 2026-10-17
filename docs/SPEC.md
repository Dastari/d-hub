# d-hub product specification

d-hub is a personal communications hub for dastari. Its primary surface is a narrow Ghostty terminal beside other work on an Omarchy desktop. It should make it possible to glance at relevant unread communications, read context and immediately reply without switching between source applications.

## Required product behaviour

| ID | Requirement | Initial delivery |
| --- | --- | --- |
| FEED-01 | One mixed chronological feed across accounts and sources, with source labels and no source tabs | Demo and first usable release |
| FEED-02 | Recent unread communications, thread grouping, account/sender filters and named focus profiles | First usable release |
| REPLY-01 | Persistent multiline reply composer targeting selected communication or latest eligible incoming item while inactive | Demo onward |
| REPLY-02 | Text or attachments lock the full reply target until successful send, explicit save-and-switch or discard | Demo onward |
| REPLY-03 | Returning to an inactive window with an empty composer leaves it ready to reply immediately | First usable release |
| FILE-01 | Paste copied files and clipboard images; add through a TUI file picker; remove and preview attachment chips | First usable release |
| FILE-02 | Receive, save and copy attachments to the desktop clipboard, including multiple files | First usable release per provider capability |
| PREVIEW-01 | Open long messages and thread context in a second Ghostty TUI window | Early prototype |
| AI-01 | Configurable Jev and OpenAI Decisions adapters for typed filtering and focus evaluation | First usable release |
| AI-02 | Optional generated summaries with visible source context and stale/loading/error state | First usable release |
| INPUT-01 | Neovim-style movement, arrows, mouse, configurable shortcuts and discoverable commands | Demo onward |
| CONFIG-01 | Searchable settings TUI and full CLI access to every persistent option | First usable release |
| THEME-01 | Custom Unicode/RGB themes, editable semantic styles and live preview | Demo onward, expand in first release |
| ALERT-01 | Sounds, native Omarchy notifications, DND, quiet hours and per-focus alert policy | First usable release |
| DESKTOP-01 | Optional Omarchy companion plugin for unread/connection status, Ghostty drawer activation and quick focus-profile actions, backed by the single d-hub service | Prototype after persistent service and reader; first usable release |
| CLIENT-01 | Full TUI mail, Teams and Discord workspaces in additional windows | Later delivery |

“First usable release” can be delivered in incremental milestones. Do not represent all integrations as complete merely because a shared interface or mock exists.

Deliver the Rust application and optional QML companion plugin separately. The feed, composer and reader remain TUIs; the plugin adds desktop controls. Standalone use must work without the plugin, and shell/plugin reload or removal must preserve daemon operation, drafts and settings.

## Feed and focus

Default to newest incoming items first, using provider timestamps converted to UTC and deterministic identity tie breakers. Show local time and date separators. Each row exposes sender, source/account, subject or conversation, preview, unread status, time and any actionable classification. Use thread rows where possible and prevent duplicates across reconnect/backfill.

Focus profiles combine explicit rules with optional typed model decisions: people, accounts, projects, mentions, response required and urgency. Source is a filter, not a tab. Chronology remains the default ordering and determines the automatic reply target; an optional priority sort does not silently redefine that target.

Keep selection by stable ID when rows move. While focused, do not move the viewport or selection to a newly arriving item. Show a new-items indicator instead. Sync must continue even when a focus hides items. Keep unclassified or uncertain items discoverable rather than permanently dropping them.

Unread state belongs to providers where supported. Local seen/dismissed state is separate and labelled when server unread state is unavailable. Previewing does not mark read by default; marking read, archive, snooze and dismissal are explicit actions with configurable policy.

## Composer and previews

The composer has a recipient/account header, multiline editor, attachment chips, draft-lock indicator and send status. It reserves usable space in narrow windows and can expand for long drafts. It must distinguish viewed item from reply target whenever they differ.

Use the state contract in [INTERACTIONS.md](INTERACTIONS.md). Drafts survive navigation, window closure and crashes. A failed send retains everything. Retry must avoid duplicates and must never fall through to a different account. Reply-all, forwarding and composing a new conversation are explicit commands that show recipients before sending.

Full preview is a TUI reader, not a browser. Convert HTML email into a readable semantic representation with headings, lists, links, tables where feasible and quoted sections. Prefer a plain-text alternative when suitable. Support full text search and quote expansion. Use terminal graphics for embedded images where supported, with text fallback. Loading remote images is an explicit privacy-controlled action; never execute scripts or render raw terminal escapes.

## Visual and input design

Use Unicode box drawing, symbols, block elements and multilingual text, not legacy extended-ASCII encodings. Match width calculations to terminal behaviour and test wide characters, emoji, combining marks and pasted multiline text. Optional font icons need Unicode/text fallbacks. Truecolor and actual pixel images are separate capabilities.

Aim for an AI-harness feel: open spacing, minimal borders, clear status, persistent composer, attachment chips, slash commands, a command palette and contextual shortcut hints. Use a consistent semantic palette across sources rather than provider brand colours. The owner prefers subtle animated colour shifts and braille spinners; keep motion restrained and tied to useful activity, with reduced motion available. Support sufficient contrast and redundant status cues beyond colour.

## Attachments

Clipboard text inserts into the editor. A dedicated rich-paste action inspects Wayland MIME types to attach copied files or image bytes. Do not treat every pasted string resembling a filename as an attachment. When a clipboard offers both image and text forms, choose based on the invoked action and expose ambiguity rather than losing content.

Snapshot attachments into private draft storage; preserve safe display names and original MIME type. Pasted images get generated filenames. Track byte size, upload state and provider limits. Reject unavailable files, unsafe special files and unsupported sizes with a clear reason while preserving the draft. File picker supports multi-select and hidden-file toggle. Drag-and-drop is optional until verified in Ghostty on Linux.

For copying received files, first download to a managed local path, then advertise file URIs through Wayland; copying an image can optionally export image bytes. Keep clipboard-backed files alive long enough to paste elsewhere. Clipboard service lifetime and cache cleanup are part of implementation, not just a toast saying “copied”.

## Settings and command line

Expose all supported settings through one typed registry used by settings TUI, CLI, validation and configuration files. Include account setup, focus, AI, keys, themes, layouts, preview, drafts, sync, attachments, sounds, alerts, privacy and data retention. Settings must show validation errors, restart requirements and effective-value origin. Secrets are referenced through a credential store, not written into exported configuration.

## Sounds and desktop alerts

Provide configurable incoming-message, mention/priority, send-failure and connection-failure sounds. Optional success sounds default off. Support custom sound files, volume and a test action. One policy engine decides both popup and sound eligibility using focus, source, sender, user activity, DND, quiet hours, cooldowns and grouping. Prevent reconnect/backfill storms and repeated sounds for edits or duplicate provider events.

Respect Omarchy DND for sounds as well as toasts. Clicking an alert selects its exact conversation or opens a full preview without overwriting a draft. Default to sender-only notification text for privacy. Do not focus-steal on arrival. The background service owns alerts so they work with the drawer closed and occur once with several windows open.

## Later full clients

The full mail TUI should add folders, search, threading, composition, replies, forwarding, attachments and account management. Teams and Discord should add richer supported conversation/channel navigation and history. All use the same identities, drafts, settings and backend. This does not imply complete feature parity with every official client, voice/video support, or unrestricted Discord personal-account access.

## Proposed performance targets

Measure on the actual Arch/Ghostty desktop with recorded fixture size. Target under 50 ms input-to-visible-update at p95 for navigation with 10,000 cached items, under 1 second warm startup, and no network/AI/image encode work on the render loop. Idle reduced-motion views should not continuously redraw; animated views use a bounded cadence. These are targets to benchmark, not measured claims. AI latency is provider-dependent and cannot block feed interaction.
