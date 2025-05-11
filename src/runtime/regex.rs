use mlua::prelude::*;

pub struct LuaRegex {
    regex: regex::Regex,
}

impl LuaRegex {
    pub fn pattern(&self) -> &str {
        self.regex.as_str()
    }
}

pub fn register(lua: &Lua) -> LuaResult<()> {
    let globals = lua.globals();
    globals.set("regex", lua.create_function(regex_new)?)?;

    Ok(())
}

fn regex_new(_lua: &Lua, pattern: String) -> LuaResult<LuaRegex> {
    let regex = regex::Regex::new(&pattern).into_lua_err()?;
    Ok(LuaRegex { regex })
}

impl LuaUserData for LuaRegex {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("find", |_, this, text: String| {
            Ok(this.regex.find(&text).map(|m| m.as_str().to_string()))
        });

        methods.add_method("is_match", |_, this, text: String| {
            Ok(this.regex.is_match(&text))
        });

        methods.add_method("replace", |_, this, (text, replace): (String, String)| {
            Ok(this.regex.replace_all(&text, replace.as_str()).to_string())
        });

        methods.add_method("captures", |lua, this, text: String| {
            if let Some(captures) = this.regex.captures(&text) {
                let result = lua.create_table()?;
                for (i, capture) in captures.iter().enumerate() {
                    if i == 0 {
                        continue;
                    }
                    let Some(capture) = capture else { continue };
                    result.set(i, capture.as_str())?;
                }
                for name in this.regex.capture_names() {
                    let Some(name) = name else { continue };
                    let Some(capture) = captures.name(name) else {
                        continue;
                    };
                    result.set(name, capture.as_str())?;
                }
                Ok(LuaValue::Table(result))
            } else {
                Ok(LuaValue::Nil)
            }
        });
    }
}
