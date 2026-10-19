The feed shows what needs you.

- Mail sent from a no-reply address, or whose replies would only reach one, is hidden right away. So are senders the provider marks as automated (auto-generated mail, Teams apps, Discord bots).
- With AI decisions on, Jev or OpenAI Decisions also checks whether a person wrote each message and whether it is meant for you or a group you're in, alongside the promotion check. Messages that fail are hidden; informational messages from people stay. A message whose decision hasn't arrived, or failed, stays visible.
- The header shows how many messages are hidden. `S` shows them, labelled with the reason, and hides them again.
- New Settings › Filtering page (also `config`): `spam.no_reply`, `spam.for_you` and `spam.for_you_threshold` (default 0.35, leaning towards showing).
- To turn on AI filtering: store the key with `:auth ai`, set `ai.enabled`, and turn on each account's AI decisions.

Decision quality on real mail is untested; adjust the threshold on your own messages. Real-account acceptance for Teams, Office 365 and iCloud is still pending the owner's sign-in. No license has been selected.
