# AGENTS.md

Project guidance for AI agents working on the livtet-desktop codebase.

## Skills

Project-specific skills live in `.agents/skills/`. When a task matches a
skill's description, load it before doing anything else. To add a new skill:

## Available skills

- **tauri-mcp-automation** — load when driving the desktop app via the Tauri
  MCP bridge. Covers filling `<wa-input>`/custom elements, clicking buttons,
  verifying reactive state, and the bridge's `execute_js` script-wrapping
  quirk.
