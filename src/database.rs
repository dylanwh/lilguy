#![allow(unused)]
// this was initially copied from tokio-rusqlite and modified to fit the needs of this project
pub mod global;

use mlua::prelude::*;
use std::{path::Path, thread};
use tokio::sync::{
    mpsc::{error::SendError, unbounded_channel, UnboundedReceiver, UnboundedSender},
    oneshot::{self},
};

const BUG_TEXT: &str = "bug in lilguy::database";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The connection to the SQLite has been closed and cannot be queried any more.
    #[error("connection closed")]
    ConnectionClosed,

    /// An error occured while closing the SQLite connection.
    /// This `Error` variant contains the [`Connection`], which can be used to retry the close operation
    /// and the underlying [`rusqlite::Error`] that made it impossile to close the database.
    #[error("error closing connection: {1}")]
    Close(Database, rusqlite::Error),

    /// A `Rusqlite` error occured.
    #[error(transparent)]
    Rusqlite(#[from] rusqlite::Error),

    /// An application-specific error occured.
    #[error("application error: {0}")]
    Other(Box<dyn std::error::Error + Send + Sync + 'static>),
}

/// The result returned on method calls in this crate.
pub type Result<T> = std::result::Result<T, Error>;

type CallFn = Box<dyn FnOnce(&mut rusqlite::Connection) + Send + 'static>;

enum Message {
    Execute(CallFn),
    Close(oneshot::Sender<std::result::Result<(), rusqlite::Error>>),
}

/// A handle to call functions in background thread.
#[derive(Debug, Clone)]
pub struct Database {
    sender: UnboundedSender<Message>,
}

impl Database {
    /// Open a new connection to a SQLite database.
    ///
    /// `Connection::open(path)` is equivalent to
    /// `Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE |
    /// OpenFlags::SQLITE_OPEN_CREATE)`.
    ///
    /// # Failure
    ///
    /// Will return `Err` if `path` cannot be converted to a C-compatible
    /// string or if the underlying SQLite open call fails.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_owned();
        tokio::task::block_in_place(|| {
            start(move || rusqlite::Connection::open(path)).map_err(Into::into)
        })
    }

    /// Open a new connection to an in-memory SQLite database.
    ///
    /// # Failure
    ///
    /// Will return `Err` if the underlying SQLite open call fails.
    pub fn open_in_memory() -> Result<Self> {
        tokio::task::block_in_place(|| {
            start(rusqlite::Connection::open_in_memory).map_err(Into::into)
        })
    }

    /// Call a function in background thread and get the result
    /// asynchronously.
    ///
    /// # Failure
    ///
    /// Will return `Err` if the database connection has been closed.
    pub async fn call<F, R>(&self, function: F) -> Result<R>
    where
        F: FnOnce(&mut rusqlite::Connection) -> Result<R> + 'static + Send,
        R: Send + 'static,
    {
        let (sender, receiver) = oneshot::channel::<Result<R>>();

        self.sender
            .send(Message::Execute(Box::new(move |conn| {
                let value = function(conn);
                let _ = sender.send(value);
            })))
            .map_err(|_| Error::ConnectionClosed)?;

        receiver.await.map_err(|_| Error::ConnectionClosed)?
    }

    pub fn blocking_call<F, R>(&self, function: F) -> Result<R>
    where
        F: FnOnce(&mut rusqlite::Connection) -> Result<R> + 'static + Send,
        R: Send + 'static,
    {
        let (sender, receiver) = oneshot::channel::<Result<R>>();

        self.sender
            .send(Message::Execute(Box::new(move |conn| {
                let value = function(conn);
                let _ = sender.send(value);
            })))
            .map_err(|_| Error::ConnectionClosed)?;

        receiver
            .blocking_recv()
            .map_err(|_| Error::ConnectionClosed)?
    }

    /// Close the database connection.
    ///
    /// This is functionally equivalent to the `Drop` implementation for
    /// `Connection`. It consumes the `Connection`, but on error returns it
    /// to the caller for retry purposes.
    ///
    /// If successful, any following `close` operations performed
    /// on `Connection` copies will succeed immediately.
    ///
    /// # Failure
    ///
    /// Will return `Err` if the tokio-rusqlitederlying SQLite close call fails.
    pub async fn close(self) -> Result<()> {
        let (sender, receiver) = oneshot::channel::<std::result::Result<(), rusqlite::Error>>();

        if let Err(SendError(_)) = self.sender.send(Message::Close(sender)) {
            // If the channel is closed on the other side, it means the connection closed successfully
            // This is a safeguard against calling close on a `Copy` of the connection
            return Ok(());
        }

        match receiver.await {
            // If we get a RecvError at this point, it also means the channel closed in the meantime
            // we can assume the connection is closed
            Err(_) => Ok(()),
            Ok(Err(e)) => Err(Error::Close(self, e)),
            Ok(Ok(v)) => Ok(v),
        }
    }
}

impl From<rusqlite::Connection> for Database {
    fn from(conn: rusqlite::Connection) -> Self {
        let (sender, receiver) = unbounded_channel::<Message>();
        thread::spawn(move || event_loop(conn, receiver));

        Self { sender }
    }
}

fn start<F>(open: F) -> rusqlite::Result<Database>
where
    F: FnOnce() -> rusqlite::Result<rusqlite::Connection> + Send + 'static,
{
    let (sender, receiver) = unbounded_channel::<Message>();
    let (result_sender, result_receiver) = oneshot::channel();

    thread::spawn(move || {
        let conn = match open() {
            Ok(c) => c,
            Err(e) => {
                let _ = result_sender.send(Err(e));
                return;
            }
        };

        if let Err(_e) = result_sender.send(Ok(())) {
            return;
        }

        event_loop(conn, receiver);
    });

    result_receiver
        .blocking_recv()
        .expect(BUG_TEXT)
        .map(|_| Database { sender })
}

fn event_loop(mut conn: rusqlite::Connection, mut receiver: UnboundedReceiver<Message>) {
    while let Some(message) = receiver.blocking_recv() {
        match message {
            Message::Execute(f) => f(&mut conn),
            Message::Close(s) => {
                let result = conn.close();

                match result {
                    Ok(v) => {
                        s.send(Ok(v)).expect(BUG_TEXT);
                        break;
                    }
                    Err((c, e)) => {
                        conn = c;
                        s.send(Err(e)).expect(BUG_TEXT);
                    }
                }
            }
        }
    }
}

impl LuaUserData for Database {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {}

    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {}

    fn register(registry: &mut LuaUserDataRegistry<Self>) {
        Self::add_fields(registry);
        Self::add_methods(registry);
    }
}
