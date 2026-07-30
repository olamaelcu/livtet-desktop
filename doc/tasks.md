# Taskwarrior integration

This project uses [Taskwarrior](https://taskwarrior.org/) for in-repo task
tracking. Both humans (via the `task` CLI) and AI agents in opencode (via the
`taskwarrior-mcp` MCP server) read and write the same task list, scoped to this
repository.

## Why this exists

- A single, repo-local task list that travels with the codebase.
- An interface for opencode agents to track their own work and report status
  back to the user.
- No external service, no sync server, no auth — just files in the repo.

## Setup

First-time (one per clone):

```sh
mise install                  # installs taskwarrior 3.4.2 + taskwarrior-mcp 0.2.0
direnv allow .                # exports mise env vars into the shell
mise run task-init            # creates .taskdata/ if missing
```

Daily, the `task` CLI and the MCP server work without further setup.

## File layout

| Path                          | Purpose                                                       | Tracked? |
| ----------------------------- | ------------------------------------------------------------- | -------- |
| `mise.toml`                   | Declares the tools and sets `TASKRC`/`TASKDATA` via `[env]`   | yes      |
| `.taskrc`                     | Taskwarrior config + UDA definitions                          | yes      |
| `.taskdata/`                  | Taskwarrior 3.x database (Taskchampion SQLite)                | yes      |
| `.mise/tasks/task-init`       | Idempotent bootstrap that creates `.taskdata/`                | yes      |
| `.envrc.example`              | Template for the per-developer `.envrc`                       | yes      |
| `.envrc`                      | Local dev env (includes `eval "$(mise env -s bash)"`)         | no (per-developer) |
| `.opencode/opencode.json`     | Wires the `taskwarrior-mcp` MCP server                        | yes      |

The `TASKRC` and `TASKDATA` env vars are Tera-templated in `mise.toml` and
resolve to absolute paths under the repo root:

```toml
TASKRC   = "{{ [config_root, '.taskrc']   | join_path }}"
TASKDATA = "{{ [config_root, '.taskdata'] | join_path }}"
```

Both MCP server commands (taskwarrior and tauri) are wrapped in `mise x --`
so the env block is applied explicitly to each spawned process.

## CLI usage

```sh
task add "Refactor search-bar oninput cast" +web livtet_component:web due:2026-08-05
task list
task 1 done
task 1 delete
```

Tags (`+web`, `+urgent`) and the `livtet_component:` UDA are interchangeable
filters:

```sh
task list +web
task list livtet_component:platform
task list project:livtet-desktop
```

## MCP usage

Opencode sessions in this repo see the full `taskwarrior_*` MCP tool family:

Core:
`taskwarrior_add`, `taskwarrior_list`, `taskwarrior_complete`,
`taskwarrior_modify`, `taskwarrior_delete`, `taskwarrior_get`,
`taskwarrior_bulk_get`, `taskwarrior_annotate`, `taskwarrior_start`,
`taskwarrior_stop`.

Reporting:
`taskwarrior_projects`, `taskwarrior_project_summary`, `taskwarrior_tags`,
`taskwarrior_summary`, `taskwarrior_undo`.

Agent intelligence:
`taskwarrior_suggest`, `taskwarrior_ready`, `taskwarrior_blocked`,
`taskwarrior_dependencies`, `taskwarrior_triage`, `taskwarrior_context`.

All tools read and write the same `.taskdata/` the CLI uses.

## The `livtet_component` UDA

Defined in `.taskrc` to match the monorepo's sub-projects:

```ini
uda.livtet_component.type   = string
uda.livtet_component.label  = Component
uda.livtet_component.values = web,desktop,mobile,platform,data,opds,core
```

When creating a task scoped to one sub-project, set the UDA explicitly:

```sh
task add "Cover art from Hardcover" livtet_component:web +web
```

Agents should follow the same convention so tasks stay filterable.

## Troubleshooting

- `task --version` reports 2.6.2 instead of 3.4.2 — your shell isn't picking up
  the mise shim. Run `mise install` and ensure the shim directory is on PATH
  (typically via `mise activate` in your shell rc).
- `task list` returns "No matches" in this repo but shows tasks in another
  shell — you don't have `TASKDATA` set. Run `direnv allow .` or
  `eval "$(mise env -s bash)"` in the current shell.
- MCP server returns the wrong task list — opencode isn't running with the
  mise env. Confirm `mise.toml` declares `taskwarrior` and `pipx:taskwarrior-mcp`,
  and that the MCP command is wrapped in `mise x --`.
- Linux hosts without a running secret service (`gnome-keyring`, `kwalletd`,
  `keepassxc`): unrelated to Taskwarrior, but `keyring` calls elsewhere in the
  app will fail. This document doesn't cover that.

## Background

The full design rationale (alternatives considered, trade-offs, deferred
work) is intentionally not duplicated here. Refer to the merge commit
message and the linked GitHub issue when this integration was first added.