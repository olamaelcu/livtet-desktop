# 56. Show paired devices with their pairing direction

Date: 2026-11-01

## Status

Accepted

Supersedes [52. Hide paired devices from the nearby list](0052-hide-paired-devices-from-the-nearby-list.md)

## Context

ADR 52 dropped every peer this desktop already held a `paired_devices` row for
from the nearby list, so an approved device could not be paired a second time.
That is right for an *outbound* row and wrong for an *inbound* one.

The two rows are not interchangeable:

- **Inbound** (`outbound = 0`, written by `pairing.approve`): the peer claimed
  this desktop and holds a token this desktop issued. It may pull from here.
- **Outbound** (`outbound = 1`, written by `pairing.claim`): this desktop
  claimed the peer and holds the token the peer issued. It is the only kind
  `devices.pull`, `offers.cover`, and `offers.decide` will act on.

Receiving a book is therefore an outbound property alone. A desktop whose only
row is inbound cannot receive from that device at all — and ADR 52 removed the
one control that could create the outbound row: the peer still advertises, but
it was hidden from the list, so **Pair** was unreachable. A user reported
exactly this: books could be sent, none could be received, with nothing in the
UI to explain or repair it.

The panel also had no way to say which direction a row pointed, because
`devices.list` did not report the flag; pull attempts failed with the daemon's
`device was not claimed by this installation` and the message had to be
matched to explain anything.

## Decision

`devices.list` reports `outbound` per row (olamaelcu/livtet `31ad0c2`), and the
panel uses it instead of inferring direction from failures.

- **Discovery is passed through untouched.** `sync_nearby_devices` returns
  what `discovery.nearby` returned. A paired peer is listed and marked
  **Paired**, with its action reading **Re-pair**; an unpaired one reads
  **Pair**. Claiming an already-paired peer is safe: `pairing.claim` upserts on
  device id and sets `outbound`, rotating the session token, which is what
  upgrades an inbound row into a receivable one.
- **The Devices list names the direction.** Each row carries **Receive from**
  (outbound: this desktop pulls its books) or **Send to** (inbound: it pulls
  this desktop's books), with a line under the heading explaining both.
- **Pulls skip inbound rows.** *Check for books* iterates outbound rows only;
  with none it says so and points at pairing from this side, rather than
  surfacing a refusal per device.
- **The filter does not come back.** A future request to hide paired peers has
  to keep the reverse claim reachable.

## Consequences

### Easier

- Receiving is reachable from the UI: a device that only ever approved this
  desktop can be claimed from the nearby list, which is the only action that
  makes it a pull source.
- The list states the same invariant the daemon enforces, so the two cannot
  drift; a refused pull is now a genuine reachability error.
- No per-poll `devices.list` join on the discovery path: one RPC instead of
  two.

### Harder / to watch

- `outbound` is part of the daemon's `devices.list` payload; adding it required
  an upstream change and a sidecar repin. Any client decoding the old shape
  fails loudly (`unexpected sync daemon payload`), which is the intended
  failure mode.
- Re-pairing a peer that is already outbound rotates its session token. That is
  harmless for the row but invalidates the peer's view of the old token, so it
  re-pulls.
- The nearby list shows a device twice across two sections (once as Nearby, once
  as Devices). The **Paired** badge and **Re-pair** label are what keep that
  from reading as a duplicate.
