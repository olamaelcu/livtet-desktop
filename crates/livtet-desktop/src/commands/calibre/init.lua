-- Calibre library importer.
--
-- A LibraryImporter that reads a Calibre library's `metadata.db` (via the host
-- `sqlite` module) and yields one BookRecord per book. Paths are relative to the
-- library root; the application resolves and imports them.

local Calibre = {}
Calibre.__index = Calibre

function Calibre.new(config, deps)
  return setmetatable({}, Calibre)
end

local function rows(db, sql, params)
  return sqlite.query(db, sql, params or {})
end

local function scalar(list, field)
  local row = list[1]
  if row ~= nil then
    return row[field]
  end
  return nil
end

function Calibre:scan(source)
  local db = source .. "/metadata.db"
  local books = rows(db,
    "SELECT id, title, sort, path, pubdate, has_cover FROM books ORDER BY id")

  local records = {}
  for _, book in ipairs(books) do
    local id = book.id

    local authors = rows(db,
      "SELECT a.name AS name, a.sort AS sort FROM authors a "
      .. "JOIN books_authors_link l ON l.author = a.id "
      .. "WHERE l.book = ? ORDER BY l.id", { id })
    local contributors = {}
    for i, a in ipairs(authors) do
      contributors[i] = { name = a.name, role = "aut", file_as = a.sort }
    end

    local isbns, others = {}, {}
    for _, ident in ipairs(rows(db, "SELECT type, val FROM identifiers WHERE book = ?", { id })) do
      if ident.type == "isbn" then
        isbns[#isbns + 1] = ident.val
      else
        others[#others + 1] = ident.type .. ":" .. ident.val
      end
    end

    local subjects = {}
    for i, t in ipairs(rows(db,
      "SELECT t.name AS name FROM tags t JOIN books_tags_link l ON l.tag = t.id "
      .. "WHERE l.book = ? ORDER BY t.name", { id })) do
      subjects[i] = t.name
    end

    local language = scalar(rows(db,
      "SELECT lc.lang_code AS code FROM languages lc "
      .. "JOIN books_languages_link l ON l.lang_code = lc.id "
      .. "WHERE l.book = ? ORDER BY l.item_order LIMIT 1", { id }), "code")

    local publisher = scalar(rows(db,
      "SELECT p.name AS name FROM publishers p "
      .. "JOIN books_publishers_link l ON l.publisher = p.id "
      .. "WHERE l.book = ? LIMIT 1", { id }), "name")

    local description = scalar(rows(db, "SELECT text FROM comments WHERE book = ? LIMIT 1", { id }), "text")

    local published = nil
    if type(book.pubdate) == "string" then
      local y, m, d = book.pubdate:match("^(%d%d%d%d)%-(%d%d)%-(%d%d)")
      if y then
        published = { year = tonumber(y), month = tonumber(m), day = tonumber(d) }
      end
    end

    local files = {}
    for _, data in ipairs(rows(db, "SELECT format, name FROM data WHERE book = ?", { id })) do
      local format = string.lower(data.format)
      files[#files + 1] = { path = book.path .. "/" .. data.name .. "." .. format, format = format }
    end

    local cover_path = nil
    if book.has_cover == 1 then
      cover_path = book.path .. "/cover.jpg"
    end

    records[#records + 1] = {
      meta = {
        title = book.title,
        title_sort = book.sort,
        contributors = contributors,
        isbns = isbns,
        other_identifiers = others,
        publisher = publisher,
        language = language,
        published = published,
        description = description,
        subjects = subjects,
      },
      files = files,
      cover_path = cover_path,
    }
  end

  return records
end

return Calibre
