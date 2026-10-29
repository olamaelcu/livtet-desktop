# 44. Theme the desktop from livtet-branding's CSS layer

Date: 2026-10-29

## Status

Accepted

## Context

`@olamaelcu/livtet-branding` was a declared dependency that nothing imported
(its tokens, the "Modern Card Catalog" Web Awesome theme, the logo, the
Android/iOS artefacts all travelled through `pnpm install` for no effect). The
app looked like Web Awesome's stock theme with the Livtet fonts bolted on:
`web/app.css` imported Web Awesome's default theme and then re-declared the
three font families in a local `:root` block.

The package's pinned commit (`6c429ab`) ships `src/app.css`, `src/themes/`,
`src/lib/tokens/*.css` and `tokens.json`, but not `src/components/` and not
`dist/`, while its `exports` map advertises both (`./dist/index.js`, and
`src/lib/index.ts` re-exports `Logo.svelte`/`ThemeToggle.svelte`). So the CSS
subpaths resolve and the JS entry does not.

The app had almost nothing to migrate: across `web/lib` and `web/routes` there
were exactly two literal colours, both in `BookCard.svelte`'s placeholder cover
gradient. Everything else already read `--wa-*` custom properties.

## Decision

Use the package **as CSS only**, imported from `web/app.css` in this order:

```
webawesome.css        (base)
themes/default.css    (wa-theme layer, stock values)
@olamaelcu/livtet-branding/app.css   (wa-theme-overrides tokens + wa-theme)
native.css
```

The two layers are what make this safe: the generated token files write into
`@layer wa-theme-overrides` and the theme into `@layer wa-theme`, the same layer
Web Awesome's `default.css` writes to, so a later import wins by layer position —
no `!important`, no specificity races, and nothing to add to the markup (the
tokens target `:where(:root)` as well as `.wa-theme-livtet`/`.wa-palette-livtet`).
That ordering is pinned by `web/tests/browser/theme.browser.test.ts`, which
asserts the brand ramp value, the focus-ring mapping and both colour schemes.

`app.css` keeps one token override: `--wa-font-family-heading` names the static
`Lora` family, but only `@fontsource-variable/lora` is installed and it registers
as `"Lora Variable"`, so the app re-declares the heading stack on that name.
Body and code families come from the package unchanged.

`BookCard.svelte`'s placeholder gradient now uses
`--wa-color-surface-lowered`/`--wa-color-surface-raised`, so the one hardcoded
surface follows the scheme.

Dark mode needs no code: Web Awesome's runtime already puts `wa-dark` on `<html>`
when the platform preference is dark (verified in the running WebView, whose
`<html>` carries `wa-theme-default wa-palette-default wa-brand-green wa-dark`),
and the brand layer defines the dark block for `.wa-dark`/`.wa-invert`. Before
this change that class selected Web Awesome's stock dark palette; now it selects
the brand one.

## Consequences

Easier: one import re-tints the whole UI — brand fills, surfaces, text, links,
focus rings, semantic ramps, plus the theme's rounding/shadow/transition tokens;
a design change upstream is a pin bump rather than edits across components; the
dependency now earns its place, so the README's claim about brand tokens is
true.

Harder: the theme is CSS the app does not own, so a pin bump can move the look
without touching this repository — the guard test only pins the wiring, not the
palette; and the package's JS entry stays unused, which means `Logo`/`ThemeToggle`
from it cannot be adopted without bumping to a commit that ships `dist/` (and
until then `src/lib/index.ts` would fail to resolve, since the pinned tarball has
no `src/components/`).
