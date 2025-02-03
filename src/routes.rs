use mlua::prelude::*;
use path_tree::PathTree;

#[derive(Debug)]
pub struct Routes {
    tree: PathTree<LuaFunction>,
    not_found: LuaFunction,
    pub ws: Option<LuaFunction>,
}

type Route<'a, 'b> = Option<(LuaFunction, path_tree::Path<'a, 'b>)>;

impl Routes {
    pub fn new(not_found: LuaFunction) -> Self {
        Self {
            tree: PathTree::new(),
            ws: None,
            not_found,
        }
    }

    pub fn find<'a, 'b>(&'a self, path: &'b str) -> (LuaFunction, Option<path_tree::Path<'a, 'b>>) {
        match self.tree.find(path) {
            Some((handler, route)) => (handler.clone(), Some(route)),
            None => (self.not_found.clone(), None),
        }
    }
}

/// routes variable
/// routes["/"] = function(request, path) return path end
/// routes["/foo"](request) -> "/"
/// routes.not_found = function(request) return "404" end
impl LuaUserData for Routes {
    fn add_fields<'lua, F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_set("not_found", |_, this, function: LuaFunction| {
            this.not_found = function;
            Ok(())
        });
        fields.add_field_method_set("websocket", |_, this, function: LuaFunction| {
            this.ws.replace(function);
            Ok(())
        });
    }

    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method_mut(
            LuaMetaMethod::NewIndex,
            |_, this, (key, value): (LuaString, LuaFunction)| {
                let key = key.to_str()?;
                if !key.starts_with("/") {
                    return Err(LuaError::runtime("routes must start with /"));
                }
                let size = this.tree.insert(&key, value);
                Ok(size)
            },
        );
    }
}
