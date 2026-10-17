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

The daemon stores one draft per conversation/reply mode initially, with revisions and an editor lease. Opening a reader does not take the lease. If full-client editing is later enabled, transfer ownership explicitly and make other editors read-only; reject stale writes. Preview windows can emit explicit “reply here” requests, but the drawer must preserve an existing draft and offer save-and-switch.

Ghostty focus events describe terminal focus, not necessarily all compositor visibility. Verify whether hidden, inactive and selected-window states can be identified reliably. If focus detection is unavailable, retain context conservatively and provide an explicit follow-latest toggle; never assume inactivity from elapsed typing time.

The Omarchy companion plugin may activate the drawer or explicitly change its focus profile. Activation alone is not feed selection or permission to retarget a draft. Profile changes obey the same state transitions as the TUI and CLI. Plugin reload, disable or shell restart does not release a draft lock or editor lease; normal client disconnect/recovery rules still apply. Uncertain window visibility remains conservative.

## Keyboard modes

Modes are navigation, composer insert, command entry and modal dialog. An optional Vim text-editing mode can be added later; Neovim-style feed navigation does not require implementing a full editor. Each mode shows its current state and relevant shortcuts.

| Action | Proposed default | Scope |
| --- | --- | --- |
| Move selection | j/k, Up/Down | Navigation |
| Cycle source filter | Left/Right | Navigation; text cursor movement in insert mode |
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

Mouse selects rows, scrolls panes, follows links through explicit actions, focuses composer and removes attachment chips. Hover should not retarget a reply. Scrolling alone should not change the target. Never let a click on a remove chip fall through into send.

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

The header shows one active source filter, cycled with Left/Right in navigation mode: All, iCloud, Discord, Teams, Email and Office 365, wrapping in both directions. Email includes both mail providers. Source filters intersect with focus and search within the same feed. Filtering may change the previewed item, but cannot retarget an existing draft. A hidden empty target is cleared while focused; explicit message selection prepares a new reply. Inactive following uses the latest incoming item eligible under both source and focus. Empty combinations show a visible no-matches state.

Outer padding is configurable per axis and defaults to zero. `padding_horizontal` applies equally to the left and right edges; `padding_vertical` applies equally to the top and bottom. Internal composer and dialog spacing remains separate. Layout and mouse hit regions use the same inset area; padding changes must preserve drafts and reply targets.

## Full preview window

`d-hub preview <opaque-message-id>` connects to the same daemon and opens a reader directly when run inside a terminal. The drawer's open-preview action launches Ghostty with that command as an argv array. Reader IDs must not be arbitrary command text or filesystem paths.

Reader provides sender/account/recipients, subject, complete body, quoted history, attachment list, local search and optional summary. Keep summary visibly distinct from original text. Full-preview functionality is an early milestone; advanced folder/channel navigation belongs to later full clients. Reuse a reader window where supported, with a safe new-window fallback. Closing it leaves the drawer and daemon running.

## Read, dismiss and profile filtering

Solid ● means unread; hollow ○ means read. Explicit selection/open marks read by default; `u` toggles it. Background preview/filter changes do not mark an item read. Remote-capable messages enqueue revision-checked read changes; pending local changes survive reconnect and ignore stale acknowledgements. Until real transports exist, report local state and queued intent honestly.

`e` dismisses/restores the selected item; `D` includes dismissed messages. Accepted sends dismiss only the incoming item captured in the submitted target, independently of the selected row. Failed/uncertain operations retain the item. `d` retains its separate draft-discard confirmation. Dismissal and spam exclusion never release a draft lock. Include-hidden settings allow recovery without making those items eligible for automatic replies.

`f` cycles All, Work, Home and custom profiles. Each profile has an editable independent decision prompt. A global spam/promotion policy applies before focus filtering, including in All; `S` includes matches for inspection. AI has no send or draft authority and requires global/per-account opt-in. Pending/failure fallback remains accessible.

For the required personal Discord experience, new DMs join the shared feed in arrival order and open full conversation history with quick reply. The optional notification bridge currently provides only captured previews; `v` requests the official DM list, requiring manual conversation selection. There is no full history, conversation-specific handoff, in-app reply or remote read sync in that mode. Preserve this capability gap in the UI and plan.
