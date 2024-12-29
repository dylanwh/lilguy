pub use mlua::prelude::*;
use mlua::IntoLua;
use notify::RecursiveMode;
use parking_lot::RwLock;
use path_tree::PathTree;
use std::{
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use typed_builder::TypedBuilder;

use crate::{
    database::{global::Global, Database},
    reload::Reload,
    template::Template,
};

const LUA_PRELUDE: &str = include_str!("prelude.lua");

#[derive(Debug, thiserror::Error)]
pub enum RuntimeInitError {
    #[error("lua error: {0}")]
    Lua(#[from] LuaError),

    #[error("template error: {0}")]
    Template(#[from] minijinja::Error),

    #[error("database error: {0}")]
    Database(#[from] tokio_rusqlite::Error),
}

#[derive(Debug, Clone, TypedBuilder)]
#[builder(field_defaults(setter(into)))]
pub struct Runtime {
    root: PathBuf,

    #[builder(default)]
    lua: Arc<RwLock<Lua>>,

    pub database: Database,
    pub template: Template,
}

impl LuaUserData for Template {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        // render(name, context)
        methods.add_method(
            "render",
            |lua, this, (name, context): (String, LuaValue)| {
                let rendered = this
                    .render(name.as_str(), context)
                    .map_err(LuaError::external)?;
                lua.create_string(&rendered)
            },
        );
    }
}

impl Reload for Runtime {
    fn name(&self) -> &'static str {
        "runtime"
    }

    fn reload(&self, files: Vec<PathBuf>) {
        println!("reloading files: {:?}", files);
        let lua = Lua::new();
        if let Err(err) = init(
            &lua,
            &self.root,
            self.database.clone(),
            self.template.clone(),
        ) {
            eprintln!("error reloading lua runtime: {err}");
            tracing::error!(?err, "error reloading lua runtime");
            return;
        }
        self.set_lua(lua);
    }

    fn files(&self) -> Vec<(PathBuf, notify::RecursiveMode)> {
        let lua = &self.lua();
        let mut files = vec![];

        tokio::task::block_in_place(|| {
            lua.globals()
                .get::<LuaTable>("WATCH_FILES")
                .expect("WATCH_FILES not set")
                .for_each(|_: String, path: String| {
                    files.push((PathBuf::from(path), RecursiveMode::NonRecursive));
                    Ok(())
                })
                .expect("error iterating over watch files");
        });

        files
    }
}

impl Runtime {
    /// load the main lua file and set up the environment
    #[allow(dependency_on_unit_never_type_fallback)]
    pub async fn run(&self, name: String, args: Vec<String>) -> Result<(), LuaError> {
        let lua = self.lua();
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

    pub fn lua(&self) -> Lua {
        self.lua.read().clone()
    }

    pub fn set_lua(&self, lua: Lua) {
        *self.lua.write() = lua;
    }

    pub fn init(&self) -> Result<(), RuntimeInitError> {
        init(
            self.lua.read(),
            &self.root,
            self.database.clone(),
            self.template.clone(),
        )?;
        Ok(())
    }
}

#[allow(dependency_on_unit_never_type_fallback)]
pub fn init<L>(
    lua: L,
    root: &Path,
    database: Database,
    template: Template,
) -> Result<(), RuntimeInitError>
where
    L: Deref<Target = Lua>,
{
    let lua = &*lua;
    let package_path = root.join("?.lua");

    lua.load_std_libs(LuaStdLib::TABLE | LuaStdLib::STRING | LuaStdLib::UTF8 | LuaStdLib::MATH)?;
    let globals = lua.globals();
    let package = globals.get::<LuaTable>("package")?;
    package.set("path", package_path.to_string_lossy())?;

    lua.load(LUA_PRELUDE).exec()?;

    globals.set("sleep", lua.create_async_function(builtin_sleep)?)?;

    let json = lua.create_table()?;
    json.set(
        "encode",
        lua.create_function(|_, value: LuaValue| {
            serde_json::to_string(&value).map_err(LuaError::external)
        })?,
    )?;
    json.set(
        "decode",
        lua.create_function(|lua, value: String| {
            let value: serde_json::Value =
                serde_json::from_str(&value).map_err(LuaError::external)?;
            lua.to_value(&value)
        })?,
    )?;
    globals.set("json", json)?;

    globals.set(
        "global",
        Global::builder().conn(database.as_ref().clone()).build(),
    )?;

    globals.set("routes", Router(PathTree::new()))?;
    globals.set("template", template)?;
    let route_mt = lua.create_table()?;
    route_mt.set(
        "__call",
        lua.create_async_function(|_, route: LuaTable| async move {
            let func = route.get::<LuaFunction>("func")?;
            let args = route.get::<LuaTable>("params")?;
            func.call_async::<LuaMultiValue>(args).await
        })?,
    )?;
    lua.set_named_registry_value("route_mt", route_mt)?;
    globals.set("null", lua.null())?;
    globals.set("array_mt", lua.array_metatable())?;

    let require = globals.get::<LuaFunction>("require")?;
    require.call("app")?;

    Ok(())
}

async fn builtin_sleep(_lua: Lua, seconds: f64) -> LuaResult<()> {
    tokio::time::sleep(Duration::from_secs_f64(seconds)).await;
    Ok(())
}

pub struct Router(PathTree<LuaFunction>);

impl Deref for Router {
    type Target = PathTree<LuaFunction>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Router {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// routes variable
/// routes["/"] = function(request, path) return path end
/// routes["/foo"](request) -> "/"
impl LuaUserData for Router {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(LuaMetaMethod::Index, |lua, this, key: String| {
            let route = this.find(key.as_str());
            match route {
                Some((func, path)) => {
                    let pattern = lua.create_string(path.pattern())?;
                    let params = lua.create_table_from(path.params_iter())?;
                    let route = lua.create_table()?;
                    route.set("pattern", pattern)?;
                    route.set("params", params)?;
                    route.set("func", func)?;
                    let route_mt = lua.named_registry_value::<LuaTable>("route_mt")?;
                    route.set_metatable(Some(route_mt));

                    Ok(LuaValue::Table(route))
                }
                None => Ok(LuaValue::Nil),
            }
        });

        methods.add_meta_method_mut(
            LuaMetaMethod::NewIndex,
            |_, this, (key, function): (String, LuaFunction)| {
                let size = this.insert(&key, function);
                Ok(size)
            },
        );
    }
}
