# d-hub interaction specification

The composer is an independent state machine. Selected feed item, viewed preview and reply target are distinct identities. The owner must always be able to see where a send will go.

## Reply target

A target includes provider, account ID, conversation/thread ID, incoming message ID, recipient set and reply mode. Capture it atomically with a draft revision. Never infer the target again from current selection when sending.

“Latest eligible” means newest reply-capable incoming communication matching the active focus, excluding snoozed/dismissed/explicitly filtered items. Outgoing messages do not qualify. Initial backlog can seed an empty context, but later historical imports must not displace an established newer target. If none qualifies, show an empty composer state; never fall back to a hidden recipient.

## State transitions

| Current state | Event | Result |
| --- | --- | --- |
| Following, empty, drawer inactive | Eligible new item | Set target to latest eligible item |
| Following, empty | Drawer gains focus | Preserve prepared target; default to ready-to-type insert mode, configurable |
| Following or selected, empty, drawer focused | Eligible new item | Preserve target and selection; show arrival indicator |
| Any empty state | Explicit feed selection | Set target to selected reply-capable item; show unsupported state for non-reply-capable items |
| Empty with target | First text edit or attachment | Create persistent draft and lock complete target |
| Locked draft | New item, selection, focus/filter change, preview opens or focus lost | Keep target, body and attachments; allow browsing with a visible locked-target banner |
| Locked draft | User deletes all text and attachments | Keep lock (status shows "LOCKED · empty, Del releases") until explicit release/discard or send lifecycle completes; emptying input is not implicit permission to redirect. Delete, save-and-switch and explicit filter changes (dismiss, D, S, focus, source, settings) are explicit releases of an empty draft; background events never release it; an empty draft is never saved. Narrow drawers show "◆ empty" |
| Locked draft | Explicit save-and-switch | Persist draft, release editor lease, then open/resume the selected conversation's draft or empty target |
| Locked draft | Explicit discard | Confirm meaningful content loss, remove draft, resolve current explicit selection or latest eligible context |
| Locked draft | Send action | Validate target, credentials and attachments; freeze revision and enter submitting |
| Submitting | Provider accepts operation | Preserve sent history/outbox record; clear submitted draft only; enter empty selected state |
| Submitting | Failure | Restore editable locked draft with error |
| Submitting | Unknown acceptance after timeout | Preserve draft and mark outcome uncertain; reconcile before offering a duplicate-prone retry |
| Empty selected state | Drawer loses focus | Resume latest following using current focus |
| Locked draft | Account disconnected or source item deleted | Retain draft, show invalid-target/reconnect state; require explicit recovery |

No background task may invoke discard, save-and-switch or retarget a locked draft. AI text suggestions cannot replace draft text without an explicit action. Navigation in a preview window must not count as selecting a reply target in the drawer.

## Multiple windows and drafts

The daemon stores one draft per incoming message/reply mode, including distinct replies within one conversation, with revisions and an editor lease. Opening a reader does not take the lease. If full-client editing is later enabled, transfer ownership explicitly and make other editors read-only; reject stale writes. Preview windows can emit explicit “reply here” requests, but the drawer must preserve an existing draft and offer save-and-switch.

Ghostty focus events describe terminal focus, not necessarily all compositor visibility. Verify whether hidden, inactive and selected-window states can be identified reliably. If focus detection is unavailable, retain context conservatively and provide an explicit follow-latest toggle; never assume inactivity from elapsed typing time.

The Omarchy companion plugin may activate the drawer or explicitly change its focus profile. Activation alone is not feed selection or permission to retarget a draft. Profile changes obey the same state transitions as the TUI and CLI. Plugin reload, disable or shell restart does not release a draft lock or editor lease; normal client disconnect/recovery rules still apply. Uncertain window visibility remains conservative.

## Keyboard modes

Modes are navigation, composer insert, command entry and modal dialog. An optional Vim text-editing mode can be added later; Neovim-style feed navigation does not require implementing a full editor. Each mode shows its current state and relevant shortcuts.

