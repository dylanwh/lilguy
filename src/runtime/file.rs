// this is an async implementation of the `io` module

use std::io::SeekFrom;

use mlua::prelude::*;

pub fn register(lua: &Lua) -> LuaResult<()> {
    let file = lua.create_table()?;
    file.set("open", lua.create_async_function(file_open)?)?;
    file.set("type", lua.create_function(file_type)?)?;
    file.set("read", lua.create_async_function(file_read)?)?;
    file.set("write", lua.create_async_function(file_write)?)?;
    file.set("remove", lua.create_async_function(file_remove)?)?;
    file.set("rename", lua.create_async_function(file_rename)?)?;
    file.set("exists", lua.create_async_function(file_exists)?)?;
    file.set("create_dir", lua.create_async_function(create_dir)?)?;
    file.set("create_dir_all", lua.create_async_function(create_dir_al)?)?;
    file.set("temp", lua.create_function(file_temp)?)?;
    lua.globals().set("file", file)?;
    Ok(())
}

use tempfile::NamedTempFile;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader};

pub struct LuaFile {
    file: BufReader<tokio::fs::File>,
}

impl LuaUserData for LuaFile {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        // reads exactly n bytes from the file. It raises an error if the end of file
        // is reached before reading the requested number of bytes.
        methods.add_async_method_mut("read_exact", |lua, mut this, size: usize| async move {
            let mut buf = vec![0; size];
            this.file.read_exact(&mut buf).await?;

            lua.create_string(&buf)
        });

        // read a line
        methods.add_async_method_mut("read_line", |lua, mut this, _: ()| async move {
            let mut buf = Vec::new();
            this.file.read_until(b'\n', &mut buf).await?;
            lua.create_string(&buf)
        });

        // read until a byte is found
        methods.add_async_method_mut("read_until", |lua, mut this, byte: u8| async move {
            let mut buf = Vec::new();
            this.file.read_until(byte, &mut buf).await?;
            (lua.create_string(&buf))
        });

        // read_to_end
        methods.add_async_method_mut("read_to_end", |lua, mut this, _: ()| async move {
            let mut buf = Vec::new();
            this.file.read_to_end(&mut buf).await?;
            lua.create_string(&buf)
        });

        // Writes the value of each of its arguments to file. The arguments must be
        // strings or numbers.  Contrary to the upstream implementation, this function
        // will raise an error instead of returning nil if the write fails.
        methods.add_async_method_mut("write", |_, mut this, args: LuaMultiValue| async move {
            let file = this.file.get_mut();
            let mut buf = Vec::new();
            for arg in args {
                match arg {
                    LuaValue::String(s) => buf.extend_from_slice(&s.as_bytes()),
                    LuaValue::Integer(i) => buf.extend_from_slice(i.to_string().as_bytes()),
                    LuaValue::Number(n) => buf.extend_from_slice(n.to_string().as_bytes()),
                    _ => return Err(LuaError::external("invalid argument")),
                }
            }
            file.write_all(&buf).await?;
            Ok(())
        });

        methods.add_async_method_mut("flush", |_, mut this, _: ()| async move {
            this.file.get_mut().flush().await?;
            Ok(())
        });

        methods.add_async_method_mut("close", |_, mut this, _: ()| async move {
            this.file.get_mut().shutdown().await?;
            Ok(())
        });

        // Sets and gets the file position, measured from the beginning of the file, to the position given by offset plus a base specified by the string whence, as follows:

        // "set": base is position 0 (beginning of the file);
        // "cur": base is current position;
        // "end": base is end of file;
        // In case of success, seek returns the final file position, measured in bytes from the beginning of the file. If seek fails, it returns nil, plus a string describing the error.

