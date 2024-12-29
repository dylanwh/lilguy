WATCH_FILES = {}

-- Store the original searcher
local original_searcher = package.searchers[2]

-- Create a custom searcher that tracks files
-- index 2 is the Lua searcher
package.searchers[2] = function(modname)
    local filename = package.searchpath(modname, package.path)
    if filename then
        WATCH_FILES[modname] = filename
    end
    return original_searcher(modname)
end

function array(t)
    if t == nil then
        t = {}
    end
    setmetatable(t, array_mt)
    return t
end

-- re-implement ipairs using using pure lua to avoid problems with crossing C boundaries
function ipairs(t)
    -- lua 5.4 removed the __ipairs metamethod but it would be useful for performance
    -- so we'll just implement it ourselves
    -- TODO: actually use this for global tables
    local mt = getmetatable(t)
    if mt and mt.__ipairs then
        return mt.__ipairs(t)
    end

    local i = 0
    return function(a, b, c, d)
        i = i + 1
        if t[i] then
            return i, t[i]
        end
    end
end

commands = {}

function serve(req)
    local route = routes[req.path]
    if route then
        local result = { route(req) }
        print("result", #result)
        if #result == 1 then
            return 200, {["Content-Type"] = "text/plain"}, result[1]
        elseif #result == 2 then
            return 200, result[1], result[2]
        elseif #result == 3 then
            return result[1], result[2], result[3]
        else
            return 500, {}, "Internal Server Error"
        end
    else
        return 404, {}, "Not Found"
    end
end