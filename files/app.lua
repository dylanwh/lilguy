routes["/"] = function(req, res)
    res:render("index.html", {})
end

routes["/docs"] = function(req, res)
    res:render("docs.html", {})
end

not_found = function(req, res)
    res:render("not_found.html", { req = req })
end
