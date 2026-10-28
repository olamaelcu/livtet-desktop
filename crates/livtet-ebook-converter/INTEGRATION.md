AWZ3→EPUB conversion (livtet-ebook-converter) wire point.

Crate: crates/livet-ebook-converter/ (built, boko 0.5.0 library + CLI fallback).
Flags: --input-type azw3 --output-type epub <src> <dst>
Library path (enabled in src/main.rs): boko::Book::open -> EpubExporter::new().export.

Desktop import pipeline (reader.rs): after AZW3 metadata read, invoke binary (or library via crate dep) with above args. On success compute inventory fields and insert via digital_inventory::ActiveModel.

No schema change needed; entity fields (digital_inventory.rs):
edition_id, file_path (abs dst epub), file_format Some("epub"), file_size_bytes, file_hash, cover_path, notes = "Converted from AZW3 via livtet-ebook-converter", added_at = now, updated_at = Some(now).
Pattern verified at reader.rs ~1479 / byte_server.rs ~297.
