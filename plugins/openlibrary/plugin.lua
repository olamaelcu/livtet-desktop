-- OpenLibrary enricher plugin for Livtet
-- Implements the Enricher contract: sources() -> ["isbn", "olid"], enrich(identity) -> Enrichment

local http = require("http")
local json = require("json")

local Enricher = {}

function Enricher.new(config, deps)
    return setmetatable({}, {__index = Enricher})
end

function Enricher:sources()
    return {"isbn", "olid"}
end

function Enricher:enrich(identity)
    local enrichment = {
        title = identity.title,
        title_sort = nil,
        contributors = {},
        publisher = nil,
        language = nil,
        published = nil,
        description = nil,
        subjects = {},
        cover = nil,
        found = {}
    }

    -- Helper to add a found identifier
    local function add_found(kind, value)
        table.insert(enrichment.found, {kind = kind, value = value})
    end

    -- Helper to fetch JSON from OpenLibrary
    local function fetch_json(url)
        local res, err = http.get(url)
        if not res then
            return nil, err
        end
        if res.status ~= 200 then
            return nil, "HTTP " .. res.status
        end
        local ok, data = pcall(json.decode, res.body)
        if not ok then
            return nil, "JSON decode failed: " .. data
        end
        return data, nil
    end

    -- Helper to fetch cover image as base64
    local function fetch_cover(cover_id)
        if not cover_id then return nil end
        local url = "https://covers.openlibrary.org/b/id/" .. cover_id .. "-L.jpg"
        local res, err = http.get(url)
        if not res or res.status ~= 200 then
            return nil
        end
        -- Return base64 encoded image
        local base64 = require("base64")
        return {mime = "image/jpeg", data_base64 = base64.encode(res.body)}
    end

    -- Try ISBN first
    for _, id in ipairs(identity.identifiers) do
        if id.kind == "isbn" then
            local isbn = id.value:gsub("%-", "") -- remove hyphens
            local data, err = fetch_json("https://openlibrary.org/isbn/" .. isbn .. ".json")
            if data then
                -- This returns an edition document
                -- Extract OLID from the edition
                if data.key then
                    local olid = data.key:match("/books/(OL%d+M)")
                    if olid then add_found("olid", olid) end
                end
                -- Also check for work OLIDs
                if data.works then
                    for _, work in ipairs(data.works) do
                        if work.key then
                            local work_olid = work.key:match("/works/(OL%d+W)")
                            if work_olid then add_found("olid", work_olid) end
                        end
                    end
                end
                -- Extract edition fields
                if data.title then enrichment.title = data.title end
                if data.publishers and #data.publishers > 0 then
                    enrichment.publisher = data.publishers[1].name
                end
                if data.publish_date then
                    -- Parse date: can be "2020", "2020-01", "2020-01-01"
                    local year, month, day = data.publish_date:match("(%d%d%d%d)[%-]?(%d?%d?)[%-]?(%d?%d?)")
                    if year then
                        enrichment.published = {year = tonumber(year), month = tonumber(month) or nil, day = tonumber(day) or nil}
                    end
                end
                if data.cover then
                    enrichment.cover = fetch_cover(data.cover)
                elseif data.covers and #data.covers > 0 then
                    enrichment.cover = fetch_cover(data.covers[1])
                end

                -- We'll also fetch the work for description and subjects if we have work OLID
                if data.works and #data.works > 0 and data.works[1].key then
                    local work_key = data.works[1].key
                    local work_data, _ = fetch_json("https://openlibrary.org" .. work_key .. ".json")
                    if work_data then
                        if work_data.description then
                            -- Description can be a string or {value: "..."}
                            if type(work_data.description) == "table" then
                                enrichment.description = work_data.description.value
                            else
                                enrichment.description = work_data.description
                            end
                        end
                        if work_data.subjects then
                            for _, subj in ipairs(work_data.subjects) do
                                table.insert(enrichment.subjects, subj)
                            end
                        end
                    end
                end

                -- Fetch authors
                if data.authors then
                    for _, author in ipairs(data.authors) do
                        if author.key then
                            local author_data, _ = fetch_json("https://openlibrary.org" .. author.key .. ".json")
                            if author_data and author_data.name then
                                table.insert(enrichment.contributors, {name = author_data.name, role = "author", file_as = nil})
                            end
                        elseif author.name then
                            table.insert(enrichment.contributors, {name = author.name, role = "author", file_as = nil})
                        end
                    end
                end

                -- If we got an OLID from this ISBN, we can also fetch the edition directly for more fields
                if data.key then
                    local edition_data, _ = fetch_json("https://openlibrary.org" .. data.key .. ".json")
                    if edition_data then
                        -- Might have more detailed info
                        if edition_data.title and not enrichment.title then enrichment.title = edition_data.title end
                        if edition_data.publishers and #edition_data.publishers > 0 and not enrichment.publisher then
                            enrichment.publisher = edition_data.publishers[1].name
                        end
                        if edition_data.languages and #edition_data.languages > 0 then
                            enrichment.language = edition_data.languages[1].key
                        end
                        if edition_data.cover and not enrichment.cover then
                            enrichment.cover = fetch_cover(edition_data.cover)
                        end
                    end
                end
            end
        end
    end

    -- Then try OLID (both edition and work)
    for _, id in ipairs(identity.identifiers) do
        if id.kind == "olid" then
            local olid = id.value
            -- Fetch edition
            local edition_data, err = fetch_json("https://openlibrary.org/books/" .. olid .. ".json")
            if edition_data then
                if edition_data.title and not enrichment.title then enrichment.title = edition_data.title end
                if edition_data.publishers and #edition_data.publishers > 0 and not enrichment.publisher then
                    enrichment.publisher = edition_data.publishers[1].name
                end
                if edition_data.publish_date and not enrichment.published then
                    local year, month, day = edition_data.publish_date:match("(%d%d%d%d)[%-]?(%d?%d?)[%-]?(%d?%d?)")
                    if year then
                        enrichment.published = {year = tonumber(year), month = tonumber(month) or nil, day = tonumber(day) or nil}
                    end
                end
                if edition_data.languages and #edition_data.languages > 0 and not enrichment.language then
                    enrichment.language = edition_data.languages[1].key
                end
                if edition_data.cover and not enrichment.cover then
                    enrichment.cover = fetch_cover(edition_data.cover)
                end
                -- Authors
                if edition_data.authors then
                    for _, author in ipairs(edition_data.authors) do
                        if author.key then
                            local author_data, _ = fetch_json("https://openlibrary.org" .. author.key .. ".json")
                            if author_data and author_data.name then
                                table.insert(enrichment.contributors, {name = author_data.name, role = "author", file_as = nil})
                            end
                        elseif author.name then
                            table.insert(enrichment.contributors, {name = author.name, role = "author", file_as = nil})
                        end
                    end
                end
                -- If this edition has a work key, fetch work for description/subjects
                if edition_data.works and #edition_data.works > 0 then
                    local work_key = edition_data.works[1].key
                    local work_data, _ = fetch_json("https://openlibrary.org" .. work_key .. ".json")
                    if work_data then
                        if work_data.description and not enrichment.description then
                            if type(work_data.description) == "table" then
                                enrichment.description = work_data.description.value
                            else
                                enrichment.description = work_data.description
                            end
                        end
                        if work_data.subjects then
                            for _, subj in ipairs(work_data.subjects) do
                                table.insert(enrichment.subjects, subj)
                            end
                        end
                    end
                end
            end

            -- Also fetch work directly if OLID looks like a work (OLIDW)
            if olid:match("^OL%d+W$") then
                local work_data, _ = fetch_json("https://openlibrary.org/works/" .. olid .. ".json")
                if work_data then
                    if work_data.title and not enrichment.title then enrichment.title = work_data.title end
                    if work_data.description and not enrichment.description then
                        if type(work_data.description) == "table" then
                            enrichment.description = work_data.description.value
                        else
                            enrichment.description = work_data.description
                        end
                    end
                    if work_data.subjects then
                        for _, subj in ipairs(work_data.subjects) do
                            table.insert(enrichment.subjects, subj)
                        end
                    end
                end
            end
        end
    end

    -- Fallback: if no cover found but we have ISBN, try ISBN cover
    if not enrichment.cover then
        for _, id in ipairs(identity.identifiers) do
            if id.kind == "isbn" then
                local isbn = id.value:gsub("%-", "")
                enrichment.cover = fetch_cover(isbn) -- fetch_cover handles ISBN fallback
                if enrichment.cover then break end
            end
        end
    end

    -- Deduplicate subjects
    local seen = {}
    local unique_subjects = {}
    for _, subj in ipairs(enrichment.subjects) do
        if not seen[subj] then
            seen[subj] = true
            table.insert(unique_subjects, subj)
        end
    end
    enrichment.subjects = unique_subjects

    -- Deduplicate contributors (by name)
    local seen_names = {}
    local unique_contributors = {}
    for _, contrib in ipairs(enrichment.contributors) do
        if not seen_names[contrib.name] then
            seen_names[contrib.name] = true
            table.insert(unique_contributors, contrib)
        end
    end
    enrichment.contributors = unique_contributors

    return enrichment
end

return Enricher