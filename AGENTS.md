# AGENTS.md

Project guidance for AI agents working on the livtet-desktop codebase.

## Pointers

- Verify the code using `mise test` and `mise lint`. Examine the other tasks
  defined in [mise.toml](./mise.toml) to determine what should be run when.

## Skills

Project-specific skills live in `.agents/skills/`. When a task matches a
skill's description, load it before doing anything else. To add a new skill:

### Available skills

- **tauri-mcp-automation** — load when driving the desktop app via the Tauri
  MCP bridge. Covers filling `<wa-input>`/custom elements, clicking buttons,
  verifying reactive state, and the bridge's `execute_js` script-wrapping
  quirk.
- **adr** — load when making or revisiting architectural decisions, or
  changing IPC, persistence, or load-bearing code. Records in `doc/adr/`
  via `adr-tools`.
- **glossary** — load when naming UI, IPC, or domain concepts, or when a
  term is ambiguous. Single source in `doc/GLOSSARY.md`.
- **release-notes** — load when drafting release notes or summarizing
  changes for a release.
