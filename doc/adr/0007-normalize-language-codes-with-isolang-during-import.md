# 7. Normalize language codes with isolang during remote import

Date: 2026-07-31

## Status

Accepted

## Context

`resolve_or_create_language()` in `tauri/src/commands/import_edition.rs`
looks up the `languages` table by exact match on `languages.code`. Remote
providers return language codes in inconsistent formats:

- **Google Books** — ISO 639-1 two-letter codes (`"en"`, `"fr"`).
- **OpenLibrary** — ISO 639-3 three-letter codes (`"eng"`, `"spa"`).
- **Hardcover** — never sends language data (the field lives on Editions, not
  Book Search).

The `languages` table is seeded from `CommonLanguages` (30 variants in
`livtet-types/src/known.rs`), all stored with ISO 639-1 codes (`"en"`,
`"es"`, `"fr"`, etc.). When OpenLibrary returns `"eng"`, the exact-match
lookup misses the seeded `"en"` row. A new row is created with
`code = "eng"`, `name = "eng"` (the raw code used as fallback name), and no
flag emoji. The same book imported from Google Books would correctly match
`CommonLanguages::English` and get the name "English" with flag "🇺🇸".

This creates duplicate language rows for the same language and degrades the
UI (bare code displayed instead of a friendly name, no flag).

## Decision

Add the [`isolang`](https://crates.io/crates/isolang) crate to
`livtet-types` and provide a normalization function on `CommonLanguages`
that maps arbitrary ISO 639 codes to canonical two-letter codes before
database lookup.

### Normalization function

`CommonLanguages::normalize_language_code(raw: &str) -> Option<LanguageInfo>`,
defined in `livtet-types/src/known.rs`. The returned `LanguageInfo` struct
carries the normalized 2-letter code, English name, native-language
autonym, and flag emoji:

```rust
pub struct LanguageInfo {
    pub code: String,              // "en"
    pub english_name: String,      // "English"
    pub autonym: Option<String>,   // "English" (same as english_name for en)
    pub flag_emoji: Option<String>, // "🇺🇸"
}
```

The normalization logic:

1. Replace underscores with hyphens (`en_GB` → `en-GB`).
2. Try `isolang::Language::from_locale()` (handles BCP 47 tags like
   `"en-GB"`, `"zh-Hans"`).
3. Try `isolang::Language::from_639_3()` (handles OpenLibrary's `"eng"`,
   `"spa"`).
4. Try `isolang::Language::from_639_1()` (handles Google Books' `"en"`,
   `"es"`).
5. If any step succeeds: look up the normalized 2-letter code in
   `CommonLanguages::all()` for name/autonym/flag, falling back to
   `isolang::Language::to_name()` for the English name and
   `flag_emoji_for()` for the flag.
6. If none succeed, return `None` — caller falls back to exact-match
   behavior.

### English name fallback

`isolang` is configured with the `english_names` feature. When a
language code is recognized by isolang but not present in
`CommonLanguages` (e.g. Swahili `"sw"`), `isolang::Language::to_name()`
provides the English display name ("Swahili") instead of falling back to
the raw code.

### Autonyms

`CommonLanguages::autonym()` returns the native-language name for each
of the 30 seeded languages (e.g. `Spanish` → `"Español"`, `Russian` →
`"Русский"`). The autonym is stored in the `languages` table for seeded
languages and the `LanguageInfo` struct carries it for use during
import. For languages not in `CommonLanguages`, autonym is `None`.

### Flag emoji

`flag_emoji_for(code: &str) -> Option<String>` derives a flag emoji
from a 2-letter language code. It first checks the 30
`CommonLanguages` variants (which have hand-picked flags), then falls
back to a `language_to_country()` mapping (~100 additional languages)
that converts language code → country code → Unicode regional indicator
pair (e.g. `"fr"` → `"FR"` → `"🇫🇷"`).

### Call site

`resolve_or_create_language()` in `tauri/src/commands/import_edition.rs`
runs the raw provider code through the normalizer before the DB lookup:

```rust
let lang_info = CommonLanguages::normalize_language_code(code);
let lookup_code = lang_info.as_ref().map(|li| li.code.as_str()).unwrap_or(code.as_str());
```

The normalized code is used for the `languages.code` lookup. When
creating a new row, the English name and flag emoji from `LanguageInfo`
are stored directly — no separate `CommonLanguages::all()` iteration
needed.

### Dependency placement

`isolang` lives in `livtet-types` (not the desktop crate) because
`CommonLanguages` owns the language model and any future consumer (CLI,
sync, mobile) benefits from the same normalization without adding a
dependency to the Tauri layer.

## Consequences

### Becomes easier

- **OpenLibrary imports match seeded languages.** `"eng"` → `"en"` matches
  `CommonLanguages::English` and gets name "English", flag "🇺🇸",
  autonym "English", and the deterministic ULID. No duplicate rows.
- **Unknown languages get readable names.** Swahili `"sw"` from a
  provider is not in `CommonLanguages`, but `isolang::Language::to_name()`
  provides "Swahili" — no bare-code fallback needed.
- **All recognized languages get flags.** `flag_emoji_for()` covers the
  30 seeded languages plus ~100 additional languages via language→country
  mapping. Only truly unknown codes get `flag_emoji = None`.
- **Autonyms are available at import time.** The `LanguageInfo` struct
  carries the native name for all 30 CommonLanguages variants. A future
  UI can display "Español" alongside "Spanish".
- **Mixed-format imports are correct regardless of provider.** If a future
  provider sends BCP 47 tags (`"en-US"`), the normalizer handles them
  without changes to the import path.
- **Unknown codes still work.** If `isolang` doesn't recognize a code
  (e.g. a non-ISO string), the normalizer returns `None` and the function
  falls back to exact-match — same behavior as before.

### Becomes harder

- **New dependency in the core type crate.** `isolang` pulls in `phf 0.11`
  (compile-time hash maps). The footprint is small (~20 kB in release) but
  it's a new crate in the dependency tree. The `english_names` feature
  adds ~180 KB of static string data.
- **`isolang` may not know every valid ISO 639 code.** If a provider sends
  a legitimate but obscure code, it will fall through to raw-code behavior.
  This is acceptable because the provider's data is authoritative and the
  fallback is no worse than before.
- **Flag mapping is opinionated.** The `language_to_country()` function
  picks one representative country per language (e.g. `"en"` → `"US"` not
  `"GB"`). This is correct for display purposes but could surprise users
  who expect region-specific flags.
- **Maintainers must understand the normalization chain.** The path from
  provider → `resolve_or_create_language()` → `normalize_language_code()` →
  `LanguageInfo` → DB lookup is longer than a simple string comparison.
  The normalizer's order matters: `from_locale` before `from_639_3` before
  `from_639_1` ensures BCP 47 tags are parsed as locales, not
  misinterpreted as three-letter codes.
