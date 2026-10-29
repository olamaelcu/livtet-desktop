# 43. Serve the package repositories from download.livtet.olamaelcu.net

Date: 2026-10-29

## Status

Accepted

## Context

ADR 0039 publishes the signed APT and Flatpak repositories to the `gh-pages`
branch, which GitHub Pages serves at `https://olamaelcu.github.io/livtet-desktop/`.
That host is not just a URL in a workflow: it is written into the two files
clients fetch — the `deb [signed-by=…]` line in `livtet.list` and `Url=` in
`livtet.flatpakrepo` — and it is what the README tells people to `curl` and
`flatpak remote-add`. Changing it later is a client-visible break, so the host
should be the one the project actually controls.

`olamaelcu.net` is delegated to DNSimple. Its `livtet` and apex records point at
the web application (`108.61.75.187`); `download.livtet` is unoccupied.

## Decision

The package repositories are served from `https://download.livtet.olamaelcu.net/`
and the pieces are kept in one place:

- A DNS **CNAME** record `download.livtet` → `olamaelcu.github.io` in the
  DNSimple zone (CNAME, not A: only apex domains use A records for Pages).
- GitHub Pages is configured with `cname=download.livtet.olamaelcu.net` and,
  once the certificate is issued, `https_enforced=true`.
- `.github/workflows/release.yml` carries `PACKAGES_URL` in its top-level `env`
  and derives both client-facing URLs from it, so the hostname is a single
  string rather than three literals.
- The publish job writes `CNAME` into the `gh-pages` branch on every run
  (`printf '%s\n' "${PACKAGES_URL#https://}" > pages/CNAME`). Pages reads the
  domain from that file, and the job rewrites the branch each release, so the
  file must be part of what the job writes; otherwise a Pages settings reset
  silently moves the site back to the `github.io` host.
- The README's Debian and Flatpak instructions use the custom host.
- The repository root document is `packaging/pages/index.html`, copied into the
  branch by the publish job (skipped when the tag predates the file, as an
  already-cut tag's tree cannot carry it). It is plain HTML with inline CSS and
  no external requests, so the page renders without JavaScript or a CDN.

Order matters: the DNS record must resolve before the custom domain is set,
because the Let's Encrypt certificate is provisioned against it, and Pages
refuses to enable a site whose publishing branch does not exist.

## Consequences

Easier: the published host belongs to the project, so a future Pages/GitHub
rename cannot invalidate the URLs baked into users' `apt` sources and Flatpak
remotes; the domain lives in one workflow variable plus one README block.

Harder: the old `github.io` URLs keep working only through GitHub's 301 to the
custom domain, and only while that hostname remains routed to this repository.
An install that pinned `olamaelcu.github.io/livtet-desktop` needs to re-point
when that fallback disappears — APT follows the redirect today, Flatpak follows
`Url=` as written. Adding or rotating the domain means editing `PACKAGES_URL`,
the README, and the Pages setting together, in that order, and re-running the
publish job so `livtet.list`, `livtet.flatpakrepo` and `CNAME` all move.
