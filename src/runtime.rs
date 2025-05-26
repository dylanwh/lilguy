pub mod channel;
pub mod dump;
pub mod file;
pub mod http;
pub mod mdns;
pub mod os;
pub mod regex;

use eyre::{eyre, Result};
use http::not_found;
pub use mlua::prelude::*;
use mlua::IntoLua;
use parking_lot::Mutex;
use serde::Serialize;
use std::{
    collections::HashSet,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    database::{global::Global, Database},
    routes::Routes,
    template::Template,
    watch::{watch, Match},
};

const LUA_PRELUDE: &str = include_str!("prelude.lua");
const SQL_SCHEMA: &str = include_str!("schema.sql");

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
    pub async fn run(&self, name: String, args: Vec<String>) -> Result<()> {
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

    pub fn lua(&self) -> Result<Lua> {
        let lua = self
            .lua
            .lock()
            .clone()
            .ok_or_else(|| eyre!("Lua runtime not started"))?;

        Ok(lua)
    }

    #[tracing::instrument(level = "debug", skip(self))]
    fn set_lua(&self, lua: Lua) {
        self.lua.lock().replace(lua);
    }

    #[tracing::instrument(level = "debug", skip(self, app))]
    pub async fn start_services(&self, app: &Path) -> Result<()> {
        let db;
        {
            let mut services = self.services.lock();
            if services.is_none() {
                let database = Database::open(app.with_extension("db"))?;
                let template = Template::new(app.with_file_name("templates"));
                db = database.clone();
                services.replace(Services { database, template });
            } else {
                db = services.as_ref().expect("services").database.clone();
            }
        }

        db.call(|conn| {
            conn.execute_batch(SQL_SCHEMA)?;
            Ok(())
        })
        .await?;

        Ok(())
    }

    fn services(&self) -> Result<Services> {
        self.services
            .lock()
            .clone()
            .ok_or_else(|| eyre!("services not started"))
    }

    #[tracing::instrument(level = "debug", skip(self, directory))]
    async fn start_watcher(
        &self,
        directory: &Path,
        tracker: &TaskTracker,
        token: &CancellationToken,
    ) -> Result<(), eyre::Error> {
        let runtime = self.clone();
        let template = runtime.services()?.template.clone();

        let mut rx = watch(
            token.clone(),
            tracker,
            directory,
            vec![
                ("runtime", Match::Extension("lua".to_string())),
                ("templates", Match::StartsWith(directory.join("templates"))),
            ],
        )
        .await?;

        let app = directory.to_path_buf();
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
        tracker: &TaskTracker,
        token: &CancellationToken,
    ) -> Result<()> {
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
    pub async fn shutdown(&self) -> Result<()> {
        let lua = self.lua()?;
        let globals = lua.globals();
        if let Some(on_shutdown) = globals.get::<Option<LuaFunction>>("on_shutdown")? {
            on_shutdown.call_async::<()>(()).await?;
        }

        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self))]
    async fn restart_lua(&self, app: &Path) -> Result<()> {
        let lua = self.new_lua(app).await?;
        self.set_lua(lua);
        Ok(())
    }

    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn start(
        &self,
        tracker: &TaskTracker,
        token: &CancellationToken,
        app: &Path,
        reload: bool,
    ) -> Result<(), eyre::Report> {
        if self.started.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.start_services(app).await?;
        if reload {
            self.start_watcher(app, tracker, token).await?;
        }
        self.start_lua(app, tracker, token).await?;
        self.started.store(true, Ordering::Relaxed);
        Ok(())
    }

    #[allow(dependency_on_unit_never_type_fallback)]
    #[tracing::instrument(level = "debug", skip(self, app))]
    async fn new_lua(&self, app: &Path) -> Result<Lua> {
        let services = self.services()?;
        let lua = Lua::new_with(
            LuaStdLib::TABLE
                | LuaStdLib::STRING
                | LuaStdLib::MATH
                | LuaStdLib::PACKAGE
                | LuaStdLib::BIT,
            LuaOptions::default(),
        )?;

        let globals = lua.globals();
        let package = globals.get::<LuaTable>("package")?;
        if let Some(parent) = app.parent() {
            package.set("path", parent.join("?.lua").to_string_lossy())?;
        }

        globals.set("warn", lua.create_function(builtin_warn)?)?;
        globals.set("debug", lua.create_function(builtin_debug)?)?;
        globals.set("info", lua.create_function(builtin_info)?)?;

        globals.set("markdown", lua.create_function(builtin_markdown)?)?;

        let json = lua.create_table()?;
        json.set("encode", lua.create_function(json_encode)?)?;
        json.set("decode", lua.create_function(json_decode)?)?;
        json.set("null", lua.null())?;
        globals.set("json", json)?;

        globals.set("global", Global::new(&services.database))?;
        globals.set("routes", Routes::new(lua.create_function(not_found)?))?;
        globals.set("database", services.database.clone())?;
        globals.set("template", services.template.clone())?;
        globals.set("null", lua.null())?;
        globals.set("array_mt", lua.array_metatable())?;

        lua.load(LUA_PRELUDE).exec_async().await?;

        channel::register(&lua)?;
        file::register(&lua)?;
        http::register(&lua)?;
        os::register(&lua)?;
        regex::register(&lua)?;
        mdns::register(&lua)?;

        let db = &services.database;
        http::set_cookie_key(&lua, db).await?;

        let require = globals.get::<LuaFunction>("require")?;
        require.call_async("app").await?;
        Ok(lua)
    }
}

