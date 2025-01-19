# API Reference

This reference documents all the core functionality of LilGuy, including routes, requests, responses, templates, and database operations.

## Routes

Routes define how your application responds to HTTP requests. They are defined in `app.lua`.

### Defining Routes

```lua
routes["/path"] = function(req, res)
    -- handle request
end
```

### Route Parameters

Dynamic segments in routes are prefixed with `:`:

```lua
routes["/users/:id"] = function(req, res)
    local id = req.params.id
    res:render("user.html", { user_id = id })
end
```

## Request Object

The request object (`req`) contains information about the HTTP request.

### Properties

- `req.method`: The HTTP method (GET, POST, etc.)
- `req.path`: The request path
- `req.query`: Table containing query parameters
- `req.headers`: Table containing request headers
- `req.body`: Request body (for POST/PUT requests)
- `req.params`: Table containing route parameters
- `req.cookies`: Table containing cookies

### Methods

```lua
-- Get a specific header
local value = req:header("Content-Type")

-- Check if request accepts a specific content type
if req:accepts("application/json") then
    -- handle JSON response
end

-- Get form data
local name = req:form("username")
```

## Response Object

The response object (`res`) is used to send the response to the client.

### Methods

#### render(template, context)
Renders a template with the given context:

```lua
res:render("page.html", {
    title = "Welcome",
    users = users_list
})
```

#### json(data)
Sends a JSON response:

```lua
res:json({
    status = "success",
    data = { name = "John" }
})
```

#### redirect(url)
Redirects to another URL:

```lua
res:redirect("/login")
```

### Properties

- `res.status`: Set the HTTP status code
- `res.headers`: Table of response headers
- `res.body`: Response body

## Templates

LilGuy uses Jinja2-style templates.

### Template Syntax

#### Variables
```html
<h1>{{ title }}</h1>
<p>{{ user.name }}</p>
```

#### Control Structures
```html
{% if user %}
    Hello, {{ user.name }}
{% else %}
    Please log in
{% endif %}

{% for item in items %}
    <li>{{ item.name }}</li>
{% endfor %}
```

#### Template Inheritance
```html
{# layout.html #}
<!DOCTYPE html>
<html>
    <head>
        <title>{% block title %}{% endblock %}</title>
    </head>
    <body>
        {% block content %}{% endblock %}
    </body>
</html>

{# page.html #}
{% extends "layout.html" %}

{% block title %}My Page{% endblock %}

{% block content %}
    <h1>Welcome</h1>
{% endblock %}
```

## Database

LilGuy uses SQLite for data storage.

### Query Execution

```lua
-- Simple query
database:query("SELECT * FROM users", function(users)
    -- handle results
end)

-- Parameterized query
database:query("SELECT * FROM users WHERE age > ?", {18}, function(users)
    -- handle results
end)
```

### Common Database Operations

```lua
-- Insert data
database:query(
    "INSERT INTO users (name, email) VALUES (?, ?)",
    {"John", "john@example.com"}
)

-- Update data
database:query(
    "UPDATE users SET name = ? WHERE id = ?",
    {"Jane", 1}
)

-- Delete data
database:query(
    "DELETE FROM users WHERE id = ?",
    {1}
)
```

## Global State

LilGuy provides a `global` table for persistent storage across requests.

```lua
-- Store a value
global.counters.visits = (global.counters.visits or 0) + 1

-- Retrieve a value
local visits = global.counters.visits
```

## Utilities

### JSON Handling

```lua
-- Encode to JSON
local json_string = json.encode({name = "John"})

-- Decode JSON
local data = json.decode(json_string)
```

### Markdown Processing

```lua
local html = markdown(markdown_text)
```

### Sleep and Timeout

```lua
-- Sleep for 2 seconds
sleep(2)

-- Set timeout for operation
timeout(5, function()
    -- operation that should complete within 5 seconds
end)
```

## HTMX Integration

LilGuy works seamlessly with HTMX for dynamic UI updates.

### HTMX Response Headers

```lua
-- Trigger client-side events
res.headers["HX-Trigger"] = "userUpdated"

-- Trigger client-side events with data
res.headers["HX-Trigger"] = json.encode({
    showMessage = "Update successful"
})
```

### HTMX Route Example

```lua
routes["/users/search"] = function(req, res)
    local query = req:form("query")
    database:query(
        "SELECT * FROM users WHERE name LIKE ?",
        {"%" .. query .. "%"},
        function(users)
            res:render("partials/user-list.html", {
                users = users
            })
        end
    )
end
```

## Command Line Interface

LilGuy provides several CLI commands:

### new
Creates a new project:
```bash
lilguy new myapp [--theme color]
```

### serve
Starts the development server:
```bash
lilguy serve [options]
  -l, --listen <addr>    Address to bind to
  --no-reload            Disable auto-reload
  -o, --open            Open browser
  -i, --interactive     Start interactive shell
```

### shell
Starts an interactive Lua shell:
```bash
lilguy shell
```

### query
Executes SQL queries:
```bash
lilguy query "SELECT * FROM users"
```

### run
Executes a specific function:
```bash
lilguy run function_name [args...]
```

[Next: Examples →](examples.md)