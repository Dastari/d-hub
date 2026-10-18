Getting Teams, Office 365 and iCloud ready for real accounts.

- New read-only connection check: `d-hub account test ID`, or **Test connection** on an account's settings page. Nothing is imported, marked read or sent.
  - iCloud: signs in, lists your folders, examines the synced ones and signs in to the send server without sending.
  - Office 365: shows the signed-in identity and granted permissions, folders with unread counts, and runs the first-sync query.
  - Teams: lists your 50 most recent chats by person, with the chat IDs to use in `resources`.
- New Office 365 and Teams accounts start with the built-in work app registration and tenant; no Entra setup is needed for that directory. Accounts created earlier keep their saved values (docs/LIVE_SETUP.md has the IDs).
- iCloud replies are sent as UTF-8, so accents and emoji arrive intact.
- Office 365: the first sync no longer sends a date format Graph rejects, and an expired sync position restarts only that folder instead of discarding mail fetched from the others.
- Failures include the server's reason (Microsoft AADSTS and Graph error codes, IMAP responses) instead of a bare HTTP status.
- A reply that never reached the server now fails with the draft kept, instead of being marked uncertain.
- `d-hub config set` and `d-hub account …` change the running service's settings by default. If the service is stopped they refuse rather than editing a file it never reads; `--config PATH` still edits the first-run seed file.
- Teams chat IDs copied from the Teams web address are decoded automatically.

Real-account acceptance for Teams, Office 365 and iCloud is still pending the owner's sign-in. Known gaps: iCloud reconnects on every poll; reactions or edits can resurface old Teams messages; quoted Teams replies show a false files marker; Office 365 replies may omit the quoted original. No license has been selected.
