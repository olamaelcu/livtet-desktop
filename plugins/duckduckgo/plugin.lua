-- DuckDuckGo cover-source enricher for Livtet
-- Implements the Enricher contract: sources() -> ["isbn", "asin"],
-- enrich(identity) -> Enrichment carrying a cover and nothing else.
--
-- This is the catch-all source: it searches the open web for a cover image when
-- the cataloguing sources have none. DuckDuckGo image search is two hops — a
-- token from the search page, then the JSON result list — and the winning image
-- lives on a host this plugin may not reach. That last hop goes through
-- DuckDuckGo's own image proxy instead, so the plugin only ever talks to two
-- allowlisted hosts and a result's origin is fetched by DuckDuckGo, not here.
--
-- A search that is challenged (DuckDuckGo answers 202 with a bot challenge),
-- empty, or yields nothing image-shaped is not an error: the answer is no cover.
--
-- `http` is a host capability: declaring it in plugin.toml binds it into this
-- chunk's environment, so it is called directly.

local Enricher = {}

local BROWSER_UA = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 " ..
    "(KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"
local SEARCH_HEADERS = {["user-agent"] = BROWSER_UA, ["accept"] = "text/html"}
-- The token endpoint answers only a same-origin request.
local RESULT_HEADERS = {
    ["user-agent"] = BROWSER_UA,
    ["accept"] = "application/json, text/javascript, */*; q=0.01",
    ["referer"] = "https://duckduckgo.com/"
}

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

local function urlencode(value)
    return (tostring(value or ""):gsub("[^%w%-%._~]", function(character)
        return string.format("%%%02X", string.byte(character))
    end))
end

-- A refused or challenged callback raises in Lua rather than answering nil, so
-- every request goes through `pcall`: a blocked hop costs a cover, not the whole
-- enrichment.
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

local function cover_at(url)
    local res = request({url = url, headers = {["user-agent"] = BROWSER_UA}})
    if not res or res.status ~= 200 then
        return nil
    end
    if type(res.body_base64) ~= "string" or #res.body_base64 == 0 then
        return nil
    end
    local cover = {mime = mime_of(res), data_base64 = res.body_base64}
    -- The proxy relays whatever the origin served; a search result that was not
    -- an image is not a cover.
    if cover.mime:sub(1, 6) ~= "image/" then
        return nil
    end
    return cover
end

-- The `vqd` token is embedded in the image-search page as `vqd="…"` (older
-- responses use `vqd=…` inside a script assignment).
local function search_token(query)
    local res = request({
        url = "https://duckduckgo.com/?q=" .. urlencode(query) .. "&iax=images&ia=images",
        headers = SEARCH_HEADERS
    })
    if not res or res.status ~= 200 or type(res.body) ~= "string" then
        return nil
    end
    return res.body:match('vqd="([%w%-_]+)"') or res.body:match("vqd=([%w%-_]+)")
end

-- The image results, newest page first. A challenged request answers with a JSON
-- object carrying `challenge` and no `results`, which is simply no result.
local function image_results(query, token)
    local res = request({
        url = "https://duckduckgo.com/i.js?l=us-en&o=json&q=" .. urlencode(query) ..
            "&vqd=" .. urlencode(token) .. "&f=,,,",
        headers = RESULT_HEADERS
    })
    if not res or res.status ~= 200 or type(res.json) ~= "table" then
        return {}
    end
    if type(res.json.results) ~= "table" then
        return {}
    end
    return res.json.results
end

-- Prefers a result whose own title names the book, since an image search for a
-- title returns the film, the sequel and the study guide beside the cover.
local function pick(results, title)
    local wanted = (title or ""):lower()
    local fallback
    for index, result in ipairs(results) do
        local image = result.image
        if type(image) == "string" and #image > 0 then
            if not fallback then
                fallback = image
            end
            if #wanted > 0 and type(result.title) == "string" then
                if result.title:lower():find(wanted, 1, true) then
                    return image
                end
            end
        end
        if index >= 8 then
            break
        end
    end
    return fallback
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

    -- Search by title and author when the edition has them: image search over an
    -- ISBN mostly returns scans of barcode pages.
    local query = ""
    if type(identity.title) == "string" and #identity.title > 0 then
        query = identity.title
        local author = identity.authors and identity.authors[1]
        if type(author) == "string" and #author > 0 then
            query = query .. " " .. author
        end
        query = query .. " book cover"
    else
        for _, identifier in ipairs(identity.identifiers or {}) do
            local kind = (identifier.kind or ""):lower()
            if kind == "isbn" or kind == "asin" then
                local value = strip(identifier.value)
                if #value > 0 then
                    query = value .. " book cover"
                    break
                end
            end
        end
    end
    if #query == 0 then
        return enrichment
    end

    local token = search_token(query)
    if not token then
        return enrichment
    end

    local image = pick(image_results(query, token), identity.title)
    if not image then
        return enrichment
    end

    enrichment.cover = cover_at("https://external-content.duckduckgo.com/iu/?u=" ..
        urlencode(image) .. "&f=1&nofb=1")
    return enrichment
end

return Enricher