| Action | Proposed default | Scope |
| --- | --- | --- |
| Switch reply, saving/restoring drafts | Up/Down | Reply-first insert mode; input stays ready |
| Move selection without switching a locked reply | j/k, Up/Down | Explicit navigation mode (Escape) |
| Move text cursor | Alt+arrows | Reply-first insert mode |
| Cycle source filter | Left/Right | Reply-first insert and navigation; reply-first also saves/switches the prepared reply; traditional insert mode uses these for cursor motion |
| First/last | gg/G, Home/End | Navigation |
| Scroll page | Ctrl+d/Ctrl+u (feed); PageDown/PageUp (preview pane, reader) | Feed, preview or reader |
| Focus next/previous pane | Tab/Shift+Tab | Outside text completion/dialog-specific use |
| Reply/edit | i or r | Navigation |
| Exit input to navigation | Escape | Insert; preserves draft |
| Newline | Shift+Enter, Ctrl+Enter or Alt+Enter where distinguishable; Ctrl+J everywhere | Insert |
| Send | Enter; F5 | Enter in Insert; F5 in Insert or navigation |
| Search | / | Navigation |
| Command palette | : | Navigation |
| Composer command | /attach, /summary | Insert at command position; literal slash otherwise |
| Full preview | Ctrl+o | Selected item |
| Show help | ? | Navigation |
| Paste rich attachment | Ctrl+v, configurable | Composer; direct Wayland clipboard inspection |
| Paste text | Shift+Insert or terminal paste | Composer |
| Copy | Control+Insert/terminal copy, contextual copy command | Preserve terminal selection behaviour |
| Close reader | q or Escape | Reader navigation; never discard a draft |

These are proposals, not terminal-independent guarantees. Test Ctrl+Enter/Alt+Enter, enhanced keyboard protocol, terminal-reserved keys and text-paste conflicts in installed Ghostty. Offer alternate bindings when terminals cannot distinguish keys. Avoid Super bindings inside the app; leave window movement to Omarchy. Prefix sequences must timeout and show discoverable hints.

In reply-first mode, an explicit message click saves/restores that message’s reply and retains insert mode. Traditional navigation preserves the locked composer while browsing. Mouse scrolls panes, follows links through explicit actions, focuses composer and removes attachment chips. Hover should not retarget a reply. Scrolling alone should not change the target. Never let a click on a remove chip fall through into send.

## Drawer layout

```text
d-hub   work                 2 new

● Sasha · Office 365        10:42
  Proposal approval

● Jules · Teams             10:40
  Release question

Message excerpt or summary

◆ LOCKED · Work
sasha@example.invalid
 + proposal.pdf · 120 KB ×

Your multiline reply…

INSERT · ⏎ send · ⇧⏎ newline
```

Consecutive messages in the current view that share provider, account and sender (and, for chats, conversation) stack under one heading: sender in bold, then provider · account name. Grouping follows the visible order, so in All another provider's message between them starts a new group, while a source filter that hides it lets them merge. Each message shows its read dot, time and up to three wrapped preview lines with quoted history removed. Mail uses the first line for its subject, and chat placeholder subjects such as "Direct message" are not shown. Rows vary in height: clicks use the rendered row areas, keyboard paging uses the number of fully visible messages, and the feed scrolls a newly selected message into view without undoing wheel scrolling. A heading stays visible when the view starts mid-group.

Feed times are local: today shows `14:03`, earlier this year `18 Oct 14:03`, older `18 Oct 2025`. The pane under the feed shows the selected message's context. A chat stack shows a short transcript of every message in it, oldest first, each with its time (no date) and its own line breaks, under the day and message count. An email shows the sender and address with the full local date, time and UTC offset, then the body. When the pane is too short, the selected message stays in view and a marker points to `o`. The reader (`o`) shows the email with From/Date headers, or the whole chat stack with day separators. Demo fixtures have no real dates and keep their prepared labels.

The composer header names the provider and account (e.g. `Discord · personal`). The recipient line names the person being answered, the incoming sender, plus `+ N more` for group chats and the address for mail. Opaque provider IDs such as Discord user snowflakes are never shown as the recipient; the immutable target still holds them for sending.

