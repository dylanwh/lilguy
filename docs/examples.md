# LilGuy Examples

This guide provides practical examples of building applications with LilGuy. Each example demonstrates different features of the framework and how they work together.

## Basic Examples

### Hello World

The simplest possible LilGuy application:

```lua
-- app.lua
routes["/"] = function(req, res)
    res:render("index.html", {
        message = "Hello from LilGuy!"
    })
end
```

```html
<!-- templates/index.html -->
{% extends "layout.html" %}

{% block content %}
<h1>{{ message }}</h1>
{% endblock %}
```

### Working with Forms

A simple form that handles both GET and POST requests:

```lua
-- app.lua
routes["/contact"] = function(req, res)
    if req.method == "POST" then
        -- Access form data
        local name = req.form.name
        local email = req.form.email
        local message = req.form.message
        
        -- Store in database
        database:execute([[
            INSERT INTO messages (name, email, message) 
            VALUES (?, ?, ?)
        ]], name, email, message)
        
        res:redirect("/contact?success=true")
    else
        res:render("contact.html", {
            success = req.query.success
        })
    end
end
```

```html
<!-- templates/contact.html -->
{% extends "layout.html" %}

{% block content %}
<article>
    {% if success %}
    <div class="alert alert-success">
        Message sent successfully!
    </div>
    {% endif %}

    <form method="POST">
        <label for="name">Name</label>
        <input type="text" id="name" name="name" required>
        
        <label for="email">Email</label>
        <input type="email" id="email" name="email" required>
        
        <label for="message">Message</label>
        <textarea id="message" name="message" required></textarea>
        
        <button type="submit">Send Message</button>
    </form>
</article>
{% endblock %}
```

## Database Examples

### Todo List Application

A complete todo list showing database operations:

```lua
-- app.lua

-- Initialize database table
database:execute([[
    CREATE TABLE IF NOT EXISTS todos (
        id INTEGER PRIMARY KEY,
        task TEXT NOT NULL,
        completed BOOLEAN DEFAULT 0,
        created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
    )
]])

-- List todos
routes["/todos"] = function(req, res)
    local todos = database:query([[
        SELECT * FROM todos ORDER BY created_at DESC
    ]])
    
    res:render("todos.html", { todos = todos })
end

-- Add new todo
routes["/todos/add"] = function(req, res)
    if req.method == "POST" then
        local task = req.form.task
        database:execute(
            "INSERT INTO todos (task) VALUES (?)",
            task
        )
        res:redirect("/todos")
    end
end

-- Toggle todo completion
routes["/todos/toggle/:id"] = function(req, res)
    database:execute([[
        UPDATE todos 
        SET completed = NOT completed 
        WHERE id = ?
    ]], req.params.id)
    
    res:redirect("/todos")
end
```

```html
<!-- templates/todos.html -->
{% extends "layout.html" %}

{% block content %}
<article>
    <header>
        <h1>Todo List</h1>
    </header>

    <form action="/todos/add" method="POST" class="grid">
        <input type="text" name="task" placeholder="What needs to be done?" required>
        <button type="submit">Add Todo</button>
    </form>

    <ul>
        {% for todo in todos %}
        <li>
            <a href="/todos/toggle/{{ todo.id }}" 
               class="{% if todo.completed %}completed{% endif %}">
                {{ todo.task }}
            </a>
        </li>
        {% endfor %}
    </ul>
</article>
{% endblock %}
```

## Dynamic UI with HTMX

### Live Search Example

Demonstrates real-time search using HTMX:

```lua
-- app.lua
routes["/search"] = function(req, res)
    res:render("search.html")
end

routes["/search/results"] = function(req, res)
    local query = req.query.q or ""
    local results = database:query([[
        SELECT * FROM items 
        WHERE name LIKE ? 
        LIMIT 10
    ]], "%" .. query .. "%")
    
    res:render("search_results.html", { results = results })
end
```

