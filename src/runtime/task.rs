use std::time::Duration;

use mlua::prelude::*;
use tokio::task::JoinSet;

pub fn register(lua: &Lua) -> LuaResult<()> {
    let globals = lua.globals();
    let task = lua.create_table()?;
    task.set("race", lua.create_async_function(task_race)?)?;
    task.set("spawn", lua.create_async_function(task_spawn)?)?;
    task.set("sleep", lua.create_async_function(task_sleep)?)?;
    task.set("timeout", lua.create_async_function(task_timeout)?)?;
    task.set("yield", lua.create_async_function(task_yield)?)?;

    globals.set("task", task)?;
    Ok(())
}

async fn task_spawn(_lua: Lua, (func, args): (LuaFunction, LuaMultiValue)) -> LuaResult<()> {
    tokio::spawn(async move {
        if let Err(err) = func.call_async::<()>(args).await {
            tracing::error!(?err, "error in spawned task");
        }
    });
    Ok(())
}

async fn task_race(lua: Lua, tasks: LuaMultiValue) -> LuaResult<LuaMultiValue> {
    let mut join_set: JoinSet<LuaResult<LuaMultiValue>> = JoinSet::new();

    if tasks.is_empty() {
        return Ok(LuaMultiValue::new());
    }

    for task in tasks {
        let task = LuaFunction::from_lua(task, &lua)?;
        join_set.spawn(async move { task.call_async(()).await });
    }

    join_set
        .join_next()
        .await
        .expect("no tasks to join")
        .map_err(LuaError::external)?
}

async fn task_sleep(_lua: Lua, seconds: f64) -> LuaResult<()> {
    tokio::time::sleep(Duration::from_secs_f64(seconds)).await;
    Ok(())
}


/// timeout(seconds, function)
async fn task_timeout(_lua: Lua, (seconds, func): (f64, LuaFunction)) -> LuaResult<()> {
    let timeout = tokio::time::sleep(Duration::from_secs_f64(seconds));
    tokio::select! {
        _ = timeout => {
            Err(LuaError::RuntimeError("timeout".to_string()))
        }
        _ = func.call_async::<()>(()) => {
            Ok(())
        }
    }
}

async fn task_yield(_lua: Lua, _: ()) -> LuaResult<()> {
    tokio::task::yield_now().await;
    Ok(())
}
