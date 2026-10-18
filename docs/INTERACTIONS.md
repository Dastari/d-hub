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
| Locked draft | User deletes all text and attachments | Keep lock until explicit release/discard or send lifecycle completes; emptying input is not implicit permission to redirect |
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
| Scroll page | Ctrl+d/Ctrl+u, PageDown/PageUp | Feed or reader |
| Focus next/previous pane | Tab/Shift+Tab | Outside text completion/dialog-specific use |
| Reply/edit | i or r | Navigation |
| Exit input to navigation | Escape | Insert; preserves draft |
| Newline | Enter; also Shift+Enter where distinguishable | Insert |
| Send | Ctrl+Enter, fallback Alt+Enter | Insert, after terminal capability check |
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

INSERT · Ctrl+Enter Send
```

At very narrow widths, wrap safely and prioritise recipient, body and attachment count. At wider widths, expand preview naturally. Minimum usable dimensions and fallback message must be measured in the prototype. No account/source tabs appear at any width.

Prefer spacing and subtle surface colours to enclosing boxes. Message sources share semantic success/error colours and have distinct source colours for dots and selection, including Discord blurple. Use labels for source identity as well. Slow accent colour shifts and braille activity indicators provide motion without rearranging content. Reduced motion uses static colours and activity labels.

The header shows one active source filter, cycled with Left/Right in reply-first insert or navigation mode: All, iCloud, Discord, Teams, Email and Office 365, wrapping in both directions. Email includes both mail providers. Source filters intersect with focus and search within the same feed. Background filtering may change the previewed item but cannot retarget an existing draft. In reply-first mode, explicit source-arrow actions save the current draft before preparing/restoring a visible message’s reply; an empty result parks it and clears the recipient. A hidden empty target is cleared while focused; explicit message selection prepares a new reply. Inactive following uses the latest incoming item eligible under both source and focus. Empty combinations show a visible no-matches state.

Outer padding is configurable per axis and defaults to zero. `padding_horizontal` applies equally to the left and right edges; `padding_vertical` applies equally to the top and bottom. Internal composer and dialog spacing remains separate. Layout and mouse hit regions use the same inset area; padding changes must preserve drafts and reply targets.

## Full preview window

`d-hub preview <opaque-message-id>` connects to the same daemon and opens a reader directly when run inside a terminal. The drawer's open-preview action launches Ghostty with that command as an argv array. Reader IDs must not be arbitrary command text or filesystem paths.

Reader provides sender/account/recipients, subject, complete body, quoted history, attachment list, local search and optional summary. Keep summary visibly distinct from original text. Full-preview functionality is an early milestone; advanced folder/channel navigation belongs to later full clients. Reuse a reader window where supported, with a safe new-window fallback. Closing it leaves the drawer and daemon running.

## Read, dismiss and profile filtering

Solid ● means unread; hollow ○ means read. Explicit selection/open marks read by default; `u` toggles it. Background preview/filter changes do not mark an item read. Remote-capable messages enqueue revision-checked read changes; pending local changes survive reconnect and ignore stale acknowledgements. Until real transports exist, report local state and queued intent honestly.

`e` dismisses/restores the selected item; `D` includes dismissed messages. Accepted sends dismiss only the incoming item captured in the submitted target, independently of the selected row. Failed/uncertain operations retain the item. `d` retains its separate draft-discard confirmation. Dismissal and spam exclusion never release a draft lock. Include-hidden settings allow recovery without making those items eligible for automatic replies.

`f` cycles All, Work, Home and custom profiles. Each profile has an editable independent decision prompt. A global spam/promotion policy applies before focus filtering, including in All; `S` includes matches for inspection. AI has no send or draft authority and requires global/per-account opt-in. Pending/failure fallback remains accessible.

For the initial personal Discord experience, new DMs join the shared feed in arrival order and support quick reply. Older conversation history is phase 2 (or phase 3 if needed), fetched only when the owner explicitly opens a chat and requests/scrolls older pages. Show cached context while loading; stale history results cannot switch conversations or redirect a draft. Backfill must not reorder the shared arrivals, generate fresh alerts, or mark all older messages read merely because they loaded. The optional notification bridge currently provides only captured previews; `v` requests the official DM list, requiring manual conversation selection. There is no full history, conversation-specific handoff, in-app reply or remote read sync in that mode. Preserve this capability gap in the UI and plan.

## Implemented reply-first flow and shared service · 18 October 2026

`reply_first` defaults true and is editable through TUI/CLI. In insert mode, Up/Down is an explicit save-and-switch action, not passive browsing: park the complete current draft, select the adjacent visible incoming item, restore its own draft/attachments and remain in insert mode. Left/Right changes the source and performs the same explicit preparation; Alt+Left/Right moves the text cursor and Alt+Up/Down moves within multiline text. Normal letters, including j/k, remain input. Pending and uncertain sends reject switching. Escape enters the separate navigation mode for browsing/commands without releasing a lock. With `reply_first = false`, original cursor arrows and explicit `s` remain available.

The offline service holds one drawer editor lease; readers never take it. Settings, read markers, drafts and mock outbox transitions commit through its single owner. IPC uses versioned length-framed JSON, same-user checks and bounded payloads/queues. Reader open marks only the selected item seen under policy; it never selects a reply recipient. Readers receive current snapshots as revisions change. A client with an old revision obtains a fresh snapshot; stale composer edits are rejected. The bounded change ledger signals when a cursor needs full resynchronization.

A disconnected drawer keeps cached content, freezes editing/sending and writes private recovery state. Closing an acknowledged editor does not stop the owner or its mock send. On restart, interrupted submission becomes uncertain; an explicit mock reconciliation is required. Recovery imports only unsent active/saved drafts with matching targets, does not overwrite conflicting work, and never replays a send. Actual Ghostty focus and graphics behaviour remains unverified.

## Live service · 18 October 2026

Live profiles start empty and are permanently separate from fixtures. LIVE/SEND labels replace simulated-send labels, and sending requires the account's enabled/allow_send flags. The service persists the immutable operation before scheduling a provider worker; readers and drawers never own provider/AI sessions. Account identity changes cannot reuse old draft or remote-read authority. Live mode refuses fixture arrivals, fake attachments and mock reconciliation. After checking an uncertain outcome in the original client, the owner can explicitly report accepted/not-sent through service resolve or the drawer's :resolve command; the latter preserves the draft and unlocks deliberate retry. Disconnect/crash never retries an operation automatically.

The service starts inactive without an editor, focuses conservatively when the drawer attaches, and becomes inactive after its editor disconnects. Empty inactive context follows the newest eligible communication; focused reading and locked/saved drafts retain their context. The active recent feed is bounded; selected items, reply targets, saved drafts and pending read changes are pinned. Shared readers keep a cached message if it leaves that window. Teams read dots remain local; iCloud/Microsoft mail dots synchronize remotely under policy. Current provider/setup limits are recorded in LIVE_SETUP.md.

## Personal Discord QR setup

The owner selected personal QR sign-in on 18 October. :auth ACCOUNT opens a modal code/approval/identity/keyring flow; standalone account auth uses the same screen. The modal owns keyboard and mouse input, retains the underlying draft, clears expired/cancelled codes, supports retry and verifies/pins the user ID. Esc cancels before commit; a keyring write explicitly started by Enter must finish before close. No token is shown or serialized. Success says signed in with DM sync/replies pending. Resize prompts replace a QR that cannot fit; never crop its quiet zone.

Default launch starts a missing live owner and presents account setup for an empty profile. : commands work directly in settings. Reopening reuses the existing service and draft lease. Explicit custom sockets and --no-start-service remain connect-only.

## Production update handover

`:update` requests a background stable-release check without changing the composer. Managed owners also check periodically and on drawer launch. A staged update pauses input only after queued edits/settings have flushed; active QR/credential setup defers acknowledgement. All frontend leases must acknowledge or disconnect, and active sends must finish, before service exec. Reconnect rehydrates the durable draft and frozen recipient without replaying mutations/sends. Managed frontends exec the installed version after reconnect with terminal restoration first. An older unacknowledged client defers handover until closed. See [updates](UPDATES.md).
