use mlua::prelude::*;

pub fn register(lua: &Lua) -> LuaResult<()> {
    let globals = lua.globals();

    let task = lua.create_table()?;
    task.set("spawn", lua.create_async_function(task_spawn)?)?;

    globals.set("task", task)?;

    Ok(())
}

async fn task_spawn(_lua: Lua, f: LuaFunction) -> LuaResult<()> {
    tokio::spawn(async move {
        if let Err(e) = f.call_async::<()>(()).await {
            tracing::error!("error in task::spawn() function: {e}");
        }
    });

    Ok(())
}
