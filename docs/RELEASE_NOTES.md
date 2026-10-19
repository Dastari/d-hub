Focus decides what's filtered, and your messages are back.

- **Fixed: a focus no longer hides real messages.** Without AI decisions, Work and Home hid every live message. Messages without a decision (AI off, pending or failed) now stay visible in every focus.
- **No focus shows everything**, including spam and no-reply mail. Filtered messages are labelled (· no-reply, · promotional) but not hidden. The header says "No focus".
- **Inside a focus** (Work, Home or your own: `:profile add NAME`, `:profile prompt NAME`), spam, promotions, no-reply and automated mail are always hidden first. Then the focus prompt decides, once AI decisions are on (`:auth ai`, `ai.enabled`). `S` shows what was hidden.
- An empty feed separates "outside Home focus · f" from "in other sources · ←→". Switching focus shows a notice explaining what it does.
- The "↓N new" count only includes arrivals inside your focus that aren't filtered.

d-hub doesn't send desktop notifications yet. Decision quality on real mail is untested. Real-account acceptance for Teams, Office 365 and iCloud is still pending the owner's sign-in. No license has been selected.
