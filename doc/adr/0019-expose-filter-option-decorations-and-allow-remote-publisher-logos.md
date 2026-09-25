# 19. Expose filter option decorations and allow remote publisher logos

Date: 2026-09-24

## Status

Accepted

## Context

`filter_options` returned `FilterOption { id, label }` only, so the filter axes
were text-only. The data needed for lightweight decoration already exists: the
`languages` table carries `code` and `flag_emoji` (see
[7. Normalize language codes with isolang during remote import](0007-normalize-language-codes-with-isolang-during-import.md),
which anticipated a UI flag), and the `publishers` table carries `logo_url`.
Tags, genres, subjects, and authors have only a name.

Rendering publisher logos means loading remote images, but the app's CSP set
`img-src 'self' asset: data:`, which blocks any external host.

## Decision

1. Add `flag_emoji: Option<String>` and `logo_url: Option<String>` to the
   `FilterOption` DTO. At most one is set, and only for the axis that owns that
   decoration: `languages` populate `flag_emoji`, `publishers` populate
   `logo_url`, every other axis leaves both `None`. The change is additive
   (nullable fields), so it does not break existing `filter_options` consumers.
   A tagged enum was rejected because specta-typescript 0.0.12 does not emit
   serde container tags for `#[serde(tag = ...)]`.
2. Widen the CSP `img-src` directive to `'self' asset: data: https:` so remote
   publisher logos load. Only images are relaxed; `connect-src`, `script-src`,
   and `font-src` are unchanged.
3. In the web UI, replace the per-axis checkbox list with a `wa-select
   multiple`, rendering the flag emoji as the option's start decoration and a
   `wa-avatar` (with an initials fallback on load failure) for publisher logos.
   Avatars render only when `logo_url` is present.

## Consequences

**Easier**: the filter panel conveys language and publisher identity at a
glance, and the multi-select scales better than a wall of checkboxes.

**Harder**: `img-src https:` lets the webview fetch images from any remote
host, so publisher-controlled `logo_url` values widen the image-loading attack
surface (mitigated by only ever rendering `img`/`wa-avatar`, never scripts).
The desktop webview now depends on two additional WebAwesome components
(`wa-select`, `wa-avatar`) being registered in `web/app.wa.js`; unregistered
`wa-*` tags render inert and no type check catches it.
