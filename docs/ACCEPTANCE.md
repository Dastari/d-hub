# d-hub acceptance scenarios

These scenarios define observable outcomes. Automated tests should use deterministic fake clocks, focus events and providers; account and desktop checks require the actual environment. See the [verification record](VERIFICATION.md) for implemented prototype coverage and pending checks; model/PTY results do not establish actual desktop or provider acceptance.

## Composer correctness

| ID | Given and action | Expected outcome |
| --- | --- | --- |
| C01 | Empty inactive drawer receives newer eligible communication B after A | Returning shows B as target and accepts immediate typing |
| C02 | Empty focused drawer shows A; B arrives | A remains selected/targeted; new-item indicator appears |
| C03 | Empty composer; select B with j/k, arrows or click | Header/target becomes B consistently |
| C04 | Type in A, then select B, change focus profile and receive C | Draft remains targeted to A with original account/recipients; viewed item may change |
| C05 | Attach a file to A with no body, then receive B | A stays locked |
| C06 | Delete all body/attachments from a locked draft | Target stays locked until explicit release/discard (Delete, save-and-switch or an explicit filter change); background events keep it |
| C07 | Send A succeeds; B is newest; drawer then loses focus | Submitted draft clears; empty composer follows B; next focus is ready to type |
| C08 | Send fails, account disconnects, or referenced item disappears | Body/attachments remain; error prevents accidental send to another target |
| C09 | Open/scroll/search/close full preview while drafting | Draft content, target and revision remain intact |
| C10 | Double send shortcut or two clients request same operation | One provider submission; second action shows existing operation |
| C11 | Timeout occurs after possible provider acceptance | Uncertain status persists; no blind automatic retry after restart |
| C12 | Explicit save-and-switch from A to B, then return to A | Each draft restores with its own immutable target and attachment set |
| C13 | Focus hides target or sync imports old mail | Existing draft remains visible; backfill does not displace newer context |
| C14 | No eligible reply-capable item exists | Empty/unsupported target state; typing cannot send to an invisible prior recipient |

## Attachments and rendering

| ID | Action | Expected outcome |
| --- | --- | --- |
| A01 | Copy multiple files in desktop file manager and invoke rich paste | Separate chips show correct names/sizes; all lock to current draft |
| A02 | Paste screenshot/image bytes | New image attachment is staged and previewable |
| A03 | Paste a filename-looking text string through text paste | Text remains text; no unexpected attachment |
| A04 | Remove attachment, lose original file, exceed size or encounter failed upload | Clear recoverable status; remaining body/files persist; no partial unintended send |
| A05 | Download received files and copy to clipboard; paste elsewhere later | Valid local files remain available for clipboard lifetime |
| U01 | Resize drawer with emoji, combining marks, CJK text and long filenames | No panic or broken editor cursor/target header; truncation/wrapping is readable |
| U02 | Disable graphics support or use missing icon glyphs | Text fallback retains complete controls and attachment access |
| U03 | Preview HTML mail containing scripts, remote images and terminal escapes | Safe readable content; no scripts, automatic remote tracking requests or terminal control execution |

## Input and configuration

| ID | Action | Expected outcome |
| --- | --- | --- |
| K01 | Type j/k/g/?/: and slashes in insert mode; navigate outside it | Normal text stays text; navigation works only in the proper context |
| K02 | Use Ctrl+Enter, Shift+Enter and fallback send keys in installed Ghostty | Enter/newline/send distinguishable; unsupported key combinations have alternatives |
| K03 | Change keymap to an overlapping mapping | Conflict is visible before apply; original mapping recoverable |
| S01 | Edit a setting in TUI, inspect via CLI and restart | Same persisted value and validation; source precedence visible |
| S02 | Launch with a temporary CLI override | Persisted config unchanged; effective origin reports override |
| S03 | Invalid config/theme edit, cancellation or masked saved setting | Atomic rejection or cancellation; running draft remains intact |
| S04 | Export config and run JSON diagnostics | No secrets, real message bodies or attachment bytes leak |

## Integrations and lifecycle

| ID | Action | Expected outcome |
| --- | --- | --- |
| P01 | Reconnect, paginate, resync or invalidate cursor | Messages deduplicate; unread/local seen state stays distinct; no alert storm |
| P02 | One account throttles/expires credentials | Other accounts/UI remain usable; actionable status for affected account |
| P03 | Reply from two accounts to similar sender/thread names | Exact account, remote conversation and recipients are preserved |
| P04 | Teams/Discord resource lacks read, reply or attachment capability | Disabled action explains why; no false success or personal-token workaround |
| I01 | AI times out, returns refusal, or finishes for old content/profile revision | Local rules remain usable; stale result ignored; no draft mutation |
| I02 | Account disallows external AI processing | No content from that account is transmitted to AI providers |
| N01 | Enable Omarchy DND or cross a quiet-hours boundary | Sounds and popups suppressed according to the same explicit policy |
| N02 | Two windows connected; provider repeats/edits event | At most one intended alert; grouping/cooldown applies |
| N03 | Click notification while another conversation has a draft | Correct item opens; existing draft remains targeted and intact |
| R01 | Terminate/restart service or drawer during draft/attachment staging | Committed draft restores; incomplete blobs cleaned safely; no automatic resend |
| R02 | Reconnect client after missed events or attempt stale multiwindow save | Snapshot/event replay converges; stale revisions rejected |

