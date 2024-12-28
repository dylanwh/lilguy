pub use mlua::prelude::*;
use mlua::IntoLua;
use path_tree::PathTree;
use std::{
    ops::{Deref, DerefMut},
    path::PathBuf,
    time::Duration,
};
use typed_builder::TypedBuilder;

use crate::{
    database::{global::Global, Database},
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
    name: String,
    root: PathBuf,

    #[builder(default)]
    lua: Lua,

    pub database: Database,
    pub template: Template,
}

impl AsRef<Lua> for Runtime {
    fn as_ref(&self) -> &Lua {
        &self.lua
    }
}

impl Runtime {
    /// load the main lua file and set up the environment
    #[allow(dependency_on_unit_never_type_fallback)]
    pub fn init(&self) -> Result<(), RuntimeInitError> {
        let lua = &self.lua;
        let package_path = self.root.join("?.lua");

        lua.load_std_libs(
            LuaStdLib::TABLE | LuaStdLib::STRING | LuaStdLib::UTF8 | LuaStdLib::MATH,
        )?;
        let globals = lua.globals();
        let package = globals.get::<LuaTable>("package")?;
        package.set("path", package_path.to_string_lossy())?;

        // set NAME global
        globals.set("NAME", self.name.as_str())?;
        globals.set("ROOT", self.root.to_string_lossy())?;

        // load prelude
        lua.load(LUA_PRELUDE).exec()?;

        globals.set("sleep", lua.create_async_function(builtin_sleep)?)?;

        let json = self.lua.create_table()?;
        json.set(
            "encode",
            lua.create_function(|_, value: LuaValue| {
                serde_json::to_string(&value).map_err(LuaError::external)
            })?,
        )?;
        json.set(
            "decode",
            self.lua.create_function(|lua, value: String| {
                let value: serde_json::Value =
                    serde_json::from_str(&value).map_err(LuaError::external)?;
                lua.to_value(&value)
            })?,
        )?;
        globals.set("json", json)?;

        globals.set(
            "global",
            Global::builder()
                .conn(self.database.as_ref().clone())
                .build(),
        )?;

        globals.set("routes", Router(PathTree::new()))?;
        let route_mt = self.lua.create_table()?;
        route_mt.set(
            "__call",
            self.lua
                .create_async_function(|_, route: LuaTable| async move {
                    let func = route.get::<LuaFunction>("func")?;
                    let args = route.get::<LuaTable>("params")?;
                    func.call_async::<LuaValue>(args).await
                })?,
        )?;
        lua.set_named_registry_value("route_mt", route_mt)?;
        globals.set("null", self.lua.null())?;
        globals.set("array_mt", self.lua.array_metatable())?;

        let require = globals.get::<LuaFunction>("require")?;
        require.call(self.name.as_str())?;

        Ok(())
    }

    #[allow(dependency_on_unit_never_type_fallback)]
    pub async fn run(&self, name: String, args: Vec<String>) -> Result<(), LuaError> {
        let lua: &Lua = self.as_ref();
        let globals = lua.globals();
        let commands = globals.get::<LuaTable>("commands")?;
        let func: LuaFunction = commands.get(name)?;
        let args = args
            .into_iter()
            .map(|arg| arg.into_lua(lua))
            .collect::<Result<Vec<LuaValue>, _>>()?;
        let args = LuaMultiValue::from(args);
        func.call_async(args).await?;
        Ok(())
    }
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
