# 24. OS keyring for OPDS credentials

Date: 2026-09-25

## Status

Accepted

## Context

[ADR 0026](0026-in-app-opds-catalog-browsing-and-acquisition.md) stores catalog
subscriptions in a Rust-owned `tauri-plugin-store` file
(`opds-catalogs.json`) and, at the time, persisted Basic/Bearer credentials
inline in that file as plaintext, leaving "an OS-keyring-backed store" as a
follow-up.

That file lives in the app data directory (`~/.local/share/net.olamaelcu.livtet/`
on Linux), so a plaintext password or patron token there is readable by any
process or backup that touches the directory. Every desktop platform we target
already provides a login credential store (Secret Service, Keychain, Credential
Manager), and the `keyring` crate selects the right one.

## Decision

1. Credentials move to the OS keyring via the `keyring` crate (v4, default
   features): Secret Service on Linux, Keychain on macOS, Credential Manager on
   Windows. Service `net.olamaelcu.livtet.opds`, account = catalog ULID, value =
   the JSON secret (`{kind, username, password}` / `{kind, token}`).
2. `opds-catalogs.json` keeps only metadata plus the auth kind. `StoredCatalog`
   drops its inline `auth` field; `auth_kind` (defaulting to `none`) is the only
   credential signal on disk.
3. `SecretStore` abstracts the keyring behind a trait. Command code runs its
   blocking calls on `spawn_blocking`; tests use an in-memory implementation.
4. There is no plaintext fallback. When the keyring is unavailable, creating or
   updating a catalog with credentials fails with
   `OpdsError::CredentialsUnavailable`, and browsing a catalog whose secret is
   missing fails the same way (fail closed). Catalogs without credentials are
   unaffected.
5. No migration: the previous inline-secret shape is not read. Any authenticated
   catalog created before this change must be re-credentialed once, after which
   the secret is gone from the file.

## Consequences

**Easier**: credentials are protected by the OS login keyring and visible in its
credential UI; the catalog file is safe to copy, sync, or back up; exactly one
code path can ever hand out a secret.

**Harder**: authenticated catalogs now require a working Secret Service (a
running `gnome-keyring`/KWallet and session D-Bus on Linux) or creation fails; a
lost keyring entry means re-entering the credential; tests cannot exercise the
real backend, so the keyring boundary is covered by the in-memory store and the
fail-closed error path is asserted directly.
