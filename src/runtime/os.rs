// async version of standard lua os library
use mlua::prelude::*;

pub fn register(lua: &Lua) -> LuaResult<()> {
    let os = lua.create_table()?;
    os.set("execute", lua.create_async_function(os_execute)?)?;
    os.set("getenv", lua.create_function(os_getenv)?)?;

    #[cfg(target_os = "windows")]
    os.set("name", "windows")?;

    #[cfg(target_os = "linux")]
    os.set("name", "linux")?;

    #[cfg(target_os = "macos")]
    os.set("name", "macos")?;

    #[cfg(target_os = "freebsd")]
    os.set("name", "freebsd")?;

    // unknown
    #[cfg(not(any(
        target_os = "windows",
        target_os = "linux",
        target_os = "macos",
        target_os = "freebsd"
    )))]
    os.set("name", "unknown")?;

    lua.globals().set("os", os)?;
    Ok(())
}

fn os_getenv(_lua: &Lua, key: String) -> LuaResult<Option<String>> {
    Ok(std::env::var(key).ok())
}

#[cfg(target_os = "windows")]
async fn os_execute(_lua: Lua, command: String) -> LuaResult<(Option<bool>, String, i32)> {
    let output = tokio::process::Command::new("powershell")
        .arg("-Command")
        .arg(&command)
        .output()
        .await
        .into_lua_err()?;

    let status = output.status;
    let exit = status.code();
    let success = if status.success() { Some(true) } else { None };
    Ok((success, "exit".to_string(), exit.unwrap_or(0)))
}

#[cfg(not(target_os = "windows"))]
async fn os_execute(_lua: Lua, command: String) -> LuaResult<(Option<bool>, String, i32)> {
    use std::os::unix::process::ExitStatusExt;

    let output = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&command)
        .output()
        .await
        .into_lua_err()?;

    let status = output.status;
    let signal = status.signal();
    let exit = status.code();
    let success = if status.success() { Some(true) } else { None };
    match (exit, signal) {
        (Some(exit), _) => Ok((success, "exit".to_string(), exit)),
        (_, Some(signal)) => Ok((success, "signal".to_string(), signal)),
        _ => Ok((success, "exit".to_string(), 0)),
    }
}