At very narrow widths, wrap safely and prioritise recipient, body and attachment count. At wider widths, expand preview naturally. Minimum usable dimensions and fallback message must be measured in the prototype. No account/source tabs appear at any width.

Prefer spacing and subtle surface colours to enclosing boxes. Message sources share semantic success/error colours and have distinct source colours for dots and selection, including Discord blurple. Use labels for source identity as well. Slow accent colour shifts and braille activity indicators provide motion without rearranging content. Reduced motion uses static colours and activity labels.

The header shows one active source filter, cycled with Left/Right in reply-first insert or navigation mode: All, iCloud, Discord, Teams, Email and Office 365, wrapping in both directions. Email includes both mail providers. Source filters intersect with focus and search within the same feed. Background filtering may change the previewed item but cannot retarget an existing draft. In reply-first mode, explicit source-arrow actions save the current draft before preparing/restoring a visible message’s reply; an empty result parks it and clears the recipient. A hidden empty target is cleared while focused; explicit message selection prepares a new reply. Inactive following uses the latest incoming item eligible under both source and focus. Empty combinations show a visible no-matches state.

Outer padding is configurable per axis and defaults to zero. `padding_horizontal` applies equally to the left and right edges; `padding_vertical` applies equally to the top and bottom. Internal composer and dialog spacing remains separate. Layout and mouse hit regions use the same inset area; padding changes must preserve drafts and reply targets.

## Full preview window

`d-hub preview <opaque-message-id>` connects to the same daemon and opens a reader directly when run inside a terminal. The drawer's open-preview action launches Ghostty with that command as an argv array. Reader IDs must not be arbitrary command text or filesystem paths.

Reader provides sender/account/recipients, subject, complete body, quoted history, attachment list, local search and optional summary. Keep summary visibly distinct from original text. Full-preview functionality is an early milestone; advanced folder/channel navigation belongs to later full clients. Reuse a reader window where supported, with a safe new-window fallback. Closing it leaves the drawer and daemon running.

## Read, dismiss and profile filtering

A dotted rule separates the feed from the preview pane; it takes the selected message's source colour, fades towards the background across the width and is hidden when nothing is selected. The drawer ends in one bottom line: a notice for about eight seconds after it appears (cut with an ellipsis when narrow), then compact status. Status is "↓N new" when messages arrived while reading, then one coloured dot per enabled account (● connected, ◌ connecting, ! problem, ○ off), named when they fit and dots only when narrow. The demo shows the next mock outcome instead. An empty feed says why: how many items are dismissed (D), filtered (S), held for AI decisions, excluded by a search, outside the focus (f), or in other sources (←→); with nothing at all it says new messages appear as accounts sync. An email preview shows only its text body (the HTML part when there is one, as mail clients show it, rendered to text without its style sheets or scripts; the plain part otherwise, or when the HTML has no text; layout tables without borders, no blank lines or table rules; the reader keeps single paragraph breaks); the feed row already names the sender, subject and time, and the reader (`o`) adds the full From/Date header. Chat stacks preview as a timed transcript. Links in the preview show as their anchor text, or just the host ("example.com/…") for a bare URL, underlined in the secondary accent; the HTML link list at the end is not shown. Clicking one opens it with the desktop handler (xdg-open; wslview, then explorer.exe, on WSL), passed as one argument with no shell. Only http, https and mailto links open, and the notice names the real host, whatever the anchor text said. Opening a link never changes the selection, draft or reply target. Feed snippets use the same labels; the reader (`o`) keeps full URLs. PageUp/PageDown page the preview without changing the selection or reply target; a new selection opens at its default view. The reply box starts one line tall; the draft wraps at words to its width (a word longer than the row breaks mid-word, and one column stays free for the cursor) and the box grows with the wrapped rows to six lines, then scrolls. Alt+Up/Down move the cursor between wrapped rows. Settings prompts wrap the same way. Feed headings, notices and the composer show a mail sender's display name rather than the raw `Name <address>` header. Rendering removes invisible formatting characters (soft hyphens, zero-width spaces, fillers, bidi controls) that terminals and layout width tables disagree about.

