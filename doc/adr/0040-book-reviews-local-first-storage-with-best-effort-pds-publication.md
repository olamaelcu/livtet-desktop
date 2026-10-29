# 40. Book reviews: local-first storage with best-effort PDS publication

Date: 2026-10-28

## Status

Accepted

## Context

The edition-detail drawer shows a book's catalog record, shelf placement, and
reader notes, but a reader has no way to write down what they thought of the
book. Reviews exist on the platform side (the Bibliograph service writes
`net.olamaelcu.livtet.biblio` records through `review-writer.ts` and renders a
`ReviewInsert` form), so the desktop needs the same capability against its own
local-first model.

Two constraints shape the answer. First, persistence: the catalog tables
(`reading_progress`, `reading_bookmarks`) are owned by the core migration set,
which the desktop consumes as a **git dependency**
(`livtet-core = { git = "…", branch = "main" }`), so a desktop feature cannot
add a column or table there without a cross-repository change and a lockfile
bump. Second, the reader's words must never be lost to a network or sign-in
problem — publication to the PDS is an account feature, not a precondition for
keeping a review.

Alternatives considered:

1. **Reuse `reading_progress.annotations`.** No schema work at all, but that
   column is a JSON array of *notes* read by `parseAnnotations` in
   `web/lib/reader/progress.ts`; storing an object-shaped review there would
   either surface in the notes list or break that parser. Two writers owning
   one column is the same mistake that `save_reader_position` already guards
   against by treating `None` annotations as "not mine".
2. **Add a table to the core migration set.** Correct in the long run, but it
   requires editing another repository, publishing a commit, and bumping the
   git dependency — a release-blocking coupling for a desktop-only feature.
3. **A desktop-owned table created on first use.** `CREATE TABLE IF NOT EXISTS
   reviews (…)` in the desktop's own database file, executed by the command
   that reads or writes it.

## Decision

Reviews are local-first, in a `reviews` table the desktop owns and creates
idempotently on first use (`commands/reviews.rs::ensure_reviews_table`), keyed
by `edition_id` and indexed for the per-edition listing. Three commands sit on
it: `save_local_review`, `load_local_reviews`, and `put_local_review_to_pds`.

The rating is `f64` (halves, 0–10) because the `wa-rating` control the form
uses offers `precision={0.5}`; the value is clamped and snapped on write so the
stored row and the published record always agree, and an unrated review stores
`NULL` rather than `0`.

Publication is best effort, opt-out, and happens **after** the local write,
from the UI. The form carries a *Publish to my PDS* switch, on by default
because publishing is the point of the feature. Its state follows
`atproto_status`: a signed-out account gets the switch forced off and a line
saying to sign in, since a switch that is on and can only fail is a lie. The
status is tri-state (`undefined` while the query loads) so the switch cannot
flash off and latch there before the answer arrives. Turning it off still
saves the review — the only difference is the missing record and, therefore, no
`record_uri` to delete remotely later.

- `save_local_review` is the promise the reader gets; nothing is rolled back if
  publishing fails.
- `put_local_review_to_pds` resolves the edition record through the existing
  `resolve_edition_record` index lookup, resolves the book record's CID with an
  anonymous `com.atproto.repo.getRecord`, then writes one
  `net.olamaelcu.livtet.bookReview` record through `AtprotoManager::write_review`.
- It returns `ReviewSync { record_uri, book_page_url, error }` — the same
  "report, do not fail" shape as `ShelfSync` — so the drawer can toast "saved
  locally" and the reason separately.
- `book_page_url` is the reviewed book's page in the Livtet web app
  (`https://livtet.olamaelcu.net/book/{repo}/{rkey}`), built from the edition
  record's own `at://` URI. It is resolved as part of publishing, because the
  edition is only known once the title lookup has run, and it is returned even
  when the write fails so the reader can still reach the book.

### The collection is fixed by the OAuth grant

Writing to `net.olamaelcu.livtet.biblio.review` — the obvious sibling of
`.shelf` and `.bookShelving` — fails at runtime with

```
403 {"error":"InsufficientScope",
     "message":"Token scope does not permit Create on net.olamaelcu.livtet.biblio.review"}
```

because the grant in the served client metadata names exactly five repo
collections (`…actor.profile`, `…biblio.readingGoal`, `…biblio.shelf`,
`…biblio.bookShelving`, `…bookReview`) plus `blob:image/*`. There is no `.review`
entry, and widening the grant means editing the platform's metadata document and
making every reader consent again. Reviews therefore use
`net.olamaelcu.livtet.bookReview`: the collection the platform's own
`review-writer.ts` writes, the one the index reads, and the one the existing
grant already covers — no re-authorization.

The record follows that writer's schema exactly (`BookReviewRecordSchema`):
`$type`, `text`, `book { uri, cid }`, optional `rating` in half steps from 1 to
10 (omitted, never `null`, when unrated), `createdAt`. No `title` and no
`updatedAt`: extra fields would make the record something other than what the
index parses. Publication is an upsert in the same sense as the platform's — the
account's reviews are listed first, a review for the same book reuses its rkey,
and only a first review gets a freshly minted TID rkey (the PDS assigns TIDs
itself, but `putRecord` requires the caller to name the key).

## Consequences

Easier: reviews work signed out, offline, and against a PDS that is down; the
desktop keeps its own schema evolution without a cross-repository release; the
record shape lives beside the shelf record builders it mirrors
(`atproto/review_records.rs` next to `shelf_records.rs`).

Reviews carry a `record_uri` column: the `at://` URI `putRecord` returned,
written back onto the row after a successful publish. Deleting a review removes
the local row only. The published copy stays in the account's repository,
because removing it is a separate authenticated write
(`com.atproto.repo.deleteRecord`) that nobody has asked for yet — the column
exists precisely so that delete can be added without re-resolving the record.
The UI states the gap rather than hiding it: the row is marked *Published* and
the delete toast says the PDS copy is unchanged.

Because the column arrived after the first release of this table, `reviews` is
migrated in place: `PRAGMA table_info` decides whether `ALTER TABLE reviews ADD
COLUMN record_uri TEXT` is needed, since SQLite has no `ADD COLUMN IF NOT
EXISTS` and `CREATE TABLE IF NOT EXISTS` leaves an older table untouched.

Harder: the table is outside the core migration regime, so it must be created
by every reader of it (`ensure_reviews_table` is called by both commands) and
it will not appear in core-generated schema dumps or sync. Sync is deliberately
out of scope: reviews travel through the PDS, not the local change log. If the
core schema ever grows a reviews table, the desktop one should be folded into it
by a follow-up ADR rather than migrated silently.
