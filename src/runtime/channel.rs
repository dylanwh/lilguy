use mlua::prelude::*;
use tokio::sync::broadcast;

pub struct LuaBroadcastSender {
    tx: broadcast::Sender<LuaValue>,
}

pub struct LuaBroadcastReceiver {
    rx: broadcast::Receiver<LuaValue>,
}

pub fn register(lua: &Lua) -> LuaResult<()> {
    let globals = lua.globals();
    let channel = lua.create_table()?;
    channel.set("broadcast", lua.create_function(channel_broadast)?)?;
    globals.set("channel", channel)?;
    Ok(())
}

fn channel_broadast(
    lua: &Lua,
    capacity: usize,
) -> LuaResult<(LuaAnyUserData, LuaAnyUserData)> {
    let (tx, rx) = broadcast::channel(capacity);
    let tx = lua.create_userdata(LuaBroadcastSender { tx })?;
    let rx = lua.create_userdata(LuaBroadcastReceiver { rx })?;


    Ok((tx, rx))
}

impl LuaUserData for LuaBroadcastSender {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("send", |_, this, value: LuaValue| {
            this.tx.send(value).map_err(LuaError::external)?;
            Ok(())
        });
        methods.add_method("subscribe", |lua, this, _: ()| {
            let rx = this.tx.subscribe();
            lua.create_userdata(LuaBroadcastReceiver { rx })
        });
    }
}

impl LuaUserData for LuaBroadcastReceiver {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method_mut("recv", |_, mut this, _: ()| async move {
            this.rx.recv().await.map_err(LuaError::external)
        });
    }
}