/// json.encode(value, options)
/// where options is an optional table with a single key `pretty`
/// if `pretty` is true, the output will be pretty printed (indented)
fn json_encode(lua: &Lua, (value, options): (LuaValue, Option<LuaTable>)) -> LuaResult<LuaString> {
    let mut buffer = Vec::new();
    let pretty = options
        .and_then(|options| options.get("pretty").ok())
        .unwrap_or(false);
    if pretty {
        let mut ser = serde_json::Serializer::pretty(&mut buffer);
        value.serialize(&mut ser).into_lua_err()?;
    } else {
        let mut ser = serde_json::Serializer::new(&mut buffer);
        value.serialize(&mut ser).into_lua_err()?;
    }

    lua.create_string(&buffer)
}

/// json.decode(value)
/// where value is a string containing json
/// returns a lua value
fn json_decode(lua: &Lua, value: String) -> LuaResult<LuaValue> {
    let value: serde_json::Value = serde_json::from_str(&value).into_lua_err()?;
    lua.to_value(&value)
}

fn builtin_markdown(_lua: &Lua, value: String) -> LuaResult<String> {
    Ok(comrak::markdown_to_html(
        &value,
        &comrak::ComrakOptions::default(),
    ))
}

fn builtin_warn(_lua: &Lua, args: LuaMultiValue) -> LuaResult<()> {
    let mut buffer = String::new();
    for arg in args {
        let arg = arg.to_string()?;
        buffer.push_str(&arg);
    }
    tracing::warn!("{buffer}");
    Ok(())
}

fn builtin_debug(_lua: &Lua, args: LuaMultiValue) -> LuaResult<()> {
    let mut buffer = String::new();
    for arg in args {
        let arg = arg.to_string()?;
        buffer.push_str(&arg);
    }
    tracing::debug!("{buffer}");
    Ok(())
}

fn builtin_info(_lua: &Lua, args: LuaMultiValue) -> LuaResult<()> {
    let mut buffer = String::new();
    for arg in args {
        let arg = arg.to_string()?;
        buffer.push_str(&arg);
    }
    tracing::info!("{buffer}");
    Ok(())
}

trait ToLuaArray {
    fn to_lua_array(self, lua: &Lua) -> LuaResult<LuaTable>;
}

impl<T, I> ToLuaArray for I
where
    I: IntoIterator<Item = T>,
    T: IntoLua,
{
    fn to_lua_array(self, lua: &Lua) -> LuaResult<LuaTable> {
        lua.create_table_from(self.into_iter().enumerate().map(|(i, item)| (i + 1, item)))
    }
}
