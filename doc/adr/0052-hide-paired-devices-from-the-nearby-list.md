# 52. Hide paired devices from the nearby list

Date: 2026-11-01

## Status

Accepted

## Context

`discovery.nearby` is a raw mDNS browse: the daemon returns every peer
advertising `_livtet-sync._tcp`, minus its own record, and says nothing about
whether this desktop already holds a `paired_devices` row for that peer. A
device keeps advertising after it is approved, so the pairing panel listed an
approved device as a peer to pair — again — with a **Pair** button that
`pairing.claim` would attempt a second time.

The two facts live behind different methods: discovery has the peer's
`node_id` and `device_id`, `devices.list` has `device_id` only.

## Decision

`sync_nearby_devices` joins the two: it calls `discovery.nearby`, then
`devices.list`, and drops each peer whose `device_id` matches a paired
device's.

- **Device id is the key.** Both pairing directions record the peer's own
  device id — the `device_id` TXT field it advertises and the one its
  `/sync/identity` answers with — so an approved device matches its
  advertisement. `node_id` would be the stronger identity but `devices.list`
  does not carry it.
- **The filter sits in the command, not the panel.** Discovery is a wire
  method shared with other clients; `sync_nearby_devices` is this app's
  answer to "what can I pair with", so the invariant lives at the boundary
  that names it, the same way the daemon drops its own record from the raw
  browse.
- **An advertisement with no device id is kept.** It cannot be matched to a
  device row, and hiding it would leave an unidentifiable peer listed
  nowhere.
- **A failing `devices.list` fails the command.** Without the device rows the
  filter cannot run, and a list that silently unfilters would offer approved
  devices again; the panel already renders that failure as "Nearby discovery
  is unavailable".

## Consequences

### Easier

- The nearby list means one thing: peers that can still be paired.
- The panel needs no second query to decide what to hide, and a 5-second
  discovery poll cannot show a stale pairing state.

### Harder / to watch

- Every nearby poll costs a second RPC (`devices.list`, one local SQLite
  query) on top of the two-second browse.
- Correlation is by device id. If a peer ever advertises a different id from
  the one it paired with — a factory reset, a reinstall — it lists as
  unpaired while its old device row lingers until revoked.
- The daemon still browses paired peers, so the work is spent and discarded.
  Moving the filter into `discovery.nearby` would fix that, but that method is
  the daemon's raw view and core owns it.