        // The default value for whence is "cur", and for offset is 0. Therefore, the
        // call file:seek() returns the current file position, without changing it; the
        // call file:seek("set") sets the position to the beginning of the file (and
        // returns 0); and the call file:seek("end") sets the position to the end of the
        // file, and returns its size.
        methods.add_async_method_mut(
            "seek",
            |_, mut this, (whence, offset): (Option<String>, Option<i64>)| async move {
                let whence = match whence.as_deref() {
                    Some("set") => SeekFrom::Start(offset.unwrap_or(0) as u64),
                    Some("cur") => SeekFrom::Current(offset.unwrap_or(0)),
                    Some("end") => SeekFrom::End(offset.unwrap_or(0)),
                    _ => return Err(LuaError::external("invalid whence")),
                };
                let pos = this.file.seek(whence).await?;
                Ok(pos as i64)
            },
        );
    }
}

// This function opens a file, in the mode specified in the string mode. In case of success, it returns a new file handle.

// The mode string can be any of the following:

// "r": read mode (the default);
// "w": write mode;
// "a": append mode;
// "r+": update mode, all previous data is preserved;
// "w+": update mode, all previous data is erased;
// "a+": append update mode, previous data is preserved, writing is only allowed at the end of file.
// The "b" suffix not supported, on all platforms a line is terminated solely by '\n'
// (because that is how Rust works).
async fn file_open(lua: Lua, (path, mode): (String, Option<String>)) -> LuaResult<LuaAnyUserData> {
    let file = match mode.as_deref() {
        Some("r") | None => tokio::fs::File::open(path).await?,
        Some("w") => tokio::fs::File::create(path).await?,
        Some("a") => {
            tokio::fs::OpenOptions::new()
                .append(true)
                .open(path)
                .await?
        }
        Some("r+") => {
            tokio::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(path)
                .await?
        }
        Some("w+") => {
            tokio::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(true)
                .open(path)
                .await?
        }
        Some("a+") => {
            tokio::fs::OpenOptions::new()
                .read(true)
                .append(true)
                .open(path)
                .await?
        }
        _ => return Err(LuaError::external("invalid mode")),
    };

    lua.create_userdata(LuaFile {
        file: BufReader::new(file),
    })
}

/// Checks whether obj is a valid file handle.
///
/// Returns the string "file" if obj is an open file handle, "closed file" if obj is a
/// closed file handle, or nil if obj is not a file handle.
fn file_type(_lua: &Lua, value: LuaValue) -> LuaResult<String> {
    match value {
        LuaValue::UserData(ud) if ud.is::<LuaFile>() => Ok("file".to_string()),
        _ => Ok("nil".to_string()),
    }
}

// read in an entire file
async fn file_read(lua: Lua, filename: String) -> LuaResult<LuaString> {
    let data = tokio::fs::read(filename)
        .await
        .map_err(LuaError::external)?;

    lua.create_string(&data)
}

async fn file_write(_lua: Lua, (filename, data): (String, Vec<u8>)) -> LuaResult<()> {
    tokio::fs::write(filename, data)
        .await
        .map_err(LuaError::external)
}

async fn file_rename(_lua: Lua, (old, new): (String, String)) -> LuaResult<()> {
    tokio::fs::rename(old, new)
        .await
        .map_err(LuaError::external)
}

async fn file_exists(_lua: Lua, filename: String) -> LuaResult<bool> {
    tokio::fs::metadata(filename)
        .await
        .map(|_| true)
        .or_else(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Ok(false)
            } else {
                Err(LuaError::external(e))
            }
        })
}

async fn create_dir(_lua: Lua, path: String) -> LuaResult<()> {
    tokio::fs::create_dir(path)
        .await
        .map_err(LuaError::external)
}

async fn create_dir_al(_lua: Lua, path: String) -> LuaResult<()> {
    tokio::fs::create_dir_all(path)
        .await
        .map_err(LuaError::external)
}

async fn file_remove(_lua: Lua, filename: String) -> LuaResult<()> {
    tokio::fs::remove_file(filename)
        .await
        .map_err(LuaError::external)
}

fn file_temp(_lua: &Lua, _args: LuaValue) -> LuaResult<String> {
    NamedTempFile::new()
        .map(|f| f.path().to_string_lossy().to_string())
        .map_err(LuaError::external)
}
