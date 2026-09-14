---
name: release-notes
description: Use when drafting release notes or summarizing changes for a release.
---

# Release Notes

Write one markdown page per release for users and operators. Describe what changes for them.

## Evidence

- Determine the target version from git tags or the user. Compare against the previous tag.
- Check commits, PR descriptions, and `CHANGELOG.md` if present. Include fixes not yet in the changelog. Check code or docs when reader impact is unclear.
- Stop and explain if the target release cannot be determined.

## Content

- Rank by reader effect, largest first. Describe each change once, in final form.
- Put required actions and compatibility breaks in `Upgrade Notes`.
- Group reader-relevant bug fixes under `Smaller fixes`. Omit internal refactors, tests, CI, and generated files unless they change visible behavior.
- Short factual sentences. No marketing claims, emojis, or pasted changelog dumps.

## Preservation

Treat existing notes as manually edited. Make targeted edits only. Restructure only on request.
