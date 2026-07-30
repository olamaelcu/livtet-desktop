# 4. Secrets via SOPS + age

Date: 2026-07-30

## Status

Accepted

## Context

`livtet-desktop` needs two externally-provisioned values at build time: a
Google Books API key and a Sentry DSN. Both are client identifiers whose
compromise is bounded (the providers restrict by referrer / project); both
are bundled into the binary at compile time.

Mobile has run a SOPS-based workflow since before desktop started:
`.sops.yaml` + age + a tracked encrypted JSON bundle, decrypted to a
gitignored JSON, exported to a gitignored dotenv file via Mise file tasks
under `.mise/tasks/`. The mobile exporter hardcodes `LIVTET_*` prefixes and
`_IOS` / `_ANDROID` / `_MOBILE` suffixes. Adopting that as-is would have
forced every consumer to know about a project namespace and a platform
axis that have no relevance on desktop.

The first attempt called for `config::File::new(".mise/secrets.env",
FileFormat::Env)`. That variant does not exist — `config = 0.15`'s
`FileFormat` enum has only `Toml | Json | Yaml | Ini | Ron | Json5 |
Corn`, and there is no `env` feature flag. It also called for
`Environment::default().separator("_")` to lift `GOOGLE_BOOKS_API_KEY`
to `google_books_api_key`. With the `convert-case` default feature on,
this transformation produces nested-map paths for unrelated env vars
(`CARGO_MANIFEST_DIR` → `cargo.manifest.dir`) and the flat typed facade
cannot deserialize against the resulting tree.

## Decision

Adopt the mobile SOPS pattern with three changes that the desktop context
makes necessary:

1. **Separate `.sops.yaml` and `.mise/secrets.json.sops`** with a
   desktop-only age recipient. The encrypted bundle is tracked;
   `age.key`, `.envrc`, `.mise/secrets.json`, `.mise/secrets.env`, and
   `.mise/.sops-age-key-fingerprint` are gitignored. CI feeds
   `SOPS_AGE_KEY` from its secret store; operators keep the key in
   `.envrc` (gitignored at user-global level).
2. **`secrets-export-env` emits lowercase keys with no prefix or
   suffix.** `google_books_api_key` and `sentry_dsn` are what consumers
   see. There is no mobile-style `LIVTET_*` namespace, no platform axis.
3. **`tauri/build.rs` uses `Environment::source(Some(map))` for both
   layers, not `Environment::default()`.** This avoids `config`'s
   `convert-case` transformation of unrelated env vars and lets the build
   script read process env directly via `std::env::vars()`. Shell-set
   `GOOGLE_BOOKS_API_KEY=cli-test cargo build` overrides the file value
   because the env HashMap is added second to the `Config::builder()`
   chain.

The schema in `.mise/secrets.json`:

```json
{
  "api_keys": {
    "google": {
      "books": "<GOOGLE_BOOKS_API_KEY value>"
    }
  },
  "telemetry": {
    "sentry": {
      "desktop": {
        "dsn": "<SENTRY_DSN value>"
      }
    }
  }
}
```

The four Mise tasks under `.mise/tasks/` port mobile's scripts verbatim
except for paths and (for `secrets-export-env`) the destination keys.
Bootstrap: `mise run secrets-init` generates an age keypair at `./age.key`,
rewrites `.sops.yaml` with the real recipient, creates an empty
`.mise/secrets.json` skeleton, and exits 0. Subsequent rotations go
through `mise run secrets-edit` (in-place edit of the encrypted file)
and `mise run secrets-decrypt` + `mise run secrets-export-env` to refresh
the working copies.

`tauri/build.rs` (`[build-dependencies]` carries `config = "0.15"` and
`serde`) anchors the env-file path to `CARGO_MANIFEST_DIR/../.mise/secrets.env`
(cargo runs build scripts with CWD = the package dir, not the workspace
root), parses the file by hand, feeds both sources via
`Environment::source(Some(map))`, deserializes a `#[derive(Deserialize)]
struct Secrets { google_books_api_key: String, sentry_dsn: String }`,
rejects empty values with `std::process::exit(1)`, and emits
`cargo:rustc-env=GOOGLE_BOOKS_API_KEY=…` and
`cargo:rustc-env=SENTRY_DSN=…`. `tauri/src/secrets.rs` exposes two
`pub const &str` via `env!()` and `pub mod secrets;` wires them into
the crate root. Once any consumer references `crate::secrets::*`, the
linker preserves the literal and the rotation takes effect on the next
build.

The operator workflow lives in `doc/secret-management.md` — sparse, the
ADR is the design record.

## Consequences

### Becomes easier

- Operators with the desktop age key can `direnv allow && mise run
  secrets-decrypt && mise run secrets-export-env && cargo build` on a
  fresh clone without touching tracked files.
- New secrets take three edits: an `export_value` line in
  `secrets-export-env`, a field on `Secrets` in `tauri/build.rs`, and
  a `pub const` in `tauri/src/secrets.rs`.
- Hard errors surface in build.rs with operator-friendly messages.
  Three failure paths (missing file, empty value, env-set-to-empty)
  are tested with `cargo check -p livtet-desktop`.
- Shell-set env vars override the file without a separate "dev mode"
  branch.

### Becomes harder or carries risk

- `cargo:rustc-env` embeds the values into the release binary.
  Appropriate for client identifiers (restricted by referrer / project);
  **not** appropriate for server credentials. A follow-up plan is
  required before any backend secret is added to this list.
- `secrets-init` has two latent bugs documented during execution: the
  encrypt-only-once branch (`sops --encrypt --in-place` then `mv`)
  doesn't work because `.sops.yaml`'s `path_regex` matches
  `.mise/secrets.json.sops` only, and the script doesn't update
  `.envrc` with the freshly-generated key. Bootstrap uses manual
  workarounds; the script as committed is not yet
  operator-self-sufficient.
- The plaintext key is in `.envrc` (gitignored at the user level). The
  external-file pattern (`SOPS_AGE_KEY_FILE=…`) recommended by
  `mobile/CONTRIBUTING.md`/`GETTING_STARTED.md` is supported by mise
  and sops but is not the default for the desktop yet.
