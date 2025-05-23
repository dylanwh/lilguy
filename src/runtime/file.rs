// this is an async implementation of the `io` module

use mlua::prelude::*;
use parking_lot::Mutex;
use std::{io::SeekFrom, path::Path, sync::Arc};
use tempfile::{NamedTempFile, TempPath};
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, AsyncWriteExt, BufReader},
    sync::{mpsc, oneshot},
};
use walkdir::{DirEntry, WalkDir};

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
    file.set("walkdir", lua.create_function(file_walkdir)?)?;
    lua.globals().set("file", file)?;
    Ok(())
}

enum Message {
    Write(Vec<u8>),
    ReadExact(usize),
    ReadLine,
    ReadUntil(u8),
    ReadToEnd,
    Seek(SeekFrom),
    Flush,
    Close,
}

type Reply = LuaResult<LuaValue>;

pub struct LuaFile {
    tx: mpsc::Sender<(Message, oneshot::Sender<Reply>)>,
}

fn read_helper(lua: &Lua, result: std::io::Result<usize>, buffer: Vec<u8>) -> LuaResult<LuaValue> {
    result.into_lua_err().and_then(|len| {
        if len > 0 {
            lua.create_string(buffer).map(LuaValue::String)
        } else {
            Ok(LuaValue::Nil)
        }
    })
}

async fn file_actor(
    lua: Lua,
    file: File,
    mut rx: mpsc::Receiver<(Message, oneshot::Sender<Reply>)>,
) {
    let mut file = BufReader::new(file);

    while let Some((msg, reply)) = rx.recv().await {
        let res = match msg {
            Message::Write(src) => file
                .get_mut()
                .write_all(&src)
                .await
                .map(|_| LuaValue::Nil)
                .into_lua_err(),
            Message::ReadExact(len) => {
                let mut buf = vec![0; len];
                read_helper(&lua, file.read_exact(&mut buf).await, buf)
            }
            Message::ReadLine => {
                let mut buf = Vec::new();
                read_helper(&lua, file.read_until(b'\n', &mut buf).await, buf)
            }
            Message::ReadUntil(end) => {
                let mut buf = Vec::new();
                read_helper(&lua, file.read_until(end, &mut buf).await, buf)
            }
            Message::ReadToEnd => {
                let mut buf = Vec::new();
                read_helper(&lua, file.read_to_end(&mut buf).await, buf)
            }
            Message::Seek(whence) => file
                .seek(whence)
                .await
                .into_lua_err()
                .and_then(|pos| lua.to_value(&pos)),
            Message::Flush => file.flush().await.into_lua_err().map(|_| LuaValue::Nil),
            Message::Close => {
                if let Err(_) = reply.send(Ok(LuaValue::Boolean(true))) {
                    tracing::error!("error replying in LuaFile actor at close");
                }
                break;
            }
        };
        if let Err(_) = reply.send(res) {
            tracing::error!("error replying in LuaFile actor")
        }
    }
}

impl LuaFile {
    async fn spawn(lua: Lua, file: File) -> LuaFile {
        let (tx, rx) = mpsc::channel(1);

        tokio::spawn(async move {
            file_actor(lua, file, rx).await;
        });

        LuaFile { tx }
    }

    async fn send(&self, msg: Message) -> Reply {
        let (send_reply, reply) = oneshot::channel();
        self.tx
            .send((msg, send_reply))
            .await
            .map_err(|_| LuaError::runtime("error sending message to file actor"))?;

        match reply.await.into_lua_err() {
            Ok(r) => r,
            Err(e) => Err(e),
        }
    }
}

impl LuaUserData for LuaFile {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method("write", |_, this, args: LuaMultiValue| async move {
            let mut buf = Vec::new();
            for arg in args {
                match arg {
                    LuaValue::String(s) => buf.extend_from_slice(&s.as_bytes()),
                    LuaValue::Integer(i) => buf.extend_from_slice(i.to_string().as_bytes()),
                    LuaValue::Number(n) => buf.extend_from_slice(n.to_string().as_bytes()),
                    _ => return Err(LuaError::external("invalid argument")),
                }
            }

            this.send(Message::Write(buf)).await
        });

