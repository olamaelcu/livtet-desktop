-- Amazon cover-source enricher for Livtet
-- Implements the Enricher contract: sources() -> ["isbn", "asin"],
-- enrich(identity) -> Enrichment carrying a cover and nothing else.
--
-- Amazon's image service addresses a cover by the ISBN-10 or ASIN of the print
-- or Kindle edition, so an ISBN-13 has to be converted first: a 978-prefixed
-- ISBN is a trivial checksum change, a 979 is not an ISBN-10 at all and Amazon
-- has no cover address for it.
--
-- `http` is a host capability: declaring it in plugin.toml binds it into this
-- chunk's environment, so it is called directly. It is not a LuaRocks module and
-- `require` cannot see it (ADR-0031).

local Enricher = {}

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

-- Identifier values reach a plugin in their stored form, which may carry a
-- `urn:isbn:` / `urn:asin:` prefix beside separators from the source metadata.
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

-- The ISBN-10 Amazon keys covers on, or nil when this identifier has none.
local function isbn10(value)
    local isbn = strip(value)
    if #isbn == 10 then
        return isbn
    end
    if #isbn ~= 13 or isbn:sub(1, 3) ~= "978" then
        return nil
    end
    local core = isbn:sub(4, 12)
    local sum = 0
    for i = 1, 9 do
        local digit = tonumber(core:sub(i, i))
        if not digit then
            return nil
        end
        sum = sum + digit * (11 - i)
    end
    local check = 11 - (sum % 11)
    if check == 10 then
        check = "X"
    elseif check == 11 then
        check = "0"
    else
        check = tostring(check)
    end
    return core .. check
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

-- The image type the host reports, so a PNG cover is stored as a PNG.
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
    local res = request({url = url})
    if not res or res.status ~= 200 then
        return nil
    end
    if type(res.body_base64) ~= "string" or #res.body_base64 == 0 then
        return nil
    end
    return {mime = mime_of(res), data_base64 = res.body_base64}
end

function Enricher:enrich(identity)
    local enrichment = {contributors = {}, subjects = {}, found = {}}

    for _, identifier in ipairs(identity.identifiers or {}) do
        local kind = (identifier.kind or ""):lower()
        local key
        if kind == "isbn" then
            key = isbn10(identifier.value)
        elseif kind == "asin" then
            key = strip(identifier.value)
        end
        if key and #key > 0 then
            -- `_SCLZZZZZZZ_` asks for the large rendering; the plain `.01.jpg`
            -- variant is the small one.
            enrichment.cover = cover_at("https://m.media-amazon.com/images/P/" .. key ..
                ".01._SCLZZZZZZZ_.jpg")
            if enrichment.cover then
                return enrichment
            end
        end
    end

    -- No usable identifier is not an error: the caller decides what an empty
    -- answer means.
    return enrichment
end

return Enricher
