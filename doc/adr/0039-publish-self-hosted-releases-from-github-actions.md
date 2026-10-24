# 0039: Publish self-hosted releases from GitHub Actions

## Status

Accepted.

## Context

- Tauri builds `deb`/`rpm`/`AppImage`/`dmg` natively but has no Flatpak, Homebrew, or repository publishing.
- The repo has no release workflow, and `package.json`'s build scripts point at a stale config path (`tauri/tauri.conf.json` does not exist).
- macOS notarization requires an Apple identity; Flatpak needs a runtime and a signed OSTree repository.
- The repo currently has no GPG key for APT/Flatpak signing, no Homebrew tap, and GitHub Pages is disabled.

## Decision

- Use a tag-triggered GitHub Actions workflow (`.github/workflows/release.yml`) that runs on every `v*` push and on manual dispatch.
- Version is taken from `crates/livtet-desktop/tauri.conf.json`; the workflow validates that the pushed tag matches this version.
- Linux bundles (`.deb`, `.rpm`, `.AppImage`) and a Flatpak `.flatpak` bundle are produced on Ubuntu 22.04 and Ubuntu 22.04‑arm runners.
- macOS produces signed (when `APPLE_CERTIFICATE` secret is present) or unsigned `.dmg` files on `macos‑15‑intel` and `macos‑15‑arm64` runners.
- GitHub Release assets plus `SHA256SUMS.txt` are published.
- A signed APT repository and a signed Flatpak OSTree repository are published on the `gh‑pages` branch at `https://olamaelcu.github.io/livtet-desktop/`.
- A Homebrew Cask in `olamaelcu/homebrew-tap` is updated automatically from each release.
- A shared GPG key (`PACKAGES_GPG_KEY` secret) signs both the APT `InRelease` and the Flatpak OSTree repo.
- Apple signing secrets (`APPLE_CERTIFICATE`, etc.) are optional; their absence results in unsigned DMGs but the cask still installs them.

## Consequences

- Installs and upgrades come from the system package managers (APT, Homebrew, Flatpak).
- The pipeline depends on the package signing key, the Homebrew tap token, the existing SOPS age key, and (optionally) the Apple signing secrets, plus GitHub Pages being enabled.
- The `gh‑pages` site must stay under 1 GB, so the publish job prunes old `.deb` files per architecture and Flatpak history to `--prune-depth=1`.
- The macOS Intel build depends on the `macos-15‑intel` runner because `crates/livtet-desktop/build.rs` builds sidecars for the host triple.

## Files affected

- `.github/workflows/release.yml` – new; the entire pipeline.
- `packaging/flatpak/net.olamaelcu.livtet.yml` – Flatpak manifest.
- `packaging/apt/apt-ftparchive.conf` – APT repository configuration.
- `packaging/homebrew/livtet.rb.tmpl` – Homebrew Cask template.
- `scripts/set-version.mjs` – version‑setter for `package.json` and `tauri.conf.json`.
- `doc/adr/0039-*.md` – this decision record.

## Verification steps (summarised)

1. Local Linux bundle build and sidecar check.
2. Local Flatpak bundle verification with `ldd`.
3. End‑to‑end release push and asset checklist.
4. APT repository verification via Docker.
5. Flatpak repository install and `ldd` check.
6. Homebrew Cask install and audit.
7. GitHub Pages files (`InRelease`, `livtet.flatpakrepo`) accessibility.