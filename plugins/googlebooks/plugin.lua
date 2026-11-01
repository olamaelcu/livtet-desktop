-- Google Books cover-source enricher for Livtet
-- Implements the Enricher contract: sources() -> ["isbn", "google_books"],
-- enrich(identity) -> Enrichment carrying a cover and nothing else.
--
-- A volume id addresses the cover directly; an ISBN needs one lookup to become a
-- volume id. That lookup is the only thing the API key is for, and the key is
-- not here: the application attaches its compiled-in key to this plugin's
-- `www.googleapis.com/books/v1/…` requests on the far side of the sandbox
-- boundary, so the secret never enters this chunk.
--
-- `http` is a host capability: declaring it in plugin.toml binds it into this
-- chunk's environment, so it is called directly.

local Enricher = {}

local function strip(value)
    local raw = (value or ""):gsub("%s", "")
    -- "urn:google_books:" is 17 bytes; the volume id follows it.
    if raw:sub(1, 17):lower() == "urn:google_books:" then
        return raw:sub(18)
    end
    if raw:sub(1, 9):lower() == "urn:isbn:" then
        return (raw:sub(10)):gsub("%-", "")
    end
    return (raw:gsub("%-", ""))
end

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
    local res = request({url = url})
    if not res or res.status ~= 200 then
        return nil
    end
    if type(res.body_base64) ~= "string" or #res.body_base64 == 0 then
        return nil
    end
    local cover = {mime = mime_of(res), data_base64 = res.body_base64}
    if cover.mime:sub(1, 6) ~= "image/" then
        return nil
    end
    return cover
end

-- Google serves the same cover at several sizes; `zoom=0` is the largest the
-- publisher allowed.
local function cover_for_volume(volume)
    return cover_at("https://books.google.com/books/content?id=" ..
        urlencode(volume) .. "&printsec=frontcover&img=1&zoom=0")
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
    return {"isbn", "google_books"}
end

-- `https://www.googleapis.com/books/v1/volumes…`. The request carries no key: the
-- application appends its own to this host and path before the call leaves, and a
-- plugin cannot see or spoof that.
--
-- The API sheds load with 429/503 often enough that a single attempt reads as "no
-- such book", so a shed answer is retried. This is the one hop that gets retries:
-- the image CDN does not have this problem.
local API_ATTEMPTS = 3

function Enricher:api(path, query)
    local url = "https://www.googleapis.com/books/v1/volumes" .. path
    if query and #query > 0 then
        url = url .. "?" .. query
    end
    for _ = 1, API_ATTEMPTS do
        local res = request({url = url, headers = {["accept"] = "application/json"}})
        if not res then
            return nil
        end
        if res.status == 200 and type(res.json) == "table" then
            return res.json
        end
        -- 429 and 5xx are the API shedding load; anything else is an answer.
        if res.status ~= 429 and res.status < 500 then
            return nil
        end
    end
    return nil
end

-- The volume id behind an ISBN, or nil when Google Books has no such volume.
function Enricher:volume_for_isbn(isbn)
    local body = self:api("", "q=" .. urlencode("isbn:" .. isbn))
    if not body then
        return nil
    end
    local items = body.items
    if type(items) ~= "table" or #items == 0 then
        return nil
    end
    local item = items[1]
    if type(item) ~= "table" or type(item.id) ~= "string" then
        return nil
    end
    return item.id
end

function Enricher:enrich(identity)
    local enrichment = {contributors = {}, subjects = {}, found = {}}

    for _, identifier in ipairs(identity.identifiers or {}) do
        local kind = (identifier.kind or ""):lower()
        local volume
        if kind == "google_books" then
            local value = strip(identifier.value)
            if #value > 0 then
                volume = value
            end
        elseif kind == "isbn" then
            local isbn = strip(identifier.value)
            if #isbn > 0 then
                volume = self:volume_for_isbn(isbn)
            end
        end
        if volume then
            enrichment.cover = cover_for_volume(volume)
            if enrichment.cover then
                return enrichment
            end
        end
    end

    return enrichment
end

return Enricher
