# d-hub integration design

Provider capabilities differ. Implement explicit capability reporting and preserve useful partial support instead of pretending all sources behave like email. Exact endpoints, consent requirements, quotas and attachment limits must be checked against current provider documentation during implementation.

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

Do not automate a normal user account or reuse a personal client token. Discord prohibits self-bots. Personal DMs to the owner are therefore an unresolved product capability, not a promised feature. Establish whether selected server channels and bot conversations satisfy the owner before implementation. See [Discord self-bot policy](https://support.discord.com/hc/en-us/articles/115002192352-Automated-User-Accounts-Self-Bots) and [developer documentation](https://discord.com/developers/docs/intro).

Use capability-specific UI for read, reply and upload. A notification-only fallback, if desired, must be labelled as such and must not claim authoritative full history, unread state or reply support.

## Typed decisions and summaries

Jev and OpenAI Decisions are separate adapters behind a domain-level interface. Both provide typed question evaluation, but do not assume identical payloads or numeric confidence semantics. See [TypeSafe introduction](https://docs.typesafe.ai/introduction) and [OpenAI Decisions](https://developers.openai.com/api/docs/guides/decisions).

Suggested independent questions over sanitised message/context:

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