```html
<!-- templates/search.html -->
{% extends "layout.html" %}

{% block content %}
<article>
    <input type="search" 
           name="q" 
           placeholder="Search..."
           hx-get="/search/results"
           hx-trigger="input changed delay:500ms"
           hx-target="#results">
    
    <div id="results">
        <!-- Results will be loaded here -->
    </div>
</article>
{% endblock %}
```

```html
<!-- templates/search_results.html -->
{% if results|length > 0 %}
    <ul>
        {% for item in results %}
        <li>{{ item.name }}</li>
        {% endfor %}
    </ul>
{% else %}
    <p>No results found</p>
{% endif %}
```

## Global State Management

Example using LilGuy's global state feature:

```lua
-- app.lua

-- Initialize counter in global state
local counters = global.counters
counters[1] = { count = 0 }

routes["/counter"] = function(req, res)
    if req.method == "POST" then
        local current = counters[1].count
        counters[1] = { count = current + 1 }
        
        -- If this is an HTMX request, just return the counter value
        if req.headers["HX-Request"] then
            res:render("counter_value.html", { count = current + 1 })
            return
        end
    end
    
    res:render("counter.html", { count = counters[1].count })
end
```

```html
<!-- templates/counter.html -->
{% extends "layout.html" %}

{% block content %}
<article>
    <h1>Counter Example</h1>
    
    <div id="counter">
        {% include "counter_value.html" %}
    </div>
    
    <button hx-post="/counter"
            hx-target="#counter">
        Increment
    </button>
</article>
{% endblock %}
```

```html
<!-- templates/counter_value.html -->
<p>Current count: {{ count }}</p>
```

## Custom Error Pages

Customizing the 404 page:

```lua
-- app.lua
function not_found(req, res)
    res.status = 404
    res:render("404.html", {
        path = req.path
    })
end
```

```html
<!-- templates/404.html -->
{% extends "layout.html" %}

{% block content %}
<article>
    <header>
        <h1>Page Not Found</h1>
    </header>
    
    <p>Sorry, the page <code>{{ path }}</code> doesn't exist.</p>
    <p><a href="/">Return to homepage</a></p>
</article>
{% endblock %}
```

## Working with JSON APIs

Creating a simple JSON API:

```lua
-- app.lua
routes["/api/items"] = function(req, res)
    if req.method == "POST" then
        local item = json.decode(req.body)
        
        database:execute([[
            INSERT INTO items (name, description) 
            VALUES (?, ?)
        ]], item.name, item.description)
        
        res:json({ status = "success" })
    else
        local items = database:query("SELECT * FROM items")
        res:json(items)
    end
end
```

## File Upload Example

Handling file uploads:

```lua
-- app.lua
routes["/upload"] = function(req, res)
    if req.method == "POST" then
        local file = req.files.photo
        
        if file then
            -- Save file info to database
            database:execute([[
                INSERT INTO uploads (
                    filename, 
                    content_type, 
                    size
                ) VALUES (?, ?, ?)
            ]], file.filename, file.content_type, file.size)
            
            -- Move file to permanent storage
            file:save("uploads/" .. file.filename)
            
            res:redirect("/upload?success=true")
        end
    end
    
    res:render("upload.html", {
        success = req.query.success
    })
end
```

```html
<!-- templates/upload.html -->
{% extends "layout.html" %}

{% block content %}
<article>
    <h1>File Upload</h1>
    
    {% if success %}
    <div role="alert">File uploaded successfully!</div>
    {% endif %}
    
    <form method="POST" enctype="multipart/form-data">
        <label for="photo">Choose a photo:</label>
        <input type="file" 
               id="photo" 
               name="photo" 
               accept="image/*" 
               required>
        
        <button type="submit">Upload</button>
    </form>
</article>
{% endblock %}
```

These examples demonstrate many of LilGuy's core features, including:

- Routing and request handling
- Template rendering
- Database operations
- Form processing
- HTMX integration
- Global state management
- File uploads
- JSON APIs
- Error handling

Each example is designed to be practical and reusable, serving as a starting point for your own applications.