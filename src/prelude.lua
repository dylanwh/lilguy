function array(t)
    if t == nil then
        t = {}
    end
    setmetatable(t, array_mt)
    return t
end

commands = {}

Request = {}

function Request:cookie(name)
    return self._cookie_jar:get(name)
end

function Request:signed_cookie(name)
    return self._cookie_jar:get_signed(name)
end

function Request:private_cookie(name)
    return self._cookie_jar:get_private(name)
end

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

function Response:set_cookie(name, value)
    self._cookie_jar:set(name, value)
end

function Response:set_signed_cookie(name, value)
    self._cookie_jar:set_signed(name, value)
end

function Response:set_private_cookie(name, value)
    self._cookie_jar:set_private(name, value)
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

function collect(...)
    local t = {}
    for v in ... do
        table.insert(t, v)
    end
    return array(t)
end