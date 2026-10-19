# d-hub integration design

Provider capabilities differ. Implement explicit capability reporting and preserve useful partial support instead of pretending all sources behave like email. Exact endpoints, consent requirements, quotas and attachment limits must be checked against current provider documentation during implementation.

## Implemented live preview · 18 October 2026

Core iCloud/Microsoft mail sync/read/replies and selected Teams chat sync/replies are implemented; [live setup](LIVE_SETUP.md) records exact scope, consent, history bounds and remaining gaps. Fake IMAP/SMTP/Graph servers verify translation and failures. Owner account acceptance and Sent-copy behavior remain untested. Attachment transfer, archive clients and desktop alerts are pending. Personal Discord access remains unresolved below.

## iCloud Mail

Use IMAP over TLS to read mail and SMTP with TLS to send. Apple documents `imap.mail.me.com:993` and `smtp.mail.me.com:587`, with an app-specific password for this client setup. Keep account credentials in the credential store. See [Apple mail settings](https://support.apple.com/en-ie/102525).

Implement mailbox discovery, UIDVALIDITY/UID-based identity, unread flags, incremental fetching, reconnect and MIME decoding. Use server-advertised extensions when supported, with polling fallback. Download attachments on demand. Parse multipart messages, inline content IDs, Unicode headers and reply/thread headers. Do not claim reliable unread mirroring until flag updates are tested.

SMTP replies preserve the intended sender, To/Cc, In-Reply-To and References. MIME-encode attachments. Determine whether the server automatically saves sent mail before adding an IMAP Sent-copy operation. A timeout after SMTP acceptance is an uncertain-send case; avoid blind automatic resend.

## Office 365 mail

Use Microsoft Graph with delegated OAuth for the signed-in owner. Choose a public-client flow suitable for the tenant, with PKCE/device-code behaviour verified against current tenant policy. Do not embed a client secret in a distributed desktop app. App registration and tenant consent are setup prerequisites.

Design around mail listing/incremental sync, full body retrieval, attachment download/upload, draft replies and sending. Separate read access from mail modification and sending access. Request only the scopes needed for enabled features. Follow pagination and delta links as opaque cursors. Expired delta state requires safe resync and deduplication, not mass re-alerting.

For attachments use the documented small-file or upload-session path; limits depend on the API and tenant. Link attachment progress to the outbox operation. A Graph acceptance response does not prove delivery.

Primary references: [mail overview](https://learn.microsoft.com/en-us/graph/outlook-mail-concept-overview), [send mail](https://learn.microsoft.com/en-us/graph/api/user-sendmail?view=graph-rest-1.0), [large attachments](https://learn.microsoft.com/en-us/graph/outlook-large-attachments).

## Microsoft Teams

Use Microsoft Graph for supported work/school chats and selected channels. Chat and channel APIs have different permissions and threading semantics. Microsoft documents delegated `Chat.Read` for listing chat messages; that endpoint does not support personal Microsoft accounts. Sending and file handling need separate endpoint/permission verification. See [chat message listing](https://learn.microsoft.com/en-us/graph/api/chat-list-messages?view=graph-rest-1.0).

Scope the first adapter to one verified conversation type. Verify unread/read markers rather than assuming all Teams client state is exposed. Fall back to explicitly labelled local unread state if necessary. Preserve mentions, edits, deletion and thread context. Teams files may require OneDrive/SharePoint upload and reference handling; a mail-style attachment POST is not a universal Teams solution.

A local desktop has no public inbound webhook by default. Start with bounded polling or a supported local-friendly change mechanism; do not assume webhooks can reach the Tailscale development machine from Microsoft. A relay is an optional later architecture with its own deployment decision.

## Discord

Supported bot integrations are limited to resources the bot is authorised to access. Message content access, intents, permissions, bot identity and upload limits need explicit verification. A bot is not a transparent replacement for the owner's personal Discord session.

On 18 October the owner explicitly selected personal QR sign-in like Concord, superseding the earlier supported-bot-only project constraint. QR account setup and the initial personal DM event/text-reply transport are implemented independently. Live owner acceptance, history/offline catch-up and remote read synchronization remain pending. This is an unofficial route and Discord's self-bot policy remains an account risk. Do not extract tokens from other clients or bypass verification. The owner has confirmed that personal DMs are essential, especially messages from their son; channels and bot conversations do not satisfy the requirement. Each new incoming DM must enter the shared feed in arrival order with quick reply, like mail/Teams. Clarified 18 October 2026: historical context is needed only when a chat opens and may be deferred to phase 2 or 3; do not block the first arrival/reply slice on history. This capability remains unresolved with the supported API; a full personal Discord client is the longer-term requirement. See [Discord self-bot policy](https://support.discord.com/hc/en-us/articles/115002192352-Automated-User-Accounts-Self-Bots) and [developer documentation](https://discord.com/developers/docs/intro).

Use capability-specific UI for read, reply and upload. An optional notification bridge now exists under `d-hub notifications`. New Discord accounts default to the personal-client mode, with connections/sends off; capture needs explicit `discord_mode = "notifications"` and a configured exact notification title/sender. It monitors only configured app names on the desktop session bus, filters titles before persistence and stores bounded previews with local seen/dismissal state. Bus ID, sender connection and serial distinguish events; arrival timestamps and IDs order the feed. It emits no popups or sounds and does not alter DND or bus policy. Denied monitoring is a visible failure.

This bridge does **not** meet the personal DM requirement. The official app must emit a desktop notification; muted/focused/suppressed messages and prior history cannot be recovered. Notification titles are mutable, unverified identities and may collide. `v` requests the official Discord DM list, with manual conversation selection; it cannot reliably open the exact DM from notification data. Read state is local only; no reply target is fabricated. Capture lasts only while its single-writer process runs, not a daemon. Target-desktop format, permissions and handoff remain unverified. The plan retains full personal history/reply as a separate unmet requirement.

### Concord reference review

Reviewed [chojs23/concord](https://github.com/chojs23/concord) at commit `0bdb321e7b8c9543fa2063035d17ac3b70ef2c25` on 18 October 2026. Its [README](https://github.com/chojs23/concord/blob/0bdb321e7b8c9543fa2063035d17ac3b70ef2c25/README.md) documents Rust/Ratatui, personal-account authentication and its unofficial status. The [license](https://github.com/chojs23/concord/blob/0bdb321e7b8c9543fa2063035d17ac3b70ef2c25/LICENSE) is GPL-3.0; d-hub has not chosen a license. No source was copied or added as a dependency, and no Concord installation/login was performed.

Its [history commands](https://github.com/chojs23/concord/blob/0bdb321e7b8c9543fa2063035d17ac3b70ef2c25/src/app/history_commands.rs) request bounded pages with cursors and distinguish initial, older, catch-up and around-message results. This is a useful reference for deferred context retrieval. Personal-client implementations exist; the owner has now selected an independent unofficial QR route. [Setup and verification scope](DISCORD_SETUP.md) records the implemented QR/new-DM/reply slice and its remaining limits. Concord does not establish official API support.

For d-hub phase 1, prioritise incoming personal DMs and quick replies without requiring history. Phase 2 opens a chat into cached context and fetches a bounded recent page, with older pages on demand; phase 3 expands navigation/search and the full client. Opening and paginating a reader must preserve drafts, ignore stale results after a chat switch and keep historical backfill out of the arrival/alert stream. No bulk history download at startup. Deferral does not approve opening another app as the primary reply experience.

## Typed decisions and summaries

Jev and OpenAI Decisions are separate adapters behind a domain-level interface. Both provide typed question evaluation, but do not assume identical payloads or numeric confidence semantics. See [TypeSafe introduction](https://docs.typesafe.ai/introduction) and [OpenAI Decisions](https://developers.openai.com/api/docs/guides/decisions).

Implemented adapters use Jev POST `https://api.typesafe.ai/v1/systemone` with `model`, `state` and named `noul` questions; OpenAI POST `https://api.openai.com/v1/decisions` with `model`, string `input` and named `predicate` questions. Defaults are `jev-latest` and `gpt-6-luna`, respectively, as documented on 17 October 2026. Responses must answer every question with a finite probability in [0,1]; refusals, duplicates, missing or unexpected types fail. Tests use fake outputs; no live latency/accuracy claims are made.

The current question set is one independent global promotion question, one per configured focus profile and, with `spam.for_you`, two "for you" questions. TypeSafe's guidance is to ask narrow questions and combine them in code, so the for-you judgment is split:

- `attention__person`: was the communication written by a person for its recipients, rather than generated by a system, app, service or marketing tool? A person's comment forwarded by a service such as an issue tracker counts as a person.
- `attention__for_you`: is it meant for the owner personally, or for a small group or conversation the owner takes part in, rather than a broadcast to a list, customers or the public?

Both carry Noul `criteria` describing yes and no. OpenAI predicates take instructions only, so the same descriptions are appended inline. The code hides a message when either answer is below `spam.for_you_threshold` (default 0.35, leaning towards showing) or the promotion answer is at least `spam.threshold`.

Jev receives a structured `state` object, as its State guide recommends; OpenAI receives the same object as a JSON string. `owner.address` is the account's configured address. `communication` holds the source; the sender (320 characters); up to ten reply addresses; the addressing (owner in To, owner in Cc, not listed, one-to-one or group chat); the visible recipient count; bulk/list and automated-sender flags; the subject (2,000); and the body (4,000, kept short because Jev loses accuracy on large irrelevant state). Providers record that addressing as evidence:
- iCloud: To/Cc against the account address, treating me.com, mac.com and icloud.com as one mailbox; List-Id, List-Unsubscribe or Precedence bulk/list/junk; and Auto-Submitted.
- Microsoft mail: toRecipients/ccRecipients and Outlook's `inferenceClassification` of "other". Folders still on a cursor created before this release report unknown addressing until they restart.
- Teams: the chat ID suffix (`@unq.gbl.spaces` for one-to-one, `@thread.` for groups) and application senders.
- Discord: DM versus group DM, and the author's bot flag.

Addressing is evidence for decisions and the local automated rule. It never chooses a reply's recipients. No attachments or tool authority are sent. Messages the local rules already hide are not sent for classification, and pending classifications run newest first. Global and per-account consent are required before scheduling. A single worker avoids blocking the UI; errors show fallback content, and fingerprint checks reject stale results after message, prompt, model or policy changes. Demo never schedules these workers.

Further independent questions planned over sanitised message/context:

- Does the communication match the named focus criteria?
- Does it explicitly require a response from the owner?
- Which configured project/category best matches, including an “other” option?
- How urgent is it on a defined rubric?

Return typed answers and provider metadata, preserve uncertainty and cache by message revision and question/profile version. Tune thresholds on owner-labelled examples; a valid schema is not a guarantee of a correct decision. Provider timeouts leave messages accessible with a pending/failed classification indicator. Do not base interaction guarantees on the owner's cited 70–500 ms latency range; measure the chosen provider and context size.

Generated summaries use a separate text-generation adapter, preferably requested or prefetched only under configured privacy/budget policy. Label summaries, retain original text, track source revision and invalidate stale results. A summary must never decide recipient, account or send action.

Default external AI processing off per account until configured. Allow choosing fields, bounded thread context, excluded accounts/senders, provider, timeout and spending limits. Exclude attachment bytes unless explicitly enabled. Treat content as data, including quoted instructions; no tool execution is needed for classification or summary.

## Ghostty and rendering

Ghostty supports Kitty graphics, and ratatui-image lists Ghostty compatibility. Keep protocol detection and text fallback. Verify installed terminal version, scrolling, resizing, alternate-screen behaviour and image clearing in the actual app. Sources: [Ghostty features](https://ghostty.org/docs/features), [ratatui-image](https://github.com/ratatui/ratatui-image), [synchronised output](https://ghostty.org/docs/help/synchronized-output).

Use a configurable launcher that passes argv safely to Ghostty. The installed version may support `ghostty +new-window` or an executable command through `ghostty -e`; determine exact syntax by its help and test. Reader selection is an opaque local ID. Do not hardcode shell snippets. Source: [Ghostty on Linux](https://ghostty.org/docs/linux/systemd).

## Omarchy, notifications and clipboard

### Companion plugin

Ship an optional QML companion plugin for an unread/connection-status bar widget, Ghostty drawer activation and quick focus-profile actions. Omarchy documents manifest-based shell plugins hosted inside its long-running Quickshell process. Keep communications work in the independent Rust daemon and connect the plugin through d-hub's versioned local API, directly or through a narrow typed CLI bridge. Sources: [Omarchy shell plugins](https://omarchy.org/manual/shell-plugins/) and [shell reference](https://github.com/omacom/omarchy/blob/quattro/docs/omarchy-shell.md).

Verify the installed desktop's manifest schema, plugin lifecycle, IPC, window activation and focus reporting before implementation. The plugin must reconnect after daemon or shell restart, show unavailable/stale status honestly, and preserve existing drafts during activation and focus-profile changes. Plugin removal must not stop the daemon or remove application state. Prototype locally after milestone 2; package the Rust application separately because plugin installation must not be assumed to build or install backend dependencies. No publishing or desktop configuration change is part of this planning update.

Keep application preferences in d-hub's shared settings registry; leave plugin placement and enabled state to Omarchy. Read theme/DND/window state through a version-aware desktop adapter. The communications drawer remains a Ghostty TUI; a native QML replacement is outside this plan.

### Notifications and clipboard

Use standard `org.freedesktop.Notifications` D-Bus for desktop alerts. The originating installation's Quickshell notification service supports a default click action; do not assume it provides arbitrary inline-reply widgets. Clicking should activate an existing drawer or launch `d-hub preview` safely.

The installed Omarchy service exposes a DND state query. Discover its supported command/IPC invocation on the target desktop and keep it inside an adapter. If DND status cannot be established, suppress custom sounds by default and expose the integration state in doctor/settings. App identity must be d-hub, not a generic critical-system-notification identity that bypasses policy.

Desktop sound playback is independent from popup rendering. Prevent separate playback from bypassing DND. Test quiet hours across midnight, timezone changes and suspend/resume. Only the backend emits alerts, with a persistent event ledger to prevent duplicates.

Use Wayland clipboard MIME access for images and copied files, optionally through [wl-clipboard](https://github.com/bugaevc/wl-clipboard) or a compatible Rust library. Inspect offered formats, handle file URI lists/file-manager formats and preserve a provider process where needed for clipboard lifetime. Test against the owner's actual file manager and screenshot tool. Text paste alone cannot establish file-paste support.
