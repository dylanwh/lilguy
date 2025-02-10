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

Response = {}

function Response:render(name, context)
    local body = template:render(name, context)
    if body then
        if self.headers["Content-Type"] == nil then
            self.headers["Content-Type"] = "text/html"
        end
        self.body = body
    else
        self.status = 500
        self.body = "Error rendering template"
    end
end

function Response:redirect(url)
    self.status = 302
    self.headers["Location"] = url
end

function Response:json(data)
    self.headers["Content-Type"] = "application/json"
    self.body = json.encode(data)
end


function head(n, iter)
    local i = 0
    return function()
        i = i + 1
        if i <= n then
            return iter()
        end
    end
end

function collect(iter)
    local t = {}
    for v in iter do
        table.insert(t, v)
    end
    return array(t)
end

function map(f, iter)
    return function()
        local v = iter()
        if v == nil then
            return nil
        end
        return f(v)
    end
end

function filter(f, iter)
    return function()
        local v
        repeat
            v = iter()
        until v == nil or f(v)
        return v
    end
end