        methods.add_async_method("read_exact", |_, this, len: usize| async move {
            this.send(Message::ReadExact(len)).await
        });

        methods.add_async_method("read_line", |_lua, this, _: ()| async move {
            this.send(Message::ReadLine).await
        });

        methods.add_async_method("read_until", |_lua, this, byte: u8| async move {
            this.send(Message::ReadUntil(byte)).await
        });

        methods.add_async_method("read_to_end", |_lua, this, _: ()| async move {
            this.send(Message::ReadToEnd).await
        });

        methods.add_async_method("flush", |_, this, _: ()| async move {
            this.send(Message::Flush).await
        });

        methods.add_async_method("close", |_, this, _: ()| async move {
            this.send(Message::Close).await
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
        methods.add_async_method(
            "seek",
            |_, this, (whence, offset): (Option<String>, Option<i64>)| async move {
                let whence = match whence.as_deref() {
                    Some("set") => SeekFrom::Start(offset.unwrap_or(0) as u64),
                    Some("cur") | None => SeekFrom::Current(offset.unwrap_or(0)),
                    Some("end") => SeekFrom::End(offset.unwrap_or(0)),
                    _ => {
                        return Err(LuaError::external(
                            "invalid whence: must be set, cur, or end. default is cur",
                        ))
                    }
                };
                this.send(Message::Seek(whence)).await
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
async fn file_open(
    lua: Lua,
    (path, mode): (LuaValue, Option<String>),
) -> LuaResult<LuaAnyUserData> {
    let path = path.to_string()?;

    let file = match mode.as_deref() {
        Some("r") | None => File::open(path).await?,
        Some("w") => File::create(path).await?,
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

    lua.create_userdata(LuaFile::spawn(lua.clone(), file).await)
}

/// Checks whether obj is a valid file handle.
///
/// Returns the string "file" if obj is an open file handle, "closed file" if obj is a
/// closed file handle, or nil if obj is not a file handle.
fn file_type(_lua: &Lua, value: LuaValue) -> LuaResult<String> {
    match value {
        LuaValue::UserData(ud) if ud.is::<Arc<Mutex<LuaFile>>>() => Ok("file".to_string()),
        _ => Ok("nil".to_string()),
    }
}

// read in an entire file
async fn file_read(lua: Lua, filename: LuaValue) -> LuaResult<LuaString> {
    let filename = filename.to_string()?;
    let data = tokio::fs::read(filename).await.into_lua_err()?;

    lua.create_string(&data)
}

async fn file_write(_lua: Lua, (filename, data): (LuaValue, LuaString)) -> LuaResult<()> {
    let filename = filename.to_string()?;

    tokio::fs::write(filename, data.as_bytes())
        .await
        .into_lua_err()
}

async fn file_rename(_lua: Lua, (old, new): (LuaValue, LuaValue)) -> LuaResult<()> {
    let (old, new) = (old.to_string()?, new.to_string()?);
    tokio::fs::rename(old, new).await.into_lua_err()
}

async fn file_exists(_lua: Lua, filename: LuaValue) -> LuaResult<bool> {
    let filename = filename.to_string()?;

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
    tokio::fs::create_dir(path).await.into_lua_err()
}

async fn create_dir_al(_lua: Lua, path: String) -> LuaResult<()> {
    tokio::fs::create_dir_all(path).await.into_lua_err()
}

async fn file_remove(_lua: Lua, filename: String) -> LuaResult<()> {
    tokio::fs::remove_file(filename).await.into_lua_err()
}

pub struct LuaTempFile {
    file: Option<TempPath>,
}

impl LuaTempFile {
    pub fn close(&mut self) {
        self.file.take();
    }

    pub fn path(&self) -> Option<&Path> {
        self.file.as_deref()
    }
}

impl LuaUserData for LuaTempFile {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("path", |lua, this| {
            if let Some(path) = this.path() {
                Ok(LuaValue::String(create_string_from_path(lua, path)?))
            } else {
                Ok(LuaValue::Nil)
            }
        });
    }

    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("close", |_, this, _: ()| {
            this.close();
            Ok(())
        });

        methods.add_meta_method(LuaMetaMethod::ToString, |lua, this, _: ()| {
            if let Some(path) = this.path() {
                Ok(LuaValue::String(create_string_from_path(lua, path)?))
            } else {
                Ok(LuaValue::Nil)
            }
        });
    }
}

fn file_temp(lua: &Lua, _args: LuaValue) -> LuaResult<LuaAnyUserData> {
    let path = NamedTempFile::new()
        .map(|f| f.into_temp_path())
        .into_lua_err()?;

    lua.create_userdata(LuaTempFile { file: Some(path) })
}

pub struct LuaWalkDir {
    iter: Box<dyn Iterator<Item = Result<DirEntry, walkdir::Error>> + Send>,
}

fn file_walkdir(lua: &Lua, (path, opts): (String, Option<LuaTable>)) -> LuaResult<LuaAnyUserData> {
    let opts = opts.as_ref();
    let contents_first = opts
        .and_then(|opts| opts.get::<bool>("contents_first").ok())
        .unwrap_or(false);
    let follow_links = opts
        .and_then(|opts| opts.get::<bool>("follow_links").ok())
        .unwrap_or(false);
    let follow_root_links = opts
        .and_then(|opts| opts.get::<bool>("follow_root_links").ok())
        .unwrap_or(true);
    let max_depth = opts.and_then(|opts| opts.get::<usize>("max_depth").ok());
    let min_depth = opts.and_then(|opts| opts.get::<usize>("min_depth").ok());
    let same_file_system = opts
        .and_then(|opts| opts.get::<bool>("same_file_system").ok())
        .unwrap_or(false);

    let walker = WalkDir::new(path)
        .follow_links(follow_links)
        .follow_root_links(follow_root_links)
        .contents_first(contents_first)
        .same_file_system(same_file_system);

    let walker = if let Some(min_depth) = min_depth {
        walker.min_depth(min_depth)
    } else {
        walker
    };
    let walker = if let Some(max_depth) = max_depth {
        walker.max_depth(max_depth)
    } else {
        walker
    };

    let ud = lua.create_userdata(LuaWalkDir {
        iter: Box::new(walker.into_iter()),
    })?;
    Ok(ud)
}

impl LuaUserData for LuaWalkDir {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method_mut(LuaMetaMethod::Call, |lua, this, ()| {
            // make sure tokio has time to run

            let entry = this.iter.next().transpose().into_lua_err()?;
            let mut ret = LuaMultiValue::new();
            if let Some(entry) = entry {
                let path = create_string_from_path(lua, entry.path())?;
                ret.push_back(LuaValue::String(path));
                let ft = entry.file_type();
                if ft.is_dir() {
                    ret.push_back(lua.to_value("directory")?);
                } else if ft.is_file() {
                    ret.push_back(lua.to_value("file")?);
                } else if ft.is_symlink() {
                    ret.push_back(lua.to_value("symlink")?);
                } else {
                    ret.push_back(lua.to_value("unknown")?);
                }
            }

            Ok(ret)
        });
    }
}

#[cfg(windows)]
fn create_string_from_path<P>(lua: &Lua, path: P) -> LuaResult<LuaString>
where
    P: AsRef<Path>,
{
    lua.create_string(path.as_ref().as_os_str().as_encoded_bytes())
}

#[cfg(not(windows))]
fn create_string_from_path<P>(lua: &Lua, path: P) -> LuaResult<LuaString>
where
    P: AsRef<Path>,
{
    use std::os::unix::ffi::OsStrExt;
    lua.create_string(path.as_ref().as_os_str().as_bytes())
}