Solid ● means unread; hollow ○ means read. Explicit selection/open marks read by default; because the preview shows a whole chat stack, the whole stack is marked read (an email only itself). `u` toggles the selected message. Background preview/filter changes do not mark an item read. Remote-capable messages enqueue revision-checked read changes; pending local changes survive reconnect and ignore stale acknowledgements. Until real transports exist, report local state and queued intent honestly.

`e` dismisses/restores the selected item; `D` includes dismissed messages. Ctrl+E does the same in insert mode, so a message can be dismissed without replying or leaving the composer; Ctrl never inserts text. Delete dismisses and marks read: always in navigation, and in insert mode while the composer has no text or attachments; it never restores. In insert mode with text it edits the draft. It first releases a draft emptied of text and attachments; a selection that is the target of a non-empty draft is refused with a notice, as for `e`. Dismissal acts on what is read together: a whole chat stack, or a single email. Selection then moves to the next message below (else above). With an empty composer in reply-first insert mode, the composer follows that explicit move. After any explicit filter change in reply-first mode, an empty composer replies to the selected visible message, or to nothing when the feed is empty; it never stays bound to a message the feed no longer shows. Dismissing a message with a reply in progress is refused with a notice; the draft is never discarded or redirected. Accepted sends dismiss the incoming item captured in the submitted target, plus the older messages stacked with it, independently of the selected row; newer messages that arrived while the reply was in flight stay unread and visible. Failed/uncertain operations retain the item. `d` retains its separate draft-discard confirmation. Dismissal and spam exclusion never release a draft lock. Include-hidden settings allow recovery without making those items eligible for automatic replies.

`f` cycles No focus, Work, Home and custom profiles. Each profile has an editable independent decision prompt. No focus shows everything, including spam, with filtered messages labelled. Inside a focus, filtering always applies first, then the profile's prompt. A message without a decision (AI off, pending or failed) stays in every focus unless the profile has local terms; demo fixtures keep their work/personal tag. Inside a focus, locally, mail from or replying only to no-reply addresses and provider-marked automated senders are hidden at once. With AI decisions on, messages a person did not write, messages not meant for the owner or a group they are in, and promotions are hidden once their decision arrives. Unanswered and failed decisions never hide anything (unless `ai.hold_pending`). The header shows "N hidden"; inside a focus, `S` shows them in place, each labelled with its reason (no-reply, automated, promotional, not from a person, not for you), and hides them again. Filtered messages are never the automatic reply target, even with focus off. Hidden messages and messages outside the focus never count as new arrivals and are never dismissed, marked read or changed at the provider. AI has no send or draft authority and requires global/per-account opt-in. Pending/failure fallback remains accessible.

For the initial personal Discord experience, new DMs join the shared feed in arrival order and support quick reply. Older conversation history is phase 2 (or phase 3 if needed), fetched only when the owner explicitly opens a chat and requests/scrolls older pages. Show cached context while loading; stale history results cannot switch conversations or redirect a draft. Backfill must not reorder the shared arrivals, generate fresh alerts, or mark all older messages read merely because they loaded. The optional notification bridge currently provides only captured previews; `v` requests the official DM list, requiring manual conversation selection. There is no full history, conversation-specific handoff, in-app reply or remote read sync in that mode. Preserve this capability gap in the UI and plan.

## Implemented reply-first flow and shared service · 18 October 2026

`reply_first` defaults true and is editable through TUI/CLI. In insert mode, Up/Down is an explicit save-and-switch action, not passive browsing: park the complete current draft, select the adjacent visible incoming item, restore its own draft/attachments and remain in insert mode. Left/Right changes the source and performs the same explicit preparation; Alt+Left/Right moves the text cursor and Alt+Up/Down moves within multiline text. Normal letters, including j/k, remain input. Pending and uncertain sends reject switching. Escape enters the separate navigation mode for browsing/commands without releasing a lock. With `reply_first = false`, original cursor arrows and explicit `s` remain available.

The offline service holds one drawer editor lease; readers never take it. Settings, read markers, drafts and mock outbox transitions commit through its single owner. IPC uses versioned length-framed JSON, same-user checks and bounded payloads/queues. Reader open marks only the selected item seen under policy; it never selects a reply recipient. Readers receive current snapshots as revisions change. A client with an old revision obtains a fresh snapshot; stale composer edits are rejected. The bounded change ledger signals when a cursor needs full resynchronization.

