# Secret management

Two build-time secrets: `GOOGLE_BOOKS_API_KEY` and `SENTRY_DSN`. They live
in a SOPS-encrypted JSON bundle in this repo, decrypted to a gitignored
working copy, exported to a gitignored dotenv file, and embedded into the
binary by `tauri/build.rs` as `pub const &str` in `tauri/src/secrets.rs`.

## Operator workflow

First-time setup (one per clone):

```sh
direnv allow .envrc                              # exports SOPS_AGE_KEY from your local age key
mise run secrets-init                            # generates ./age.key, rewrites .sops.yaml
```

Daily / per-build:

```sh
mise run secrets-decrypt                         # .mise/secrets.json.sops → .mise/secrets.json
mise run secrets-export-env                      # .mise/secrets.json → .mise/secrets.env
cargo build
```

Rotate a value:

```sh
mise run secrets-edit                             # opens the encrypted file in $EDITOR, auto-re-encrypts on save
mise run secrets-decrypt && mise run secrets-export-env
cargo build
```

CI: supply `SOPS_AGE_KEY` from the secret store; the same three Mise
tasks run before `cargo build --release`.

## Add a new secret

Three places to edit, in order:

1. `tauri/build.rs` — add the field to `struct Secrets` and the
   `cargo:rustc-env` emit.
2. `tauri/src/secrets.rs` — add `pub const NAME: &str = env!("NAME");`.
3. `.mise/tasks/secrets-export-env` — add an
   `export_value <snake_name> '<jq.path>'` line.

Then `mise run secrets-edit` to add the value to the encrypted bundle,
decrypt + export, and rebuild.

## Read a secret in code

```rust
use crate::secrets;

fn send_to_sentry(event: &Event) {
    if secrets::SENTRY_DSN.is_empty() { return; }
    // ...
}
```

The const is baked into the binary at compile time. Override for one
build with an env var:

```sh
GOOGLE_BOOKS_API_KEY=dev-test cargo build
```

Env vars win over the file. Build fails fast if `.mise/secrets.env` is
missing or any field is empty.

## What's tracked, what's not

Tracked:

- `.sops.yaml` — age recipient
- `.mise/secrets.json.sops` — encrypted bundle
- `.mise/tasks/*` — the four bootstrap scripts

Gitignored:

- `age.key` — local private key
- `.envrc` — operator-local `SOPS_AGE_KEY`
- `.mise/secrets.json` — decrypted JSON
- `.mise/secrets.env` — exported dotenv
- `.mise/.sops-age-key-fingerprint` — sentinel for cache invalidation

## Architecture

See [ADR-0004](adr/0004-secrets-via-sops-age.md) for the design
rationale.
