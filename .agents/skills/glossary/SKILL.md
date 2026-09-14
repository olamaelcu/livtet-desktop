---
name: glossary
description: Use when naming UI elements, IPC commands, or domain concepts, or when a term's meaning is ambiguous.
---

# Glossary

Maintain `doc/GLOSSARY.md` as the single source for project terms. One entry per concept, Chatto-agnostic.

## Tasks

- **Lookup:** Find terms case-insensitively. Return entry + section. If absent, suggest closest entries.
- **Add:** Check for duplicates against code, ADRs, and `AGENTS.md`. Propose definition + section for approval before writing.
- **Audit (default):** Check definitions and links against sources. Report stale, missing, duplicate, or misplaced entries. Never apply fixes without a request.

For audits, prioritize terms used in several ADRs or whose meaning changed. Propose at most ~10 missing terms.

## Entries

Bold term + short definition. Expand abbreviations on first use. One section per term: UI, IPC, Backend, Domain. Foundational terms first, not alphabetical.

Link to the ADR or code that defines the concept. Keep old names only if readers still need them for existing identifiers. If code differs from the canonical term, report the rename; a glossary task does not authorize code changes.
