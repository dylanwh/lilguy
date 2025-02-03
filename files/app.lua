routes["/"] = function(req, res)
    req.cookies["test"] = "test"
    res:render("index.html", {})
end

routes["/docs"] = function(req, res)
    res:redirect("https://lilguy.app")
end

routes.not_found = function(req, res)
    res.status = 404
    res:render("not_found.html", {
        req = req
    })
end

-- routes.websocket = function(req, socket)
--     socket:send("Hello, World!")
--     local msg = socket:recv()
-- end
