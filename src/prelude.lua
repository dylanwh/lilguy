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
    return self.cookie_jar:get(name)
end

function Request:signed_cookie(name)
    return self.cookie_jar:get_signed(name)
end

function Request:private_cookie(name)
    return self.cookie_jar:get_private(name)
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
    self.cookie_jar:set(name, value)
end

function Response:set_signed_cookie(name, value)
    self.cookie_jar:set_signed(name, value)
end

function Response:set_private_cookie(name, value)
    self.cookie_jar:set_private(name, value)
end

function collect(...)
    local t = {}
    for v in ... do
        table.insert(t, v)
    end
    return array(t)
end

function take(n,  iter, state, initial)
    -- Return a stateful iterator
    local count = 0
    local done = false

    return function(s, var)
        -- If we've reached our limit or previously finished, stop iteration
        if done or count >= n then
            return nil
        end

        -- Get next value(s) from the source iterator
        local val = iter(s, var)

        -- If the original iterator is done, mark as done
        if val == nil then
            done = true
            return nil
        end

        -- Increment our counter
        count = count + 1

        -- Return the current value
        return val
    end, state, initial
end

-- Function to drop the first n items from an iterator
function drop(n, iter, state, initial)
    -- Skip the first n items
    local var = initial
    local skipped = 0

    -- Skip n items
    while skipped < n do
        var = iter(state, var)
        if var == nil then
            -- If we run out of items while skipping, return empty iterator
            return function()
                return nil
            end, state, initial
        end
        skipped = skipped + 1
    end

    -- Return an iterator starting from item n+1
    return function(s, var)
        local val = iter(s, var)
        return val
    end, state, var
end
