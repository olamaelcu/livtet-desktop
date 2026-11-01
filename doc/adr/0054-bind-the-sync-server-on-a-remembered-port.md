# 54. Bind the sync server on a remembered port

Date: 2026-11-01

## Status

Accepted

## Context

A pairing stores the peer's address once, at approval (`paired_devices.listen_on`),
and every later pull dials it. The daemon, meanwhile, was spawned with
`--port 0`, so it bound an ephemeral port on every launch and advertised the
new one over mDNS. Nothing refreshed the pairing rows from those
advertisements: this desktop's own sidecar saw `0.0.0.0:38981` after a restart
while its row for the peer still said `:61057`, and the peer's row here still
said the port it had four hours earlier.

The result is silence, not an error the reader can see. The pull loop logged
`pull from paired device failed … error sending request for url
(http://192.168.1.226:61057/sync/changes?since_version=110&limit=100)` once a
minute, the offer the other machine had recorded never arrived,
`inbound_offers` stayed empty, and the accept prompt (ADR 53) never opened. A
re-pair was the only way back, and the same breakage happens on every restart —
in both directions, since each side holds the other's stale port.

## Decision

Desktop (this repository):

- `sync.json` gains `listen_port`, remembered from the last successful start.
  Settings written before the field existed load with `None`.
- Setup resolves the port before spawning: the remembered one when it can still
  be bound, otherwise the first free port in `7710..=7769`, otherwise `None` so
  the kernel picks. Binding is the probe — a `TcpListener` bound to the same
  interface the daemon will use, dropped immediately before the spawn.
- The spawned daemon reports what it bound (`status.server_running`,
  `status.port`), and that value is written back. A daemon that could not bind
  the requested port exits instead of serving, so the spawn is retried once
  with an ephemeral port and the kernel's choice is what gets remembered — the
  next launch asks for that.
- A failed write is a warning, not a failed start: the daemon is already
  serving; only the next launch loses the stability.

Core (`livtet-sync-server`, the pinned-revision repository):

- `pull_device` keeps its behaviour and becomes `pull_device_once`. The new
  `pull_device` wrapper retries it once when the failure is followed by a
  successful re-resolution of the peer's address from its mDNS advertisement
  (matched by the `device_id` TXT record). The corrected address is written to
  `paired_devices.listen_on` first, so the retry and every later pull use it.
- `preferred_address` picks the first routable address a peer advertises —
  loopback, unspecified, and link-local addresses are skipped — and falls back
  to the first one listed, so two instances on one machine still sync.

## Consequences

### Easier

- A pairing survives a restart on both sides: the port is the same one the peer
  already stored.
- A peer that moved (new port, new DHCP lease) is found again on the next pull
  instead of needing to be paired anew.
- The failure is no longer invisible: the retry logs the address change, and a
  pull that still fails reports the transport error it started with.

### Harder / to watch

- A fixed port can collide with another program. The scan range keeps the app
  working (one free port is enough), but each launch may land on a different
  port, and peers still need the mDNS re-resolution to find it.
- The port is per machine, not per interface: flipping the LAN toggle re-probes
  against the new bind host at the next launch.
- Re-resolution costs one 2-second mDNS browse, paid only when a pull fails and
  only once per failed device per pull tick.
- The core half cannot reach this app until the pinned revision moves past the
  commit that carries it; until then only the port half is in effect.
