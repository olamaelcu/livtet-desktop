# 30. Plugin management IPC (list, add, remove, remote)

Date: 2026-09-28

## Status

Accepted. Builds on [ADR-0029](0029-livtet-plugin-host-sidecar.md).

## Context

`list_plugins` existed but there was no way to add or remove plugins from the
app. The `/plugins` route needs to list loaded plugins, install new ones from a
local directory, and remove them; remote-registry install is desired but no
registry server (`stanchion-index-poem`) is deployed yet.

## Decision

Add four commands to `commands/plugins.rs`, registered in `collect_commands!`
and surfaced in `web/lib/bindings.ts`:

- `add_plugin_from_path(source)` — sideload. Validates the source with
  `stanchion::registry::read_manifest`, copies it into `plugins_dir/<manifest
  name>`, then launches the host to confirm it loads and returns its
  `PluginSummary`; a plugin that copies but fails to load is rolled back.
- `remove_plugin(name)` — deletes the plugin directory.
- `discover_remote_plugins(registry_url)` / `install_remote_plugin(registry_url,
  id)` — typed commands for the "from repository" flow that return
  `PluginError::Unavailable` until a registry endpoint exists.

`PluginError` gains an `Invalid` variant for validation failures (bad manifest,
name collision, unsafe name) and `unavailable`/`invalid` constructors.

**Uninstall is revoke.** The host is launched per call (ADR-0029), so it holds no
state to revoke a capability against between calls, and editing a signed
plugin's manifest would break its signature. Durable revocation is therefore
uninstalling the plugin (removing its directory); the UI's "revoke" acts on
`remove_plugin`. Per-capability, persistent grant management is deferred to a
future trust-store (would be its own ADR).

**Path safety.** Plugin names — from a manifest or the frontend — are checked to
be a single safe path segment (no `/`, `\`, `.`, `..`) before joining onto
`plugins_dir`, so neither install nor remove can escape it.

## Consequences

- Easier: the app can install and remove plugins; the route has a real backend.
- Harder / deferred: remote install is a stub pending the index server and the
  `distribution-client` feature; there is no per-capability revoke or a trusted-
  signer editor yet.
- The `Invalid` variant is additive to the `PluginError` IPC surface; existing
  callers are unaffected.
- The install path reads only the manifest `name` via a local `toml` parse
  rather than `stanchion::registry::read_manifest`. This keeps install decoupled
  from the registry crate's export surface (it was not exported on the earlier
  pin), and the host still fully validates the manifest when it loads the
  plugin. Retained after the stanchion bump to `49d1781`.
