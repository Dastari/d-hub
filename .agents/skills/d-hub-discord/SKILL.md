---
name: d-hub-discord
description: Design or implement d-hub's supported Discord bot integration, resource capabilities, message events and attachments. Use when assessing Discord channel or DM feasibility and coding its adapter.
---

# Discord integration

Read the Discord scope in [integration design](../../../docs/INTEGRATIONS.md). The owner requires personal DMs, especially messages from their son; channels are lower priority. Phase 1 is new arrivals in the shared feed plus quick replies. History is deferred to phase 2 (or 3 if needed), fetched on chat open and paginated on demand, with no initial bulk sync. Concord is a researched technical reference; see the integration design for pinned sources and access/licensing constraints. Do not treat historical backfill as an initial feed/reply gate.

Use [Discord developer docs](https://discord.com/developers/docs/intro) and [self-bot policy](https://support.discord.com/hc/en-us/articles/115002192352-Automated-User-Accounts-Self-Bots). Never implement personal user-token automation or extract a token from the desktop client. A bot cannot mirror arbitrary personal-account DMs.

## Define capability before implementation

Establish the authorised bot identity, guild/channel access, intents, message-content eligibility and sending/upload permissions. Display identity accurately: messages sent as a bot must not appear to be sent as the human owner. Do not declare personal-account client support based on a bot demo.

Map gateway events and REST history into stable account/resource/message IDs. Handle edits/deletions, reconnect/resume, duplicate events, pagination and rate limits. Keep server unread semantics separate from d-hub's local seen state. Old history does not emit fresh alerts.

Verify current attachment upload limits and required request shapes. Preserve upload and send errors without losing the locked draft. Reply relationships and channel/thread IDs must come from the frozen target snapshot.

Automated tests use synthetic gateway/HTTP fixtures. Real bot installation, permissions and test messages are explicit owner integration steps. If required personal DMs cannot be supported, report the limitation and ask for a product choice instead of silently substituting a notification scraper.
