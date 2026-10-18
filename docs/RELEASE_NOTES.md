Simpler settings, several accounts per provider and a readable feed.

- Settings are pages: an overview, global sections (General, Feed & composer, Focus profiles, AI decisions, Spam filtering) and one page per account. Esc goes back; Tab cycles sections.
- Each account page shows status and statistics with rename, sign in again, connect/disconnect, reply/AI permissions and confirmed removal. + Add account suggests a free ID, so several accounts of one provider are easy. CLI: `d-hub account add PROVIDER [ID] --name NAME`, `d-hub account rename ID NAME`.
- Consecutive messages from one sender and conversation stack under one heading while adjacent in the current view. Each shows up to three lines of its text instead of "Direct message"; live Discord is no longer labelled demo.
- Selecting a chat stack previews every message with its time; emails show the sender address and full local date. `o` reads the whole stack or email.
- Ctrl+E dismisses without replying, even while typing (a whole chat stack, or one email). A reply in progress is never discarded or redirected.
- Viewing a stack marks all of it read; an accepted reply dismisses the answered message and the older ones stacked with it, never newer arrivals.
- Times are shown in local time (previously UTC for live messages). The composer names the person you are replying to instead of a Discord user ID.

Existing configuration and accounts carry over unchanged; renaming an account never changes its sign-in, drafts or AI decisions. Teams/iCloud/Office 365 real-account acceptance, Discord history/catch-up, attachments and desktop alerts remain unfinished. No license has been selected.
