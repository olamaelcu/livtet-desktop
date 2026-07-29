# AGENTS.md

Project guidance for AI agents working on the livtet-desktop codebase.

## Skills

Project-specific skills live in `.agent/skills/`. This path is configured in `.opencode/opencode.json` via `skills.paths` (OpenCode's default discovery paths are `.opencode/skills/`, `.claude/skills/`, `.agents/skills/` — we use a custom folder so agent tooling for this project stays colocated with the code).

When a task matches a skill's description, load it before doing anything else. To add a new skill:

1. Create `.agent/skills/<name>/SKILL.md`
2. Use only lowercase letters, numbers, and single hyphens in the name (must match the directory name)
3. Frontmatter must include `name` and `description`; the description should start with "Use when..." and describe triggering conditions, not the workflow
4. Restart the session to pick up the new skill

## Available skills

- **tauri-mcp-automation** — load when driving the desktop app via the Tauri MCP bridge. Covers filling `<wa-input>`/custom elements, clicking buttons, verifying reactive state, and the bridge's `execute_js` script-wrapping quirk.
