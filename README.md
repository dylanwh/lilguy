# LilGuy Web Framework

LilGuy is a simple web framework using Lua, SQLLite templates, and live reload. 

LilGuy is implemented on Lua, SQLLite and MiniJinja. The Lua files and templates are monitored and when updates are made, the server reloads the Lua files and templates to refresh the web and server interface. This allows for the server to be updated and changes to be seen in real time. 

## Getting Started

LilGuy can be downloaded and compiled from source or using the available installers or packages. Installers are available for Windows 64-bit, MacOS (Universal), and x86_64 or aarch64 Linux.
- [Windows](https://github.com/dylanwh/lilguy/releases/download/v0.1.3/lilguy-0.1.3-x86_64.msi)
- [MacOS](https://github.com/dylanwh/lilguy/releases/download/v0.1.3/lilguy-0.1.3.pkg)
- [Linux x86_64](https://github.com/dylanwh/lilguy/releases/download/v0.1.3/lilguy-0.1.3-linux-x86_64.tar.zst)
- [Linux ARM_64](https://github.com/dylanwh/lilguy/releases/download/v0.1.3/lilguy-0.1.3-linux-aarch64.tar.zst)
- [FreeBSD x86_64](https://github.com/dylanwh/lilguy/releases/download/v0.1.3/lilguy-0.1.3-freebsd-x86_64.tar.zst)

### Prerequisites

There are no prerequisites for LilGuy as it is statically compiled. 
However, the system does not have a version of tar which supports ZStandard compression you will need to install a tar which supports ZStandard.

### Installation
For Windows and Mac installation instructions, please see the [website](https://lilguy.app/installation.html).
For Linux, entering the following command will create a directory, and inside of that directory will be the lilguy executable file which can be moved to the desired location.
```
tar -xvf lilguy-0.1.3-linux-x86_64.tar.zst
```
1.	In the new terminal window, enter `lilguy new project-name` and press enter. Replace project-name should be replaced with the name of the project.
2.	Make the top level of the project folder the active directory using `cd project-name`.
3.	The `lilguy serve --open` command will start the LilGuy server process `lilguy serve` and also `--open` a browser window to the LilGuy default web page.
4.	The terminal window will continue showing output for the LilGuy server process. To shutdown the LilGuy server, press `Ctrl+C` in the terminal. The LilGuy server will also close when the computer is restarted or shut down.
5.	The app.lua file or the HTML templates in the project-name/templates directory can be updated to make changes to the server. Unless the `lilguy serve` is flagged with `--no-reload`, these will update on the server in real time, allowing changes to be viewed immediately.

## Deployment
It is recommended when deploying LilGuy to production or on a publicly accessible server to use `lilguy serve --no-reload`. This will result in performance improvements as the app will not try to reload the file changes every time an update is made to the SQLite database.

## Contributing
LilGuy uses the Contributor Covenant code of conduct. Please read [CONTRIBUTING.md](CONTRIBUTING.md) for details.

## License
The MIT License (MIT)

Copyright (c) 2024, 2025 Dylan Hardison

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
