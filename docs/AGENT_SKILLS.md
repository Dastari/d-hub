# d-hub agent skills

Skills are stored in `.agents/skills` so they travel with this project and can be discovered by compatible agents opened in the repository. If an agent does not discover this directory automatically, read the matching SKILL.md explicitly. A new agent session should discover the added skills; no global skill-directory changes are required.

| Skill | Use |
| --- | --- |
| d-hub-rust-tui | Ratatui/Crossterm UI, composer reducer, input modes and reader windows |
| d-hub-icloud-mail | iCloud IMAP/SMTP, MIME, unread sync and replies |
| d-hub-microsoft-graph | Office 365 and Teams OAuth, Graph sync, permissions and files |
| d-hub-discord | Owner-selected personal QR setup, private identity/session storage, DM phases and separate bot capabilities |
| d-hub-omarchy-ghostty | Window launch, clipboard, themes, notifications, DND and sounds |
| d-hub-ai-decisions | Jev/OpenAI adapters, focus questions, failure handling and summaries |
| cli-creator | Composable CLI contracts, diagnostics, structured output and packaging |
| openai-docs | Current official OpenAI API documentation and implementation guidance |

The six d-hub skills were authored for this project from its product contract and linked primary documentation. `cli-creator` and `openai-docs` were installed from `openai/skills`, paths `skills/.curated/cli-creator` and `skills/.curated/openai-docs`, from the upstream main branch on 17 October 2026. Their accompanying license/resources are preserved. They are a retrieved snapshot, not an automatic update channel.

## Project scope takes precedence

The language, project location and configuration contract are already selected. Generic skill suggestions must not relocate this project, change Rust to another runtime, require a universal raw-API escape hatch, put secrets on argv or install the CLI globally during the offline prototype. Follow project documentation for those decisions.

Skills guide work when relevant; they do not authorise live sends, account installation, token extraction, desktop resets or publishing. Read only the skills needed for the current slice rather than loading every provider manual for UI work.
