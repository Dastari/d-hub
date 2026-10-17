---
name: d-hub-microsoft-graph
description: Implement d-hub's Office 365 mail and Teams adapters with Microsoft Graph, delegated OAuth, sync, replies and attachment flows. Use for Microsoft account integration and Graph permission troubleshooting.
---

# Microsoft Graph integration

Read [integration design](../../../docs/INTEGRATIONS.md). Separate Office 365 mail, Teams chats and Teams channels: they do not share one universal permission set, unread model or file-upload path.

## Establish the supported contract

Before implementing an operation, open its current official Microsoft Graph v1.0 endpoint page and record method, permissions/account types, pagination, response, throttling and attachment semantics. Use beta only for a documented unmet requirement with an explicit compatibility decision.

Starting references:

- [Mail overview](https://learn.microsoft.com/en-us/graph/outlook-mail-concept-overview)
- [Delegated permissions](https://learn.microsoft.com/en-us/graph/permissions-overview)
- [List chat messages](https://learn.microsoft.com/en-us/graph/api/chat-list-messages?view=graph-rest-1.0)
- [Large mail attachments](https://learn.microsoft.com/en-us/graph/outlook-large-attachments)
- [Throttling](https://learn.microsoft.com/en-us/graph/throttling)

Use delegated public-client OAuth suited to the tenant; verify flow availability and consent policy. Do not embed a client secret. Cache refresh credentials securely and handle revocation without locking the UI. Differentiate missing consent, unsupported account type, expired auth and network failures.

## Synchronisation

Follow nextLink/deltaLink as opaque provider cursors. Keep account/resource-scoped cursors and restart invalid deltas with deduplication and alert suppression. Honour Retry-After and throttle independently per adapter. Verify immutable identity handling when mail moves folders.

Never assume Teams exposes all unread state from the official client. Report local seen state honestly when needed. Define chat/channel threading and mention handling explicitly. Webhooks require a reachable callback; a local Tailscale-only daemon is not automatically reachable by Graph.

## Writes and files

Office mail replies, sendMail and attachment upload sessions have distinct lifecycle semantics. Teams attachments can reference OneDrive/SharePoint files and may need extra permissions. Do not substitute a mail attachment payload for a Teams upload flow.

Persist outbox operation/revision before writes. Treat accepted responses as acceptance, not proof of delivery. Reconcile ambiguous outcomes before resubmission. Test request translation, pagination, token expiry, rate limits and upload failures with fake HTTP responses. Live sends require an explicitly chosen test recipient/conversation.
