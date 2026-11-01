-- Goodreads cover-source enricher for Livtet
-- Implements the Enricher contract: sources() -> ["isbn", "asin"],
-- enrich(identity) -> Enrichment carrying a cover and nothing else.
--
-- Goodreads answers an ISBN search with a redirect to the matching book page,
-- so the identifier resolves to a cover in two hops: read the page the host hands
-- back as text, take the cover path out of it, then fetch that image. The HTML is
-- read with plain Lua patterns — the `xml`/`json` capabilities are withdrawn
-- (ADR-0031 amendment), so nothing here may parse a tree.
--
-- `http` is a host capability: declaring it in plugin.toml binds it into this
-- chunk's environment, so it is called directly.

local Enricher = {}

-- Goodreads serves the full page to a browser-shaped request; an unset agent is
-- answered with a challenge page that carries no cover.
local BROWSER_UA = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 " ..
    "(KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"

local function strip(value)
    local raw = (value or ""):gsub("[%s%-]", "")
    if raw:sub(1, 9):lower() == "urn:isbn:" then
        return raw:sub(10)
    end
    if raw:sub(1, 10):lower() == "urn:asin:" then
        return raw:sub(11)
    end
    return raw
end

-- A query value is percent-encoded byte by byte: `?q=` carries an identifier, and
-- anything a source wrote beside the digits must not break the URL.
local function urlencode(value)
    return (tostring(value or ""):gsub("[^%w%-%._~]", function(character)
        return string.format("%%%02X", string.byte(character))
    end))
end

-- A refused or failed callback raises in Lua rather than answering nil, so every
-- request goes through `pcall`: a source that is down costs a cover, not the
-- whole enrichment.
local function request(parameters)
    local ok, res = pcall(http, parameters)
    if not ok then
        return nil
    end
    return res
end

local function mime_of(res)
    local content_type = res.headers and res.headers["content-type"]
    if type(content_type) ~= "string" then
        return "image/jpeg"
    end
    local mime = content_type:match("^%s*([%w%.%+%-]+/[%w%.%+%-]+)")
    if mime then
        return mime:lower()
    end
    return "image/jpeg"
end

-- The bytes travel as base64: `body` is lossy text and would corrupt an image.
local function cover_at(url)
    local res = request({url = url, headers = {["user-agent"] = BROWSER_UA}})
    if not res or res.status ~= 200 then
        return nil
    end
    if type(res.body_base64) ~= "string" or #res.body_base64 == 0 then
        return nil
    end
    return {mime = mime_of(res), data_base64 = res.body_base64}
end

-- Cover paths appear in the page as
-- `compressed.photo.goodreads.com/books/<stamp>i/<book id>`; the leading stamp
-- is part of the immutable path, so both halves are needed to address the image.
local function cover_for(query)
    -- No `accept` header: Goodreads answers an HTML-shaped request by looping on
    -- the book page, and only a plain fetch reaches it.
    local res = request({
        url = "https://www.goodreads.com/search?q=" .. urlencode(query),
        headers = {["user-agent"] = BROWSER_UA}
    })
    if not res or res.status ~= 200 or type(res.body) ~= "string" then
        return nil
    end
    local stamp, book = res.body:match(
        "compressed%.photo%.goodreads%.com/books/(%d+)i/(%d+)")
    if not book then
        return nil
    end
    return cover_at("https://i.gr-assets.com/images/S/compressed.photo.goodreads.com/books/" ..
        stamp .. "i/" .. book .. ".jpg")
end

function Enricher.new(config, deps)
    return setmetatable({}, {__index = Enricher})
end

-- Roles this enricher plays. The app lists a plugin by what it declares
-- here rather than by its name, so a new source needs no app change.
function Enricher:provides()
    return {"cover"}
end

function Enricher:sources()
    return {"isbn", "asin"}
end

function Enricher:enrich(identity)
    local enrichment = {contributors = {}, subjects = {}, found = {}}

    for _, identifier in ipairs(identity.identifiers or {}) do
        local kind = (identifier.kind or ""):lower()
        if kind == "isbn" or kind == "asin" then
            local query = strip(identifier.value)
            if #query > 0 then
                enrichment.cover = cover_for(query)
                if enrichment.cover then
                    return enrichment
                end
            end
        end
    end

    return enrichment
end

return Enricher
