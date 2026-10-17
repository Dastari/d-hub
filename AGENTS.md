# Instructions for agents working on d-hub

Read README.md, HANDOFF.md and docs/INTERACTIONS.md before implementation. The owner is dastari and the application and executable are named d-hub.

## Relevant skills

Project skills live in `.agents/skills`; see docs/AGENT_SKILLS.md for selection and provenance. Read the relevant SKILL.md when working on that domain. The project location, Rust choice and docs/CONFIGURATION.md contract take precedence over generic CLI skill defaults. Do not relocate the project or add raw API/token-argv features just because a template suggests them.

## Product invariants

- One chronological communications feed. No source tabs. Focus profiles and filters operate within the feed.
- The persistent reply composer changes context on explicit selection only when no draft exists. Any text or attachment locks account, recipient and conversation.
- Background arrivals, AI results, filter changes and preview windows must never redirect an active draft or send it to another recipient.
- Empty composer follows the latest eligible incoming communication while the drawer is inactive. Do not change an empty composer unexpectedly while the user is actively reading.
- Unicode, truecolor, image previews and an AI-harness-style composer are core design goals. Keep text and controls usable when terminal graphics are unavailable.
- Arrow keys, mouse and Neovim-style navigation coexist. Do not consume normal typing as navigation in insert mode.
- All persistent user settings have both TUI and CLI access. Share one typed schema, validation and persistence path.
- Notification and sound policy respects Omarchy DND. Never fake success for an unsupported provider capability.

## Implementation discipline

- Start with the offline demo and a deterministic composer state machine. Follow docs/PLAN.md in vertical slices.
- This handoff contains no code. Dependency versions, API permissions and crate compatibility must be verified when scaffolding.
- Use fake messages and stub sends in automated tests. Real account access and sending require an explicit setup/testing step by the owner.
- Store secrets through a credential store; never commit tokens, message caches, attachments or real message fixtures.
- Launch subprocesses with argument arrays; never interpolate message content into shell commands. Sanitize untrusted terminal control sequences.
- Keep one background owner of sync, sends, notifications and data. Multiple windows must not duplicate any of these.
- Follow any parent instructions on the development machine. Desktop config edits require the Omarchy skill where available; no desktop config change is needed to start the demo.
- Do not create a remote repository, choose a license or deploy anything without the owner's direction.
- Test meaningful state transitions, send failures, cache migrations and provider contracts. Record what was actually verified.
