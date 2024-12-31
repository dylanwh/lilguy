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
    local response = { status = 200, headers = {}, body = "" }
end

local body_mt = {}
function body_mt:__tostring()
    return self.body
end