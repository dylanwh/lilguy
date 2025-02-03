use std::borrow::Cow;

use mlua::prelude::*;

use crate::routes::Routes;

use super::{file::LuaFile, http::LuaCookies, regex::LuaRegex};

pub fn to_strings(values: LuaMultiValue) -> Vec<String> {
    let mut results = vec![];
    for value in values {
        results.push(stringify_value(0, value));
    }
    results
}

pub fn stringify_value(indent: usize, value: LuaValue) -> String {
    match value {
        LuaValue::Nil => "nil".to_string(),
        LuaValue::Boolean(b) => format!("{b}"),
        LuaValue::LightUserData(_) => "<lightuserdata>".to_string(),
        LuaValue::Integer(i) => format!("{i}"),
        LuaValue::Number(n) => format!("{n}"),
        LuaValue::String(s) => stringify_string(s),
        LuaValue::Table(t) => stringify_table(indent, t),
        LuaValue::Function(f) => stringify_function(indent, f),
        LuaValue::Thread(_) => "--[[thread]] nil".to_string(),
        LuaValue::UserData(ud) => stringify_userdata(ud).to_string(),
        LuaValue::Error(error) => format!("--[[error: {error}]] nil"),
        _ => "--[[other]] nil".to_string(),
    }
}

fn stringify_userdata<'a>(ud: LuaAnyUserData) -> Cow<'a, str> {
    if ud.is::<Routes>() {
        let routes = ud.borrow::<Routes>();
        let n = routes.iter().count();
        return format!("Routes [[ {n} routes ]]").into();
    }

    if ud.is::<LuaFile>() {
        return "file".into();
    }

    if ud.is::<LuaRegex>() {
        let Ok(regex) = ud.borrow::<LuaRegex>() else {
            return "Regex[[ ???? ]]".into();
        };
        let pattern = regex.pattern();
        return format!("Regex [[{pattern}]]").into();
    }

    if let Ok(cookies) = ud.borrow::<LuaCookies>() {
        let mut buffer = String::new();
        buffer.push_str("Cookies [[\n");
        for cookie in cookies.jar.iter() {
            buffer.push_str(&format!("  {cookie}\n"));
        }
        buffer.push_str("]]");
        return buffer.into();
    }

    "userdata".into()
}

fn stringify_function(_indent: usize, _f: LuaFunction) -> String {
    "function(...) return ... end".to_string()
}

fn stringify_string(s: mlua::String) -> String {
    let bytes = s.as_bytes();
    let s = s.to_str().expect("string is not valid utf-8");
    let mut seen_single = false;
    let mut seen_bracket = 0;
    let mut buffer = String::with_capacity(bytes.len() + 8);

    for i in 0..bytes.len() {
        let c = bytes.get(i);
        match c {
            Some(b'\'') => seen_single = true,
            Some(b'[') => {
                let mut new_seen_bracket = 0;
                for j in i + 1..bytes.len() {
                    let c = bytes.get(j);
                    match c {
                        Some(b'=') => new_seen_bracket += 1,
                        Some(b'[') => break,
                        _ => break,
                    }
                }
                if new_seen_bracket > seen_bracket {
                    seen_bracket = new_seen_bracket;
                }
            }
            _ => {}
        }
    }

    if !seen_single {
        buffer.push('\'');
    } else {
        buffer.push('[');
        buffer.push_str(&"=".repeat(seen_bracket + 1));
        buffer.push('[');
    }

    buffer.push_str(&s[..]);

    if !seen_single {
        buffer.push('\'');
    } else {
        buffer.push(']');
        buffer.push_str(&"=".repeat(seen_bracket + 1));
        buffer.push(']');
    }

    buffer
}

fn stringify_key(key: LuaValue) -> String {
    match key {
        LuaValue::String(s) => {
            let word = s.to_str().expect("string is not valid utf-8");
            if word.chars().all(|c| c.is_alphanumeric()) {
                format!("{word}")
            } else {
                format!("[{}]", stringify_string(s))
            }
        }
        _ => format!("[{}]", stringify_value(0, key)),
    }
}

fn stringify_table(indent: usize, table: LuaTable) -> String {
    let mut buffer = String::new();
    if table.is_empty() {
        buffer.push_str("{}");
        return buffer;
    }

    buffer.push_str("{\n");

    // For sequence values, increase indent for both the value and its container
    table.sequence_values().for_each(|value| {
        let value = value.expect("table value is valid");
        buffer.push_str(&"  ".repeat(indent + 1));
        buffer.push_str(&stringify_value(indent + 1, value)); // Increase indent
        buffer.push_str(",\n");
    });

    // Same for key-value pairs
    table.pairs().for_each(|pair| {
        let (key, value): (LuaValue, LuaValue) = pair.expect("table pair is valid");
        if key.is_integer() {
            return;
        }
        buffer.push_str(&"  ".repeat(indent + 1));
        buffer.push_str(&stringify_key(key));
        buffer.push_str(" = ");
        buffer.push_str(&stringify_value(indent + 1, value)); // Increase indent
        buffer.push_str(",\n");
    });

    buffer.push_str(&"  ".repeat(indent));
    buffer.push('}');

    buffer
}
