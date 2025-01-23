pub mod dump;
pub mod http;
pub mod os;

use eyre::ContextCompat;
pub use mlua::prelude::*;
use mlua::IntoLua;
use parking_lot::Mutex;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    database::{global::Global, Database},
    routes::Routes,
    template::Template,
    watch::{watch, Match},
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

#[derive(Debug, Clone, Default)]
pub struct Runtime {
    lua: Arc<Mutex<Option<Lua>>>,
    services: Arc<Mutex<Option<Services>>>,
    started: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
struct Services {
    database: Database,
    template: Template,
}

impl Runtime {
    pub fn new() -> Self {
        Self::default()
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

    #[tracing::instrument(level = "debug", skip(self))]
    fn set_lua(&self, lua: Lua) {
        self.lua.lock().replace(lua);
    }

    #[tracing::instrument(level = "debug", skip(self, app))]
    pub fn start_services(&self, app: &Path) -> Result<(), Error> {
        let mut services = self.services.lock();

        if services.is_none() {
            let database = Database::open(app.with_extension("db"))?;
            let template = Template::new(app.with_file_name("templates"));
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

    #[tracing::instrument(level = "debug", skip(self, app))]
    async fn start_watcher(
        &self,
        app: &Path,
        token: &CancellationToken,
        tracker: &TaskTracker,
    ) -> Result<(), Error> {
        let runtime = self.clone();
        let template = runtime.services()?.template.clone();

        let mut rx = watch(
            token.clone(),
            tracker,
            app.parent().unwrap_or_else(|| Path::new(".")).to_path_buf(),
            vec![
                ("runtime", Match::Extension("lua".to_string())),
                ("templates", Match::Parent(app.join("templates"))),
            ],
        )
        .await;

        let app = app.to_path_buf();
        tracker.spawn(async move {
            while let Some((name, _changes)) = rx.recv().await {
                tracing::debug!("reload {name}");
                match name {
                    "runtime" => {
                        tracing::info!("restarting runtime");
                        if let Err(err) = runtime.restart_lua(&app).await {
                            tracing::error!(?err, "error restarting runtime");
                        }
                    }
                    "templates" => {
                        tracing::info!("reloading templates");
                        if let Err(err) = template
                            .call(|env| {
                                env.clear_templates();
                                Ok(())
                            })
                            .await
                        {
                            tracing::error!(?err, "error reloading templates");
                        }
                    }
                    _ => {}
                }
            }
        });

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self, app))]
    async fn start_lua(
        &self,
        app: &Path,
        token: &CancellationToken,
        tracker: &TaskTracker,
    ) -> Result<(), Error> {
        let lua = self.new_lua(app).await?;
        self.set_lua(lua);

        let runtime = self.clone();
        let token = token.clone();
        tracker.spawn(async move {
            token.cancelled().await;
            if let Err(err) = runtime.shutdown().await {
                tracing::error!(?err, "error calling on_shutdown");
            }
        });
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn shutdown(&self) -> Result<(), Error> {
        let lua = self.lua()?;
        let globals = lua.globals();
        if let Some(on_shutdown) = globals.get::<Option<LuaFunction>>("on_shutdown")? {
            on_shutdown.call_async::<()>(()).await?;
        }

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn restart_lua(&self, app: &Path) -> Result<(), Error> {
        let lua = self.new_lua(app).await?;
        self.set_lua(lua);
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn start(
        &self,
        app: &Path,
        reload: bool,
        token: &CancellationToken,
        tracker: &TaskTracker,
    ) -> Result<(), Error> {
        if self.started.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.start_services(&app)?;
        if reload {
            self.start_watcher(&app, token, tracker).await?;
        }
        self.start_lua(app, token, tracker).await?;
        self.started.store(true, Ordering::Relaxed);
        Ok(())
    }

    #[allow(dependency_on_unit_never_type_fallback)]
    #[tracing::instrument(level = "debug", skip(self, app))]
    async fn new_lua(&self, app: &Path) -> Result<Lua, Error> {
        let services = self.services()?;
        let lua = Lua::new_with(
            LuaStdLib::TABLE
                | LuaStdLib::STRING
                | LuaStdLib::UTF8
                | LuaStdLib::MATH
                | LuaStdLib::COROUTINE
                | LuaStdLib::PACKAGE,
            LuaOptions::default(),
        )?;

        let globals = lua.globals();
        let package = globals.get::<LuaTable>("package")?;
        if let Some(parent) = app.parent() {
            package.set("path", parent.join("?.lua").to_string_lossy())?;
        }

        globals.set("sleep", lua.create_async_function(builtin_sleep)?)?;
        globals.set("timeout", lua.create_async_function(builtin_timeout)?)?;
        globals.set("markdown", lua.create_function(builtin_markdown)?)?;

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

        lua.load(LUA_PRELUDE).exec()?;

        http::register(&lua)?;

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

fn builtin_markdown(_lua: &Lua, value: String) -> LuaResult<String> {
    Ok(comrak::markdown_to_html(
        &value,
        &comrak::ComrakOptions::default(),
    ))
}
