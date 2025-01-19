# Getting Started with LilGuy

This guide will walk you through installing LilGuy and creating your first web application.

## Installation

LilGuy is distributed as a single binary, making installation straightforward. Choose the installation method for your operating system:

### Windows
1. Download the Windows installer (`lilguy-installer.msi`) from the releases page
2. Double-click the installer and follow the prompts
3. LilGuy will be added to your system PATH automatically

### macOS
1. Download the macOS installer package (`lilguy.pkg`) from the releases page
2. Double-click the package and follow the installation prompts
3. LilGuy will be installed to `/usr/local/bin`

### Linux
1. Download the appropriate binary for your architecture:
   - `lilguy-linux-amd64` for 64-bit Intel/AMD systems
   - `lilguy-linux-arm64` for 64-bit ARM systems (like Raspberry Pi)
2. Make the binary executable:
   ```bash
   chmod +x lilguy-linux-*
   ```
3. Move it to your PATH:
   ```bash
   sudo mv lilguy-linux-* /usr/local/bin/lilguy
   ```

## Verifying Installation

Open a terminal or command prompt and run:
```bash
lilguy --version
```

You should see the version number printed to confirm successful installation.

## LilGuy Commands

LilGuy comes with several built-in commands:

- `lilguy new <project-name>`: Create a new project
- `lilguy serve`: Run the development server
- `lilguy shell`: Start an interactive Lua shell
- `lilguy run <function>`: Execute a specific function
- `lilguy query "SQL"`: Run SQL queries against the database

## Creating Your First Project

1. Create a new project:
   ```bash
   lilguy new myapp
   cd myapp
   ```

2. Start the development server:
   ```bash
   lilguy serve
   ```

Your app will be available at `http://localhost:8000`

### Project Structure

A new LilGuy project has this structure:
```
myapp/
├── app.lua        # Main application logic
├── assets/        # Static files (CSS, images, etc.)
│   └── pico.css   # Default styling
└── templates/     # HTML templates
    ├── index.html
    └── layout.html
```

## Development Server Options

The `serve` command has several useful options:

```bash
lilguy serve --help
Options:
  -l, --listen <addr>  Address to bind to [default: 0.0.0.0:8000]
  --no-reload          Disable auto-reload on file changes
  -o, --open          Open browser automatically
  -i, --interactive   Start an interactive Lua shell
```

## Basic Usage

### Routes

Routes are defined in `app.lua`. Here's a simple example:

```lua
routes["/"] = function(req, res)
    res:render("index.html", {
        message = "Welcome to LilGuy!"
    })
end

routes["/about"] = function(req, res)
    res:render("about.html", {
        title = "About Us"
    })
end
```

### Templates

Templates use Jinja2 syntax. Here's a basic template example:

```html
{% extends "layout.html" %}

{% block content %}
<h1>{{ message }}</h1>
<p>This is a LilGuy app!</p>
{% endblock %}
```

### Database

LilGuy uses SQLite for data storage. You can interact with the database in several ways:

1. Direct SQL queries:
```bash
lilguy query "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)"
```

2. From Lua code:
```lua
routes["/users"] = function(req, res)
    database:query("SELECT * FROM users", function(users)
        res:render("users.html", { users = users })
    end)
end
```

## Next Steps

Now that you're familiar with the basics of LilGuy, you can:

- Learn about all available functions and features in the [API Reference](api-reference.md)
- Check out the [Examples](examples.md) for common usage patterns
- Join the community to get help and share your projects

[Next: API Reference →](api-reference.md)

## Common Commands Reference

Here's a quick reference of common commands you'll use:

```bash
# Create a new project
lilguy new myapp

# Start the development server
lilguy serve

# Start the server and open browser
lilguy serve --open

# Start server with interactive shell
lilguy serve --interactive

# Run SQL queries
lilguy query "SELECT * FROM sqlite_master"

# Start an interactive shell
lilguy shell
```