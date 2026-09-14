---
name: adr
description: Use when making or revisiting architectural decisions, adding ADRs, or changing IPC, persistence, or load-bearing code.
---

# ADR

Record architectural decisions in `doc/adr/` using `adr-tools` (`NNNN-kebab-case.md`).

## Workflow

1. List `doc/adr/` for the next number and existing context.
2. Read only ADRs relevant to the current task.
3. Create with `adr new <title>`, then fill in Context/Decision/Consequences.
4. Amend in place for clarifications; supersede by adding a new ADR that links the old one and noting `Superseded by N` + status in the old file.

## Format

Match existing files: `# N. Title`, `Date:`, `Status`, `Context`, `Decision`, `Consequences`.

Keep each section short. State what becomes easier/harder. Link follow-up ADRs by number and relative path.

## Rules

- One decision per file. No FDR sweep, no index file — the directory listing is the index.
- Discuss breaking changes to IPC (`tauri/src/commands/`, `web/lib/bindings.ts`), persistence, or `AppState` with the user first.
