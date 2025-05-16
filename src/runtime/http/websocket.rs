use axum::extract::ws::{Message, Utf8Bytes, WebSocket};
use futures_util::{
    stream::{SplitSink, SplitStream},
    SinkExt, StreamExt,
};
use mlua::prelude::*;
use tokio::sync::Mutex;

pub struct LuaMessage(Message);

pub struct LuaWebSocket {
    sender: Mutex<SplitSink<WebSocket, Message>>,
    receiver: Mutex<SplitStream<WebSocket>>,
}

impl LuaWebSocket {
    pub fn new(ws: WebSocket) -> Self {
        let (sender, receiver) = ws.split();

        LuaWebSocket {
            sender: Mutex::new(sender),
            receiver: Mutex::new(receiver),
        }
    }

    async fn send(&self, msg: LuaMessage) -> Result<(), LuaError> {
        let mut sender = self.sender.lock().await;
        sender.send(msg.into()).await.into_lua_err()
    }

    async fn recv(&self) -> Result<Option<LuaMessage>, LuaError> {
        let mut receiver = self.receiver.lock().await;
        let resp = receiver.next().await.transpose().into_lua_err()?;
        Ok(resp.map(LuaMessage))
    }
}

impl From<LuaMessage> for Message {
    fn from(val: LuaMessage) -> Self {
        val.0
    }
}

impl From<Message> for LuaMessage {
    fn from(val: Message) -> Self {
        LuaMessage(val)
    }
}

impl LuaUserData for LuaWebSocket {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method("send", |lua, this, msg: LuaValue| async move {
            let msg = LuaMessage::from_lua(msg, &lua)?;
            this.send(msg).await
        });
        methods.add_async_method("recv", |_lua, this, ()| async move { this.recv().await });
    }

    /// ws.binary is a shortcut for { type = "binary", data = ... }
    /// same for ping and pong.
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        add_lua_message_field("binary", fields);
        add_lua_message_field("ping", fields);
        add_lua_message_field("pong", fields);
    }
}

fn add_lua_message_field<F>(name: &'static str, fields: &mut F)
where
    F: LuaUserDataFields<LuaWebSocket>,
{
    fields.add_field_function_get(name, move |lua, _| {
        lua.create_function(move |lua, data: LuaString| {
            let table = lua.create_table()?;
            table.set("type", name)?;
            table.set("data", data)?;
            Ok(table)
        })
    });
}

fn lua_message(lua: &Lua, ws_type: &str, ws_data: &[u8]) -> LuaResult<LuaValue> {
    let table = lua.create_table()?;
    table.set("type", ws_type)?;
    table.set("data", lua.create_string(ws_data)?)?;
    Ok(LuaValue::Table(table))
}

impl IntoLua for LuaMessage {
    fn into_lua(self, lua: &Lua) -> LuaResult<LuaValue> {
        let LuaMessage(msg) = self;

        let value = match msg {
            Message::Text(utf8_bytes) => {
                LuaValue::String(lua.create_string(utf8_bytes.as_bytes())?)
            }
            Message::Binary(bytes) => lua_message(lua, "binary", &bytes)?,
            Message::Ping(bytes) => lua_message(lua, "ping", &bytes)?,
            Message::Pong(bytes) => lua_message(lua, "pong", &bytes)?,
            Message::Close(_) => return Ok(LuaValue::Nil),
        };

        Ok(value)
    }
}

impl FromLua for LuaMessage {
    fn from_lua(value: LuaValue, _lua: &Lua) -> LuaResult<Self> {
        match value {
            LuaValue::String(s) => {
                let msg = Message::Text(Utf8Bytes::from(&*s.to_str()?));
                Ok(msg.into())
            }
            LuaValue::Table(table) => {
                let msg_type: String = table.get("type")?;
                let data: String = table.get("data")?;

                match msg_type.as_str() {
                    "binary" => Ok(LuaMessage(Message::Binary(data.into()))),
                    "ping" => Ok(LuaMessage(Message::Ping(data.into()))),
                    "pong" => Ok(LuaMessage(Message::Pong(data.into()))),
                    _ => Err(LuaError::RuntimeError("Invalid message type".into())),
                }
            }
            _ => Err(LuaError::RuntimeError("Expected a table".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::ws::Message;

    #[test]
    fn test_lua_message_conversion() {
        let lua = Lua::new();
        let message = Message::Text("Hello, World!".into());
        let lua_message: LuaMessage = message.into();

        let lua_value = lua_message.into_lua(&lua).unwrap();
        assert!(lua_value.is_table());

        let converted_message: LuaMessage = LuaMessage::from_lua(lua_value, &lua).unwrap();
        assert_eq!(converted_message.0, Message::Text("Hello, World!".into()));

        let code = r#"
            msg = { type = "binary", data = "stuff" }
        "#;
        lua.load(code).exec().unwrap();
        let msg = lua.globals().get::<LuaMessage>("msg").unwrap();
        assert_eq!(msg.0, Message::Binary("stuff".into()))
    }
}