A disconnected drawer keeps cached content, freezes editing/sending and writes private recovery state. Closing an acknowledged editor does not stop the owner or its mock send. On restart, interrupted submission becomes uncertain; an explicit mock reconciliation is required. Recovery imports only unsent active/saved drafts with matching targets, does not overwrite conflicting work, and never replays a send. Actual Ghostty focus and graphics behaviour remains unverified.

## Live service · 18 October 2026

Live profiles start empty and are permanently separate from fixtures. LIVE/SEND labels replace simulated-send labels, and sending requires the account's enabled/allow_send flags. The service persists the immutable operation before scheduling a provider worker; readers and drawers never own provider/AI sessions. Account identity changes cannot reuse old draft or remote-read authority. Live mode refuses fixture arrivals, fake attachments and mock reconciliation. After checking an uncertain outcome in the original client, the owner can explicitly report accepted/not-sent through service resolve or the drawer's :resolve command; the latter preserves the draft and unlocks deliberate retry. Disconnect/crash never retries an operation automatically.

The service starts inactive without an editor, focuses conservatively when the drawer attaches, and becomes inactive after its editor disconnects. Empty inactive context follows the newest eligible communication; focused reading and locked/saved drafts retain their context. The active recent feed is bounded; selected items, reply targets, saved drafts and pending read changes are pinned. Shared readers keep a cached message if it leaves that window. Teams read dots remain local; iCloud/Microsoft mail dots synchronize remotely under policy. Current provider/setup limits are recorded in LIVE_SETUP.md.

## Personal Discord QR setup

The owner selected personal QR sign-in on 18 October. :auth ACCOUNT opens a modal code/approval/identity/keyring flow; standalone account auth uses the same screen. The modal owns keyboard and mouse input, retains the underlying draft, clears expired/cancelled codes, supports retry and verifies/pins the user ID. Esc cancels before commit; a keyring write explicitly started by Enter must finish before close. No token is shown or serialized. Success explains that enabling the account starts new-DM reception and allow_send separately permits replies. Resize prompts replace a QR that cannot fit; never crop its quiet zone.

Default launch starts a missing live owner and presents account setup for an empty profile. : commands work directly in settings. Reopening reuses the existing service and draft lease. Explicit custom sockets and --no-start-service remain connect-only.

## Production update handover

`:update` requests a background stable-release check without changing the composer. Managed owners also check periodically and on drawer launch. A staged update pauses input only after queued edits/settings have flushed; active QR/credential setup defers acknowledgement. All frontend leases must acknowledge or disconnect, and active sends must finish, before service exec. Reconnect rehydrates the durable draft and frozen recipient without replaying mutations/sends. Managed frontends exec the installed version after reconnect with terminal restoration first. An older unacknowledged client defers handover until closed. See [updates](UPDATES.md).

## Responsive settings workspace

Settings use the full available height, including tall narrow drawers and expanded terminals. They are pages rather than one long list: an overview links to Accounts, General, Feed & composer, Focus profiles, AI decisions and Filtering, each with a one-line summary. Accounts lists every account by display name with a status marker (● connected, ◌ connecting, ○ disconnected, ! problem) plus text, provider and unread count, then + Add account. Each account has its own page: status, identity and cached/unread/latest statistics above Display name, Sign in (again), Connect/Disconnect, Allow replies, only that provider's fields, Allow AI decisions and Remove account…. Each profile also has its own page. A breadcrumb title shows the path; windows at least 96 cells wide add a clickable page list on the left. Field labels are readable names; the selected field's note shows its `config` key for CLI parity. Field names use the main text colour, values the secondary accent, and explanatory notes muted text; selected rows use a continuous full-width surface highlight. Wide windows align name/value columns; narrow windows stack them and wrap by Unicode display cells. Long/multiline values show a bounded preview with an ellipsis/newline marker; Enter edits the full value.

