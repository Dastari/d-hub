Initial personal Discord DM transport.

- Receive new personal/group DMs into the shared feed in arrival order using the QR credential already in your keyring.
- Use :account enable ACCOUNT to start reception, :connections to check status, and :account disable ACCOUNT to stop. Updates never enable connections/sends automatically.
- Text replies use the immutable target and durable outbox; enable accounts.ACCOUNT.allow_send separately. No automatic retry of uncertain sends.
- Maintain heartbeat/reconnect/resume in the background, deduplicate replays, and preserve drafts/read state through edits/deletions.
- Recheck private/group recipients before replies and reject account identity mismatches.

Use :update in a managed installation. After updating an existing sign-in, enable the account and wait for Discord DMs connected, then ask for a new test message. DMs sent to the old build are not backfilled.

Read dots remain local. History, catch-up after a full stop/expired session, attachment transfers, desktop alerts and the Omarchy companion remain unfinished. Fake-server tests cover the protocol; actual Discord delivery/reply acceptance remains an owner setup check. This is the owner-selected unofficial personal-client route, not a supported bot integration. No license has been selected.
