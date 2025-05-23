routes["/"] = function(req, res)
    res:render("index.html", {})
end

routes["/docs"] = function(req, res)
    res:redirect("https://lilguy.app")
end


-- this is called from an htmx button
local clicks = global.clicks
routes["/click/:name"] = function(req, res)
    local name = req.params.name
    if not clicks[name] then
        clicks[name] = 0
    end
    clicks[name] = clicks[name] + 1
    res.headers['Content-Type'] = 'application/html'
    res.body = string.format("Clicked %s %d times", name, clicks[name])
end

routes.not_found = function(req, res)
    res.status = 404
    res:render("not_found.html", {
        req = req
    })
end