Enter opens a page, runs an action or edits a value in place: the row becomes an input with the cursor in it while the rest of the page stays visible. Single-line values save with Enter; multiline prompts/terms wrap at words inside the row, Enter adds a line and Ctrl+s saves; Escape cancels. A rejected value keeps the row editing with the reason in the notice line. Credentials entered from an account's Sign in (or Store API key) row are typed into that row as dots. `:spam prompt` and `:profile prompt ID` open the page holding that row, already editing. Left/Right (h/l) only change choices or follow links; Left on a link or action goes back and never runs an account action. Escape or Backspace returns to the parent page; Escape on the page settings were opened at (overview from `,`, Accounts on first launch, an account after `:account add`) returns to the drawer, and `q` always closes. Tab/Shift+Tab move between sections. A second click follows a link; account actions need the keyboard. Removing an account opens a confirmation page and is refused while a send from that account is pending or uncertain.

The add-account form keeps provider, ID and display name local until Create. The ID defaults to the first free `provider`, `provider-2`, … so several accounts of one provider are routine; an ID the owner typed survives provider changes. Created accounts stay disconnected with sends off and select Sign in. The ID is permanent because credentials, drafts and cursors are keyed by it; renaming changes only the display name, which the feed (e.g. `Discord · Son`) and composer use to tell same-provider accounts apart. Captured notification items keep their read-only label.

Keep the selected field visible through scrolling, resize and account-list changes. Up/Down and j/k select; PageUp/PageDown move by the visible field count, Home/End select the first/last field, and wheel/click select settings without changing a communication or reply target. Settings viewport state stays local to the window. Use the shared typed registry for all edits. First-account guidance and settings-specific controls remain visible, without a fixed three-entry limit.

## Account command entry

Settings accept : directly; composer insert mode requires Escape then : so normal reply text remains literal. Tab/Shift+Tab cycles matches for the typed prefix, including account providers, configured authentication IDs and custom profiles. Unknown commands and supplied IDs survive Tab unchanged. Completion never inserts a placeholder account ID.

Pasting a full colon-prefixed command into settings/navigation opens command entry; pasting it into empty command entry also accepts the prefix. Newlines are flattened and Enter is required to execute. Escape returns to the originating settings/navigation screen. Settings stay visible behind command entry, long input scrolls horizontally with its cursor, and adding an account selects its fields with a sign-in notice. Setup validation uses the shared typed settings path; account creation alone never enables connections or sends.

## Initial personal Discord transport · v0.3.0

QR sign-in stores a session; :account enable ID separately starts new-DM reception and allow_send separately permits text replies. Updates preserve both opt-ins. :connections reports disabled/connecting/connected/auth/identity/rate-limit states. Enabling an unsigned account reports missing QR setup without looking up credentials. One daemon-owned per-account Gateway job maintains heartbeat/reconnect/resume independently of REST replies; a bounded queue reconnects from its last enqueued sequence on overflow. No token, Gateway session ID or unrelated guild/relationship payload is included in IPC/session snapshots.

New personal/group DMs join the shared feed by local arrival order. Stable remote identity deduplicates replays and preserves the first arrival time. Own outgoing messages and guild traffic are excluded. Edits/deletions update captured items without changing draft text or recipients; deletions invalidate reply authority. Group recipients are rechecked before POST, and the immutable outbox operation determines the reply channel/message. An ambiguous response stays uncertain, with no automatic resend. Accepted replies dismiss their captured incoming item and the older messages stacked with it, never newer arrivals. Read dots are explicitly local. Older history, remote read acknowledgements and catch-up after a fresh/non-resumable session remain future work; do not claim earlier unseen DMs will appear on upgrade.

## Help

`?` and `:help` open the controls as sections (Reply, Browse, Mouse, Reader, Commands) of key and description rows. With room, keys sit in an aligned column and descriptions wrap beside them; in a narrow drawer, or for long command keys, the key takes its own line and the description is indented beneath. Demo-only controls (fixtures, simulated arrivals, mock outcomes) are left out of live help, and live-only ones (`:connections`, `:resolve`) out of demo help. The footer shortens to "Esc close · ↑↓ PgUp/Dn scroll" when narrow.
