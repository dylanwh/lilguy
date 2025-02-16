use std::{net::SocketAddr, sync::Arc};

use mlua::prelude::*;
use parking_lot::Mutex;
use tokio::{
    io::BufReader,
    net::{TcpListener, TcpStream},
};
use tokio_util::sync::CancellationToken;

use crate::io_methods;

pub fn register(lua: &Lua) -> LuaResult<()> {
    let globals = lua.globals();
    let net = lua.create_table()?;
    net.set("listen", lua.create_async_function(net_listen)?)?;
    net.set("connect", lua.create_async_function(net_connect)?)?;

    globals.set("net", net)?;
    Ok(())
}

#[derive(Debug)]
pub struct LuaTcpListener {
    listener: Mutex<Option<Arc<TcpListener>>>,
    shutdown: CancellationToken,
}

impl LuaTcpListener {
    pub fn close(&self) {
        self.listener.lock().take();
        self.shutdown.cancel();
    }
}

impl LuaUserData for LuaTcpListener {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("close", move |_, this, _: ()| {
            this.close();
            Ok(())
        });

        methods.add_async_method("accept", |lua, this, _: ()| async move {
            let mut ret = LuaMultiValue::new();
            let Some(listener) = this.listener.lock().clone() else {
                return Ok(ret);
            };

            if let Some((stream, addr)) = accept_or_cancelled(&listener, &this.shutdown).await? {
                ret.push_back(LuaValue::UserData(
                    lua.create_userdata(LuaTcpStream::new( stream ))?,
                ));
                ret.push_back(LuaValue::String(lua.create_string(addr.to_string())?));
            }

            Ok(ret)
        });
    }
}

async fn accept_or_cancelled(
    listener: &TcpListener,
    shutdown: &CancellationToken,
) -> LuaResult<Option<(TcpStream, SocketAddr)>> {
    let res = tokio::select! {
        result = listener.accept() => {
            result.map(Some)
        },
        _ = shutdown.cancelled() => {
            Ok(None)
        }
    };
    res.map_err(LuaError::external)
}

#[derive(Debug)]
pub struct LuaTcpStream {
    stream: BufReader<TcpStream>,
}

impl LuaTcpStream {
    pub fn new(stream: TcpStream) -> Self {
        Self {
            stream: BufReader::new(stream),
        }
    }
}

impl LuaUserData for LuaTcpStream {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        io_methods!(methods, stream);
    }
}

async fn net_listen(_lua: Lua, addr: String) -> LuaResult<LuaTcpListener> {
    let listener = TcpListener::bind(addr).await.map_err(LuaError::external)?;
    let listener = Mutex::new(Some(Arc::new(listener)));
    let shutdown = CancellationToken::new();
    Ok(LuaTcpListener { listener, shutdown })
}

async fn net_connect(_lua: Lua, addr: String) -> LuaResult<LuaTcpStream> {
    let stream = TcpStream::connect(addr).await.map_err(LuaError::external)?;
    let stream = BufReader::new(stream);
    Ok(LuaTcpStream { stream })
}
