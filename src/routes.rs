use mlua::prelude::*;
use path_tree::PathTree;
use std::ops::{Deref, DerefMut};

#[derive(Debug, Default)]
pub struct Routes(PathTree<LuaFunction>);

impl Routes {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Deref for Routes {
    type Target = PathTree<LuaFunction>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Routes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// routes variable
/// routes["/"] = function(request, path) return path end
/// routes["/foo"](request) -> "/"
impl LuaUserData for Routes {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(LuaMetaMethod::Index, |lua, this, key: String| {
            let route = this.find(key.as_str());
            match route {
                Some((func, path)) => {
                    let pattern = lua.create_string(path.pattern())?;
                    let params = lua.create_table_from(path.params_iter())?;
                    let route = lua.create_table()?;
                    route.set("func", func)?;
                    route.set("params", params)?;
                    route.set("pattern", pattern)?;

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
