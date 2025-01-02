pub mod http;

pub use mlua::prelude::*;
use mlua::IntoLua;
use parking_lot::Mutex;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

use crate::{
    database::{global::Global, Database},
    routes::Routes,
    template::Template,
    watch::{watch, MatchExtension, MatchParent},
};

const LUA_PRELUDE: &str = include_str!("prelude.lua");

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("lua error: {0}")]
    Lua(#[from] LuaError),

    #[error("lua not initialized")]
    LuaNotStarted,

    #[error("services not started")]
    ServicesNotStarted,

    #[error("database error: {0}")]
    Database(#[from] crate::database::Error),

    #[error("reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
}

#[derive(Debug, Clone)]
pub struct Runtime {
    directory: PathBuf,
    lua: Arc<Mutex<Option<Lua>>>,
    services: Arc<Mutex<Option<Services>>>,
    watch_token: Arc<Mutex<Option<CancellationToken>>>,
}

#[derive(Debug, Clone)]
struct Services {
    database: Database,
    template: Template,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub reload: bool,
}

impl Runtime {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            lua: Arc::new(Mutex::new(None)),
            services: Arc::new(Mutex::new(None)),
            watch_token: Arc::new(Mutex::new(None)),
        }
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.directory.join("assets")
    }

    /// load the main lua file and set up the environment
    #[allow(dependency_on_unit_never_type_fallback)]
    pub async fn run(&self, name: String, args: Vec<String>) -> Result<(), Error> {
        let lua = self.lua()?;
        let globals = lua.globals();
        let commands = globals.get::<LuaTable>("commands")?;
        let func: LuaFunction = commands.get(name)?;
        let args = args
            .into_iter()
            .map(|arg| arg.into_lua(&lua))
            .collect::<Result<Vec<LuaValue>, _>>()?;
        let args = LuaMultiValue::from(args);
        func.call_async(args).await?;
        Ok(())
    }

    pub fn lua(&self) -> Result<Lua, Error> {
        let lua = self
            .lua
            .lock()
            .clone()
            .ok_or_else(|| Error::LuaNotStarted)?;

        Ok(lua)
    }

    fn set_lua(&self, lua: Lua) {
        self.lua.lock().replace(lua);
    }

    pub fn start_services(&self) -> Result<(), Error> {
        let mut services = self.services.lock();
        if services.is_none() {
            let database = Database::open(self.directory.join("app.db"))?;
            let template = Template::new(self.directory.join("templates"));
            services.replace(Services { database, template });
        }
        Ok(())
    }

    fn services(&self) -> Result<Services, Error> {
        self.services
            .lock()
            .clone()
            .ok_or_else(|| Error::ServicesNotStarted)
    }

    pub async fn start_watcher(&self) -> Result<(), Error> {
        if self.watch_token.lock().is_some() {
            return Ok(());
        }
        tracing::info!("starting watcher");

        let runtime = self.clone();
        let template = runtime.services()?.template.clone();

        let token = CancellationToken::new();
        let mut rx = watch(
            self.directory.clone(),
            vec![
                ("runtime", MatchExtension("lua".to_string()).into()),
                (
                    "templates",
                    MatchParent(self.directory.join("templates")).into(),
                ),
            ],
            token.clone(),
        )
        .await;
        self.watch_token.lock().replace(token);

        tokio::spawn(async move {
            while let Some((name, _changes)) = rx.recv().await {
                tracing::info!("reload {name}");
                match name {
                    "runtime" => {
                        tracing::info!("restarting runtime");
                        runtime.restart_lua().await.unwrap();
                    }
                    "templates" => {
                        tracing::info!("reloading templates");
                        template
                            .call(|env| {
                                env.clear_templates();
                                Ok(())
                            })
                            .await
                            .unwrap();
                    }
                    _ => {}
                }
            }
        });

        tracing::info!("watcher started");

        Ok(())
    }

    pub async fn start_lua(&self) -> Result<(), Error> {
        let lua = self.new_lua().await?;
        self.set_lua(lua);
        Ok(())
    }

    pub async fn restart_lua(&self) -> Result<(), Error> {
        let lua = self.new_lua().await?;
        self.set_lua(lua);
        Ok(())
    }

    pub async fn start(&self, options: Options) -> Result<(), Error> {
        self.start_services()?;
        if options.reload {
            self.start_watcher().await?;
        }
        self.start_lua().await?;
        Ok(())
    }

    #[allow(dependency_on_unit_never_type_fallback)]
    async fn new_lua(&self) -> Result<Lua, Error> {
        let services = self.services()?;
        let lua = Lua::new();
        lua.load_std_libs(
            LuaStdLib::TABLE | LuaStdLib::STRING | LuaStdLib::UTF8 | LuaStdLib::MATH,
        )?;

        let globals = lua.globals();
        let package = globals.get::<LuaTable>("package")?;
        #[cfg(windows)]
        package.set("path", format!("?.lua"))?;
        #[cfg(not(windows))]
        package.set("path", self.directory.join("?.lua").to_string_lossy())?;

        globals.set("sleep", lua.create_async_function(builtin_sleep)?)?;
        globals.set("timeout", lua.create_async_function(builtin_timeout)?)?;

        let json = lua.create_table()?;
        json.set("encode", lua.create_function(json_encode)?)?;
        json.set("decode", lua.create_function(json_decode)?)?;
        globals.set("json", json)?;

        globals.set("global", Global::new(&services.database))?;

        globals.set("routes", Routes::new())?;
        globals.set("database", services.database.clone())?;
        globals.set("template", services.template.clone())?;
        globals.set("null", lua.null())?;
        globals.set("array_mt", lua.array_metatable())?;

        lua.set_warning_function(|_, msg, _| {
            tracing::warn!("{msg}");
            Ok(())
        });

        // lua already has warn, but let's add debug() and info() as well
        // should work like print(foo, bar) == print(foo .. bar)
        globals.set(
            "debug",
            lua.create_function(|_, args: LuaMultiValue| {
                let mut buffer = String::new();
                for arg in args {
                    let arg = arg.to_string()?;
                    buffer.push_str(&arg);
                }
                tracing::debug!("{buffer}");
                Ok(())
            })?,
        )?;

        globals.set(
            "info",
            lua.create_function(|_, args: LuaMultiValue| {
                let mut buffer = String::new();
                for arg in args {
                    let arg = arg.to_string()?;
                    buffer.push_str(&arg);
                }
                tracing::info!("{buffer}");
                Ok(())
            })?,
        )?;

        http::register(&lua)?;

        lua.load(LUA_PRELUDE).exec()?;

        let require = globals.get::<LuaFunction>("require")?;
        require.call_async("app").await?;
        Ok(lua)
    }

    pub fn database(&self) -> Result<Database, Error> {
        Ok(self.services()?.database)
    }
}

fn json_encode(_: &Lua, value: LuaValue) -> LuaResult<String> {
    serde_json::to_string(&value).map_err(LuaError::external)
}

fn json_decode(lua: &Lua, value: String) -> LuaResult<LuaValue> {
    let value: serde_json::Value = serde_json::from_str(&value).map_err(LuaError::external)?;
    lua.to_value(&value)
}

async fn builtin_sleep(_lua: Lua, seconds: f64) -> LuaResult<()> {
    tokio::time::sleep(Duration::from_secs_f64(seconds)).await;
    Ok(())
}

/// timeout(seconds, function)
async fn builtin_timeout(_lua: Lua, (seconds, func): (f64, LuaFunction)) -> LuaResult<()> {
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
