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

impl Database {
    pub async fn open(root: &Path) -> Result<Self, DatabaseError> {
        let path = root.join("app.db");
        let conn = Connection::open(&path).await?;
        Ok(Self( conn))
    }
}

