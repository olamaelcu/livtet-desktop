# 53. Ask before a sync offer lands

Date: 2026-11-01

## Status

Accepted

## Context

ADR 48 made sending a book **offer + pull**: the sender records a `file_offer`
change row, and the receiver's daemon downloads the file and applies the
catalog rows on its next pull tick. Nothing asked the reader. A book sent to
the wrong device landed in that library, cost the transfer, and had to be
deleted by hand.

The reader wants a prompt that shows the book — cover, title, authors — with
accept and decline, and the sender should learn the answer. None of that
existed: the offer payload carried ids and a file name but no title or
authors, a pull had no way to stop short of the library, and the receiver had
no channel back.

The protocol half lives in the sync daemon (`livtet-sync-server` and friends,
the `olamaelcu/livtet` repository this app consumes at a pinned revision), so
this record covers **both sides** and what each one changed.

## Decision

Core (see that repository's ADR 5, `livtet@15c2d09`):

- `devices.offer_book` adds `title` and `authors` to the offer payload.
- `pull_device` records each offer addressed to this device in a new
  `inbound_offers` table (client migration `client-0009`) and holds back the
  `work`, `edition`, and `digital_inventory` rows its payload names.
- `offers.list` returns pending offers, `offers.decide { offer_id, accepted }`
  is the only path that downloads (accept) or discards (decline), and
  `offers.cover` proxies the sender's cover as a data URL because nothing is
  on disk yet.
- After deciding, the daemon `POST`s `/sync/offers/:offer_id/decision` to the
  sender over the token it issued and reports `delivered` rather than failing
  when the peer is unreachable.
- New notifications: `offer.received` on the receiver, `offer.decided` on the
  sender (through the route's sink).

Desktop:

- `sync_offers_list`, `sync_offers_decide`, and `sync_offer_cover` proxy the
  three methods; the DTOs (`SyncIncomingOffer`, `SyncOfferDecision`,
  `SyncOfferCover`) carry exactly what the prompt renders and nothing else —
  sizes and hashes stay in the daemon's row.
- `offer.received` → `sync://offer-received` and `offer.decided` →
  `sync://offer-decided`, so the prompt does not wait for its 30-second poll
  and the sender toasts the answer.
- `IncomingOfferDialog` is mounted in the root layout, not in Sync settings: an
  offer is a decision the reader must see wherever they are. It shows the
  cover when the sender has one, the title, the authors, and who sent it, and
  it re-asks if closed without an answer.
- Accepting invalidates the whole query cache, because a book and its catalog
  rows arrive at once; declining invalidates only the offers key.

## Consequences

### Easier

- A refused book leaves nothing behind on the receiver: no file, no rows, no
  library entry.
- The prompt needs no library row, so it works on a fresh install and for
  books this device has never seen.
- The sender knows the outcome instead of inferring it from silence.

### Harder / to watch

- The desktop depends on a daemon revision that implements the offer methods.
  An older sidecar fails all three commands with an RPC `method not found`
  error, which surfaces as a toast rather than a crash.
- `pull_device` no longer treats a page uniformly: rows are partitioned by the
  offer payload's ids, so a payload whose ids do not match its rows holds
  nothing.
- A decision is reported best-effort. The receiver keeps its answer when the
  sender is gone (`delivered: false` → a warning toast); nothing retries, and
  the sender has no record of the answer it missed.
- Closing the prompt re-asks rather than deferring: an unanswered offer has no
  other place to live in the UI.
