# 42. Import ATProto shelves as virtual books

Date: 2026-10-29

## Status

Accepted

## Context

A reader who shelves books on the Livtet platform already owns that shelving in
their AT Protocol repository: one
`net.olamaelcu.livtet.biblio.bookShelving` record per shelved edition, each
carrying the edition reference, its title, and `metadata.status` naming one of
the app's four reading statuses (`reading`, `read`, `to-read`, `dnf`). The
desktop can mint a catalog-only "virtual book" for any title it knows, so the
obvious feature is "add a book from my shelves". Two questions decide the shape.

**Where does the list come from?** The appview exposes `listShelves` and
`listBooksOnShelf`, which return the enriched `BookShelfView` the platform UI
renders. Against a real account holding 2 855 shelving records in its repository,
both endpoints answered `{"bookShelves": []}` — the appview indexes a lagging
copy. The repository itself answered every page. Reading the account's own PDS
also gets the whole shelving in one collection walk instead of one request per
shelf, and needs no index to have caught up.

**What does importing one book mean?** The record carries a title and an edition
URI, nothing else: no cover, no contributors, no description. The book index
mints those edition records, so it can supply the metadata — but only per book,
which rules out enriching the whole list (2 855 lookups) and points at enriching
one book at the moment it is imported. The shelf the book sat on is the part the
user asked to keep, so it has to reach the local shelf tables, not just the
catalog.

## Decision

`list_shelved_books` reads the signed-in account's `bookShelving` collection
from its own PDS (`com.atproto.repo.listRecords`, 100 per page, capped at
10 000 records and stopped on a repeated cursor), parses each value into
`{ book_uri, title, shelf_key, shelf_name, shelved_at }` and returns the rows
sorted by shelf order then title. The reading status comes from
`metadata.status`, falling back to the shelf URI's rkey; a status this app does
not model leaves `shelf_key` null so the book stays importable.

`import_shelved_book` is a single command with three parts:

1. **Metadata, at import time.** One `searchEditions` call by title; the hit
   whose `uri` equals the record's edition URI wins, and a hit may be accepted on
   an equal title only when no exact record matched, so a different book with the
   same title cannot donate its author or cover. A failed lookup still imports
   the book with its title alone.
2. **A virtual edition.** `create_virtual_book_and_index` with the edition URI
   as an identifier — no file, and the URI doubles as the dedup key, so
   re-importing the same shelved book returns the first edition with
   `duplicate: true` instead of adding a second catalog row.
3. **The reading status.** `ensure_shelf` + `upsert_book_shelf` against
   `shelf:<status>`, the same local convention `sync_shelf_placement` uses. No
   record is written back to the PDS: the shelving record already exists there,
   and re-publishing it would be a write nobody asked for.

`create_virtual_book_inner` grew an `AuthorRequirement` (`Required` for the
manual form and the MCP tool, `Optional` for this import). The index has no
contributor names for some editions ("Was Hinduism Invented?" returns one
contributor with an empty name); refusing those books would drop a shelf entry
the reader has, and inventing an author would corrupt the catalog. The import
keeps the book with no author row.

The drawer is offered from the toolbar's *Add* dropdown only while
`atproto_status` reports a session, and it searches and filters the whole list
client-side (100 rows rendered at a time out of the matches).

## Consequences

Easier: the shelf list is one collection walk (~29 pages, a few seconds for 2 855
records, cached per drawer open) instead of thousands of index calls; the reading
status arrives with the book in the same command; a repeat import is a no-op; the
toolbar's dropdown value read was fixed to the property (`wa-dropdown-item` never
renders `value` as an attribute), so all three *Add* entries work.

Harder: the list shows titles and shelf badges only — covers and authors appear
once a book is imported, because per-row enrichment does not scale; a book already
in the library is not moved when its shelf changes upstream (the duplicate path
returns early after re-recording the local placement it was told about, but the
drawer's row stays "Already added"); the index's `eng` codes are not mapped to the
local `en` language ids, so imported books have no language recorded; and a shelf
the app does not model imports the book without a status.
