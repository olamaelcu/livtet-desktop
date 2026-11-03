# 57. Serve sync peers over iroh only

Date: 2026-11-03

Status: Accepted

## Context

The sync daemon in `olamaelcu/livtet` retired its embedded HTTP server
(`f7bf91a`, "sync: retire the HTTP transport and serve only over iroh"). Peers
are now dialled by iroh node id; there is no listen address, no served-request
log, and no server to start or stop.

The desktop was built around the old surface: `server.start`, `server.stop`,
and `requests.recent`; `SyncRequestRecord`; `server_running`, `host`,
`requests_served`, and `last_request_at` in `health`/`status`; the
`sync://server-started|stopped|request|completed` events; a LAN bind flag; and
an address shown on the settings page. Keeping the dependency on the old
revision was not an option: it also predates the offer and mutual-pairing work.

## Decision

Track the current `livtet` main (`667b7e40`) and carry its surface through the
desktop:

- Drop `server.start`, `server.stop`, `requests.recent`, and
  `SyncRequestRecord`.
- `health` and `status` expose `node_id`; `server_running`, `host`,
  `requests_served`, and `last_request_at` are gone.
- Notifications reduce to `pairing.requested|claimed`, `sync.pulled`, and
  `offer.received|decided`. `sync://pulled` now drives the library refresh.
- The sidecar takes `--port` (UDP), `--books-dir`, `--covers-dir`,
  `--device-name`, and `--pull-interval-secs`; `--host` and `--lan` are gone,
  and the LAN preference is removed from `sync.json`.
- Port probing binds a UDP socket; the remembered port remains, since a
  firewall rule names it.
- The firewall hint targets UDP.
- `pairing.claim` carries `node_id` and `device_name` only.

## Consequences

- The settings page reports pairing state rather than server state; there is
  nothing to start and no address to copy.
- `sync_diagnostics` no longer probes HTTP endpoints or reads a served-request
  log. It summarizes claims awaiting approval and pairings that point one way.
- A device paired under the HTTP transport must pair again once both sides run
  the iroh-only daemon.
- `livtet-sync-http` is no longer a dependency; nothing in the workspace used
  it directly.
