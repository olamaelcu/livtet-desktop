# 48. Offer book files to paired devices

Date: 2026-10-31

## Status

Accepted. Amended by [ADR 0053](0053-ask-before-a-sync-offer-lands.md): a pull
records the offer and waits for the receiver's decision instead of downloading
it on sight.

## Context

A paired device holds a session token this desktop issued it, so it can pull
from the sync server, but this desktop holds no credential the peer trusts.
Transfer therefore has to be **offer + pull**, not a push: the desktop records
that a book is available, and the paired device fetches it on its next sync.
Metadata already travels through `change_log`; files are served by
`GET /sync/files/:inventory_id`, and covers had no route at all.

## Decision

- **Offer.** `devices.offer_book { edition_id, device_id }` verifies the
  device is paired and the edition has a file on disk, then appends a
  `change_log` row with `entity_type = 'file_offer'`, `operation = 'OFFER'` and
  a payload carrying `inventory_id`, `edition_id`, `work_id`, `to_device_id`,
  file name/size/hash, and whether a cover exists. Rows share the normal
  version cursor, so a peer sees offers while pulling `/sync/changes`.
- **Metadata.** The same command touches `updated_at` on the digital
  inventory, edition, and work rows. The existing audit triggers emit full
  UPDATE payloads for them, so the normal pull carries the catalog record.
  Identifiers and contributors outside those rows still need a full pull.
- **Bytes.** `GET /sync/files/:inventory_id` serves the book (with ranges);
  the new `GET /sync/covers/:inventory_id` serves the extracted cover. Both
  require the session token the caller was issued. `GET /sync/changes` filters
  `file_offer` rows by `to_device_id` against the device the token belongs to,
  so a peer never sees another device's offer; other changes stay library-wide.
- **Claim.** `pairing.claim` posts a claim to a discovered peer, follows its
  decision stream, and stores the token the peer issued as an **outbound**
  paired device (`paired_devices.outbound = 1`). Existing approval rows stay
  inbound; only outbound rows can authenticate against the peer.
- **Pull.** `devices.pull` (and a once-a-minute loop) reads `/sync/changes`
  with that outbound token, downloads every offer addressed to this device's
  id, then applies the metadata through `SyncEngine::apply_changes`. The
  downloaded file and cover are rewritten to the receiver's `books/` and
  `covers/` paths before the inventory row is written; the per-device cursor
  lives in `client_settings`. Offers already waiting are pulled once when the
  claim is approved.
- **Receiver.** A paired device filters offers by `to_device_id`, fetches the
  file and cover, then imports them with its own paths and applies the
  accompanying change rows. The desktop's drawer exposes the offer as a
  **Send to…** dropdown; the nearby list exposes **Pair** for the reverse
  direction.

## Consequences

### Easier

- The existing claim/approval flow supplies the outbound token: the receiver
  is whichever side held the token the other issued it.
- Offers reuse change-log ordering and conflict handling; no new transport.
- The file and cover routes are the same surface a future mobile client uses.

### Harder / to watch

- Applying the sender's absolute `file_path`/`cover_path` would break the
  receiver's library, so the import step rewrites both paths before writing
  the inventory row.
- Only **outbound** paired rows can be pulled from. Approval rows hold tokens
  this desktop issued, which the peer's server does not accept.
- The apply path writes trigger-shaped payloads with raw SQL: ids are hex,
  temporal values stay SQLite text, and a column the trigger does not emit is
  left untouched on conflict. Extending the triggers means revisiting this.
- Pulling is polled once a minute (and once on claim approval); a large library
  may need progress reporting in the UI.
