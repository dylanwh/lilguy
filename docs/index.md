# LilGuy - Small Scale Web Development

LilGuy is a friendly, lightweight web framework that makes building web applications fun and productive. Perfect for hobbyists, learners, and anyone who wants to make their computer do useful things without the complexity of professional-grade frameworks.

## Runs Anywhere

LilGuy is incredibly lightweight and can run on:
- Old Raspberry Pi devices (Pi 3 and newer)
- Home routers (tested on ASUS)
- Windows computers
- macOS
- Any Linux system

You don't need a powerful server or cloud hosting - LilGuy is happy running on modest hardware you might already have at home.

## Core Features

- 🚀 **Zero Configuration**: Start building immediately with sensible defaults
- 📝 **Simple Syntax**: Write backend logic in Lua, a clean and approachable language
- 🎨 **Beautiful by Default**: Built-in Pico CSS for clean, responsive designs
- ⚡ **Dynamic UI**: Seamless HTMX integration for interactive experiences
- 🗄️ **Built-in Database**: SQLite storage that just works
- 🔄 **Live Reload**: See your changes instantly during development
- 📐 **Templates**: Familiar Jinja2 syntax for your views
- 💨 **Lightweight**: Runs smoothly on minimal hardware

## Perfect For

- Home automation projects
- Personal websites and blogs
- Learning web development
- DIY home dashboards
- Small business websites
- Family photo galleries
- Recipe collections
- Book libraries
- Local community sites
- Home inventory systems

While LilGuy isn't designed for large-scale commercial applications, it's plenty fast for personal projects and local applications. It's ideal for:

- Hobbyists who want to create useful web applications
- Beginners learning web development
- People who want to run services on their home network
- Anyone who wants to make a computer do useful things without complexity

## Example: A Complete Route in 5 Lines

```lua
routes["/hello"] = function(req, res)
    res:render("hello.html", {
        message = "Welcome to LilGuy!"
    })
end
```

```html
<!-- templates/hello.html -->
<h1>{{ message }}</h1>
```

## Key Technologies

LilGuy brings together the best of:

- **Lua**: A friendly, powerful scripting language for your backend logic
- **HTMX**: Add dynamic behaviors without writing JavaScript
- **Pico CSS**: Beautiful, semantic HTML with minimal effort
- **SQLite**: Reliable, zero-config database storage
- **Jinja2**: Familiar, powerful templating syntax

## Why Choose LilGuy?

LilGuy is designed for developers who want to:

- Build web applications quickly without complex setup
- Work with a simple, cohesive tech stack
- Focus on features instead of configuration
- Create fast web interfaces
- Have a great development experience out of the box
- Run applications on minimal hardware they already own

[Get Started →](getting-started.md)