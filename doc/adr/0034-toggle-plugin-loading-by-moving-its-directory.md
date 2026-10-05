# 34. Toggle plugin loading by moving its directory

Date: 2026-10-04

## Status

Accepted. Builds on [ADR-0029](0029-livtet-plugin-host-sidecar.md) and
[ADR-0030](0030-plugin-management-ipc.md).

## Context

The plugins page can install and uninstall plugins, but there is no way to stop
one loading without deleting it. Uninstalling loses the plugin's files and any
grant decisions made about it, so "I want this off for now" costs a reinstall.

The plugin surface offers `list_plugins`, `add_plugin_from_path`,
`remove_plugin`, `discover_remote_plugins` and `install_remote_plugin`. None of
them carries an enabled state, and `PluginSummary` is
`{ name, version, granted, signer }`.

Two existing properties constrain the design. The host is launched, queried and
dropped per call, so it "carries no state between calls to revoke against"
(`commands/plugins.rs`) — there is no running registry to toggle. And the set of
loaded plugins is exactly the set of directories under `plugins_dir`, which is
why `remove_plugin` is just `remove_dir_all`. The filesystem already *is* the
registry.

## Decision

Disable a plugin by moving its directory out of the scanned root, and enable it
by moving it back.

- A sibling root, `plugins-disabled/`, is created beside `plugins_dir` and
  recorded on `AppState` as `disabled_plugins_dir`.
- **`set_plugin_enabled(name: String, enabled: bool) -> Result<(), PluginError>`**
  moves `<name>` between the two roots. It reuses `ensure_safe_name`, so a name
  from the frontend still cannot escape its root, and it is idempotent: enabling
  an enabled plugin succeeds without doing anything.
- `PluginSummary` gains **`enabled: bool`**.
- `list_plugins` launches the host twice — once per root — and marks the second
  set `enabled: false`. Disabled plugins therefore keep reporting their real
  name, version, signer and grants, because the host still parses them; it
  simply never loads them for work.

Moving rather than marking is what makes this fit: nothing in the host or in
`stanchion` needs to learn about an enabled flag, the state survives restart
without a new store, and it is as durable as uninstall is today. A disabled
plugin is unreachable by exactly the same mechanism as an uninstalled one — it
is not in the directory the host scans — while its files and identity survive.

## Consequences

- Easier: turning a plugin off is reversible and cheap; a user can disable a
  suspect plugin without losing it, and grants are not re-prompted on re-enable.
  No new persistence format, no schema migration, no change to `stanchion`.
- Harder: `list_plugins` now costs two host launches instead of one. The host is
  already launched per call, so this doubles a cost that was never amortised,
  but it is measurable on a large plugin set.
- `PluginSummary` gaining a field is an additive change to the generated
  TypeScript (`web/lib/bindings.ts`); every consumer must be regenerated via
  `mise generate-bindings`.
- A plugin whose directory is moved while the host is mid-call will fail that
  call. The host is launched per call and calls are short, so the window is
  small, but `set_plugin_enabled` is not synchronised against an in-flight
  `list_plugins`.
- Two roots now hold plugin directories. Anything that reasons about "installed
  plugins" must consider both — notably `remove_plugin`, which must delete from
  whichever root holds the plugin.