## Omarchy companion plugin

Milestone 2a uses fictional messages and stub sends on the actual desktop. These checks do not require live accounts.

| ID | Action | Expected outcome |
| --- | --- | --- |
| O01 | Receive fixture items, disconnect/reconnect backend and reload plugin | Unread/connection indicator converges from the backend snapshot; stale/unavailable status is explicit |
| O02 | Activate drawer repeatedly from plugin with an existing draft, then hide/show it | Existing drawer is reused where supported; safe fallback connects to one daemon; body, attachments and complete reply target remain intact |
| O03 | Change focus profile through plugin while drafting, then inspect in TUI/CLI | Same shared setting/validation path; feed updates without retargeting or discarding the draft |
| O04 | Reload/disable plugin or restart Omarchy shell while backend and reader are running | Backend continues, committed draft restores unchanged and plugin reconnects when enabled; no duplicate provider work, sends or alerts |
| O05 | Run without plugin, remove it, or lack reliable compositor focus/visibility reporting | Demo and standalone TUI/CLI remain usable; application state is retained and automatic following stays conservative when focus is unknown |

## Verification record template

For each implemented milestone record command/test or manual steps, date, platform and terminal versions, result, fixture/account scope and unresolved limitation. Account tests use owner-authorised test recipients. Desktop tests must name the actual Ghostty/Omarchy environment; a headless build result does not establish clipboard, sound or window behaviour.

## Personal Discord requirement and feed policy

| ID | Scenario | Expected |
| --- | --- | --- |
| D01 | Owner receives a personal DM, especially from their son | New item joins the same feed in arrival order; channels/bot DMs are not a substitute |
| D02 | Reply to a new personal Discord DM, phase 1 | Correct account/conversation and locked recipient; accepted reply dismisses the answered item; historical backfill is not required; currently unmet |
| D03 | Optional notification mode is explicitly enabled | Capture matching desktop previews only; label missing history, in-app reply, remote unread and exact-conversation link; do not pass the phase 1 personal arrival/reply criteria based on this fallback |
| D04 | Explicitly open a chat, phase 2 (may defer to 3) | Show cached context and fetch a bounded recent page; older pages load on request/scroll with loading, retry and end states; no startup-wide history requirement |
| D05 | History loads after switching chats or while another reply is drafted | Reject stale conversation results, deduplicate overlap, preserve reader position and locked draft; no fresh feed arrivals/alerts or blanket read marking from backfill |
| F01 | Explicitly select/open an unread item | Hollow read dot persists; pending remote flag intent is revision checked; remote sync remains pending real adapters |
| F02 | Reply accepted while another item is selected | Dismiss the submitted target’s original incoming item, preserving the other item |
| F03 | Reply fails or acceptance is uncertain | Keep original item and draft; do not dismiss or blindly resend |
| F04 | Manually dismiss/restore, or include dismissed/spam | e/D/S work within source and focus filters; a locked draft keeps its complete target |
| F05 | Edit profile/spam prompt during an in-flight decision | Reject stale result, retain draft, classify again under current policy or use local fallback |

## Reply-first navigation and offline service

| ID | Scenario | Expected |
| --- | --- | --- |
| R01 | Type a reply, Up/Down to another message, type another, return | Each incoming message retains its own body/attachments; insert mode remains active |
| R02 | Two incoming messages share a conversation | Their reply targets and saved drafts remain distinct |
| R03 | Left/Right while replying | Cycle source, save current draft and prepare/restore a visible reply; no matches parks it with no hidden recipient |
| R04 | Alt+arrows / ordinary j/k while typing | Move the text cursor / insert literal characters; no source or message switch |
| R05 | Switch during submitting or uncertain outcome | Keep original draft/target and explain why switching is blocked |
| B01 | Start a second owner/editor, or edit from reader | Reject duplicate owner/editor and reader mutation; multiple readers remain usable |
| B02 | Reader opens another item while reply is drafted | Share seen state without changing reply target/body |
| B03 | Crash after submission; restart | Durable draft/outbox becomes uncertain; no automatic resend |
| B04 | Drawer closes during acknowledged mock send | Owner completes once, stores result and dismisses the original incoming item |
| B05 | Stale revision/composer/settings edit | Reject or rebase against a fresh snapshot; preserve newer work and queued local text |
| B06 | Socket disconnect with unacknowledged text | Freeze edits/sends, write private recovery; explicit restore refuses conflicts/pending sends and never replays operations |
| B07 | Legacy/malformed/future schema | Import valid JSON once without deleting it; reject invalid/future/foreign data without replacement |
| B08 | Old event cursor / oversized or unsupported IPC | Signal snapshot resync / reject bounded frame or version; other clients remain usable |
