pub use mlua::prelude::*;
use mlua::IntoLua;
use parking_lot::Mutex;
use std::{
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};

use crate::{
    database::{global::Global, Database},
    routes::Routes,
    template::{self, Template},
    watch::{self, watch, MatchExtension, MatchParent, Matcher},
};

const LUA_PRELUDE: &str = include_str!("prelude.lua");

pub struct App {
    database: Database,
    routes: Routes,
}

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
}

#[derive(Debug, Clone)]
pub struct Runtime {
    directory: PathBuf,

    lua: Arc<Mutex<Option<Lua>>>,
    services: Arc<Mutex<Option<Services>>>,

    watching: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
struct Services {
    database: Database,
    template: Template,
}

impl Runtime {
    pub fn new(directory: PathBuf) -> Self {
        let lua = Arc::new(Mutex::new(None));
        let services = Arc::new(Mutex::new(None));

        // let watcher = watch(
        //     directory.clone(),
        //     vec![
        //         ("lua", Box::new(MatchExtension("lua".to_string()))),
        //         (
        //             "templates",
        //             Box::new(MatchParent(directory.join("templates"))),
        //         ),
        //     ],
        // ).await;
        // let x = watcher.recv();

        Self {
            directory,
            lua,
            services,
            watching: Arc::new(AtomicBool::new(false)),
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

    pub fn set_lua(&self, lua: Lua) {
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

    pub fn services(&self) -> Result<Services, Error> {
        self.services
            .lock()
            .clone()
            .ok_or_else(|| Error::ServicesNotStarted)
    }

    pub async fn start_watcher(&self) {
        if self.watching.load(std::sync::atomic::Ordering::Relaxed) {
            tracing::info!("watcher already started");
            return;
        }
        tracing::info!("starting watcher");

        let (mut rx, guard) = watch(
            self.directory.clone(),
            vec![
                ("runtime", MatchExtension("lua".to_string()).into()),
                (
                    "templates",
                    MatchParent(self.directory.join("templates")).into(),
                ),
            ],
        )
        .await;

        let runtime = self.clone();
        let template = runtime.services().unwrap().template.clone();

        tokio::spawn(async move {
            while let Some((name, changes)) = rx.recv().await {
                tracing::info!("reload {name}");
                match name {
                    "runtime" => {
                        tracing::info!("restarting runtime");
                        runtime.restart().await.unwrap();
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

            drop(guard);
        });


        tracing::info!("watcher started");
        self.watching
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub async fn start(&self) -> Result<(), Error> {
        self.start_services()?;
        self.start_watcher().await;

        let lua = self.new_lua().await?;
        self.set_lua(lua);
        Ok(())
    }

    pub async fn restart(&self) -> Result<(), Error> {
        let lua = self.new_lua().await?;
        self.set_lua(lua);
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
        lua.load(LUA_PRELUDE).exec()?;
        globals.set("sleep", lua.create_async_function(builtin_sleep)?)?;

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

        let require = globals.get::<LuaFunction>("require")?;
        require.call_async("app").await?;
        Ok(lua)
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
