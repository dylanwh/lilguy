pub mod global;

use std::path::Path;
use tokio_rusqlite::Connection;

#[derive(Debug, Clone)]
pub struct Database(Connection);
pub type DatabaseError = tokio_rusqlite::Error;

impl AsRef<Connection> for Database {
    fn as_ref(&self) -> &Connection {
        &self.0
    }
}

impl From<Connection> for Database {
    fn from(conn: Connection) -> Self {
        Self(conn)
    }
}

impl Database {
    pub async fn open(name: &str, root: &Path) -> Result<Self, DatabaseError> {
        let path = root.join(format!("{}.db", name));
        let conn = Connection::open(path).await?;
        Ok(conn.into())
    }
}
