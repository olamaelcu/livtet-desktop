# 50. Choose a cover source in the enrichment review

Date: 2026-10-31

## Status

Accepted. Extends [46](0046-enrich-edition-metadata-from-plugin-enrichers.md):
enrichment keeps its preview/apply contract, but the cover it offers is now a
choice rather than a foregone conclusion.

## Context

Several enrichers can offer a cover for the same edition. `combine` folded their
answers in plugin order and kept the first non-`None` cover, and plugin order is
stanchion's load order — alphabetical by plugin name. So with `amazon`,
`duckduckgo`, `goodreads` and `googlebooks` installed, Amazon's image always won
when it existed, and the review screen stated only `image/jpeg — will be saved`.
The reader was shown which sources contributed, but not which of them supplied
the cover, and had no way to prefer another.

That is the wrong default for the one field where sources visibly disagree: a
cover is the most subjective thing an enrichment writes, and its quality varies
by source far more than a title or a publisher does.

Alternatives considered:

1. **Global source preference in Settings.** One ordering for the library,
   applied wherever covers are picked. Rejected for now: it is a settings
   surface plus a persisted store for a decision readers make per book, and it
   would have to govern the not-yet-implemented bulk path too.
2. **Remember the choice per edition.** Persist which source a reader picked, so
   re-enriching keeps it. Rejected for now: a new per-edition field and its
   migration for a choice that is cheap to re-make, and it would still need the
   per-apply picker behind it.
3. **Text-only offers.** List each source's cover as `name — mime` with no
   image. Rejected: choosing a cover without seeing it is choosing a name.
4. **Serve previews from the loopback byte server.** Smallest IPC payload, but a
   new route, a bounded in-memory cover map and its cleanup for a handful of
   images per click.

## Decision

The preview lists **every** cover a source offered, and apply takes the chosen
source.

- `combine` keeps each answer's cover beside the merged enrichment, in plugin
  order, so the order remains the tie-break but is no longer the only outcome.
- `EnrichmentPreview.cover: Option<CoverOffer>` becomes
  `covers: Vec<CoverOffer>`, where `CoverOffer` is
  `{ source, mime, storable, thumbnail }`. `thumbnail` is a `data:` URL of a
  JPEG rendered server-side at 240px; `null` when the bytes cannot be decoded,
  which still leaves the offer selectable.
- `apply_enrichment` gains `cover_source: Option<String>`: `None` takes the
  first offer (the old behaviour, now opt-in by omission), a name that is not in
  the preview is refused rather than silently ignored.
- The choice is per apply. Nothing is persisted: the review is re-run to change
  it.
- A cover is still offered only to an edition that has none, and still stored
  only when a `digital_inventory` row exists ([46](0046-enrich-edition-metadata-from-plugin-enrichers.md)).

## Consequences

Easier: a reader can compare what each installed source actually returns and
pick the better scan; the review screen finally names the cover's origin.

Harder: `preview_enrichment` decodes and re-encodes one thumbnail per offered
cover (CPU work on a blocking worker, bounded by the number of matching
enrichers), and the preview payload grows by roughly 15-40 KB per offer. The IPC
shape changes, so `web/lib/bindings.ts` regenerates and any other consumer of
`EnrichmentPreview` must move from `cover` to `covers`.

Deferred: a remembered or global preference (alternatives 1 and 2), which would
build on this contract rather than replace it.
