---
name: d-hub-icloud-mail
description: Implement or debug d-hub's iCloud IMAP and SMTP adapter, MIME parsing, unread sync, replies and email attachments. Use for mail protocol work rather than Microsoft Graph.
---

# iCloud mail integration

Read the iCloud section of [integration design](../../../docs/INTEGRATIONS.md) and the outbox contract in [architecture](../../../docs/ARCHITECTURE.md).

Use [Apple's published server settings](https://support.apple.com/en-ie/102525) and current protocol references: [IMAP4rev2](https://www.rfc-editor.org/rfc/rfc9051.html), [Internet message format](https://www.rfc-editor.org/rfc/rfc5322.html), and [SMTP](https://www.rfc-editor.org/rfc/rfc5321.html). Actual server capability discovery governs optional IMAP extensions; do not assume iCloud implements every rev2 extension.

## Identity and sync

Key remote messages by account, mailbox identity, UIDVALIDITY and UID. Sequence numbers are not durable IDs. Handle UIDVALIDITY changes, expunges, moves, partial fetches and reconnects without duplicating feed items or alerts. Negotiate supported incremental/IDLE features, with bounded polling fallback.

Separate IMAP unread flags from local viewed/dismissed state. Fetch MIME metadata before bodies/attachments where useful. Verify multipart alternatives, encoded headers, content-ID images, attachment filenames, quoted replies and Unicode subjects with fictional fixtures.

## Sending

Use an app-specific password through a secret reference, TLS validation and the configured account identity. Build valid MIME and reply headers. Display exact sender and recipients, with explicit reply-all mode. Enforce attachment limits before network submission where possible.

Bind every send to a persisted outbox operation and frozen draft revision. SMTP timeout after acceptance is uncertain, not safe to retry automatically. Determine whether the server already saves Sent mail before adding an IMAP append; track append failure separately from sending failure.

## Validate

Use a fake/local mail server and synthetic messages for automated tests. Include reconnect, malformed MIME, flag changes, missing attachment bytes and uncertain SMTP completion. Live credentials and sending are explicit owner setup/testing steps; never embed personal mail or tokens in fixtures/logs.
