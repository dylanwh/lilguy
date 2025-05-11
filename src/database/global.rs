use super::Database;
use mlua::prelude::*;
use rusqlite::{params, OptionalExtension, Row, ToSql};
use serde::{de::DeserializeOwned, Serialize};
use tokio::{
    sync::mpsc::{self, Receiver},
    task::block_in_place,
};

#[derive(Debug, thiserror::Error)]
pub enum GlobalTableError {
    #[error("database error: {0}")]
    Database(#[from] super::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("sqlite jsonb error: {0}")]
    Jsonb(#[from] serde_sqlite_jsonb::Error),

    #[error("invalid key")]
    InvalidKey,
}

/// Handle to reference a global table in the database.
/// This is table in the lua sense.
/// Each one maps to a sqlite table, but the schema is always the same.
/// The contents are (id, optional key, value).
#[derive(Debug)]
pub struct GlobalTable {
    pub name: String,
    pub database: Database,
}

#[derive(Debug, Clone)]
pub enum GlobalTableKey {
    Int(i64),
    Str(String),
}

impl GlobalTableKey {
    pub fn column(&self) -> &'static str {
        match self {
            GlobalTableKey::Int(_) => "key_int",
            GlobalTableKey::Str(_) => "key_str",
        }
    }
}

impl Serialize for GlobalTableKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::ser::Serializer,
    {
        match self {
            GlobalTableKey::Int(key) => serializer.serialize_i64(*key),
            GlobalTableKey::Str(key) => serializer.serialize_str(key),
        }
    }
}

impl From<i64> for GlobalTableKey {
    fn from(key: i64) -> Self {
        Self::Int(key)
    }
}

impl From<&str> for GlobalTableKey {
    fn from(key: &str) -> Self {
        Self::Str(key.to_owned())
    }
}

impl From<String> for GlobalTableKey {
    fn from(key: String) -> Self {
        Self::Str(key)
    }
}

impl TryFrom<LuaValue> for GlobalTableKey {
    type Error = GlobalTableError;

    fn try_from(value: LuaValue) -> Result<Self, Self::Error> {
        let value = match value {
            LuaValue::Integer(key) => key.into(),
            key => key
                .to_string()
                .map_err(|_| GlobalTableError::InvalidKey)?
                .into(),
        };

        Ok(value)
    }
}

impl ToSql for GlobalTableKey {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        match self {
            GlobalTableKey::Int(key) => key.to_sql(),
            GlobalTableKey::Str(key) => key.to_sql(),
        }
    }
}

pub struct GlobalTablePairs<V: DeserializeOwned>(
    pub Receiver<Result<(GlobalTableKey, V), GlobalTablePairsError>>,
);

impl GlobalTable {
    fn new(name: String, database: Database) -> Self {
        Self { name, database }
    }

    fn sql_name(&self) -> String {
        format!("\"lg_global_{}\"", self.name.replace("\"", "\"\""))
    }

    pub fn create(&self) -> Result<(), super::Error> {
        let sql_name = self.sql_name();
        self.database.blocking_call(move |conn| {
            conn.execute(
                &format!(
                    r"
                            CREATE TABLE IF NOT EXISTS {sql_name} (
                                key_int INTEGER UNIQUE,
                                key_str TEXT UNIQUE,
                                value JSONB NOT NULL,
                                PRIMARY KEY (key_int, key_str),
                                CHECK ((key_int IS NULL) != (key_str IS NULL))
                            )
                        "
                ),
                [],
            )?;

            Ok(())
        })?;

        Ok(())
    }

    pub async fn get<K, V>(&self, key: K) -> Result<Option<V>, GlobalTableError>
    where
        K: TryInto<GlobalTableKey>,
        V: DeserializeOwned,
    {
        let sql_name = self.sql_name();
        let key = key.try_into().map_err(|_| GlobalTableError::InvalidKey)?;
        let value = self
            .database
            .call(move |conn| {
                let sql = format!(
                    "SELECT jsonb(value) FROM {sql_name} WHERE {key_column} = ?",
                    key_column = key.column(),
                );
                let value: Option<Vec<u8>> =
                    conn.query_row(&sql, [key], |row| row.get(0)).optional()?;

                Ok(value)
            })
            .await?;

        let value = value
            .map(|value| serde_sqlite_jsonb::from_slice(&value[..]))
            .transpose()?;

        Ok(value)
    }

    pub async fn set<K, V>(&self, key: K, value: V) -> Result<(), GlobalTableError>
    where
        K: TryInto<GlobalTableKey>,
        V: Serialize,
    {
        let sql_name = self.sql_name();
        let key = key.try_into().map_err(|_| GlobalTableError::InvalidKey)?;
        let column = key.column();
        let value = serde_sqlite_jsonb::to_vec(&value)?;

        self.database
            .call(move |conn| {
                let sql = format!(
                    "INSERT OR REPLACE INTO {sql_name} ({column}, value) VALUES (?, jsonb(?))",
                );
                conn.execute(&sql, params![key, value])?;
                Ok(())
            })
            .await?;

        Ok(())
    }

    pub async fn del<K>(&self, key: K) -> Result<(), GlobalTableError>
    where
        K: TryInto<GlobalTableKey>,
    {
        let sql_name = self.sql_name();
        let key = key.try_into().map_err(|_| GlobalTableError::InvalidKey)?;
        let column = key.column();

        self.database
            .call(move |conn| {
                conn.execute(
                    &format!("DELETE FROM {sql_name} WHERE {column} = ?",),
                    [key],
                )?;

                Ok(())
            })
            .await?;

        Ok(())
    }

    // TODO: pairs, ipairs, get numeric keys, set numeric keys, table.insert, len

    /// len - like in lua, returns the number of elements in the table with a key that is null
    pub async fn len(&self) -> Result<usize, GlobalTableError> {
        let sql_name = self.sql_name();
        let len: usize = self
            .database
            .call(move |conn| {
                let len = conn.query_row(
                    &format!("SELECT max(key_int) FROM {sql_name}",),
                    [],
                    |row| row.get(0),
                )?;

                Ok(len)
            })
            .await?;

        Ok(len)
    }

    /// this returns a channel that will return the key and value pairs
    pub async fn pairs<V>(&self) -> GlobalTablePairs<V>
    where
        V: DeserializeOwned + Send + 'static,
    {
        let sql_name = self.sql_name();
        let conn = self.database.clone();
        let (tx, rx) = mpsc::channel(1);

        tokio::spawn(async move {
            let sql = format!("SELECT key_int, key_str, jsonb(value) FROM {sql_name}");
            conn.call(move |conn| {
                let mut stmt = conn.prepare(&sql)?;
                let mut query = stmt.query([])?;

                while let Some(row) = query.next()? {
                    tx.blocking_send(do_pairs(row)).unwrap();
                }

                Ok(())
            })
            .await
            .unwrap();
        });

        GlobalTablePairs(rx)
    }

    pub async fn destroy(&self) -> Result<(), super::Error> {
        let sql_name = self.sql_name();
        self.database
            .call(move |conn| {
                conn.execute(&format!("DROP TABLE IF EXISTS {sql_name}",), [])?;

                Ok(())
            })
            .await?;

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GlobalTablePairsError {
    #[error("invalid keys")]
    InvalidKeys,

    #[error("async_rusqlite error: {0}")]
    Database(#[from] super::Error),

    #[error("rusqlite error: {0}")]
    Rusqlite(#[from] rusqlite::Error),

    #[error("jsonb error: {0}")]
    Jsonb(#[from] serde_sqlite_jsonb::Error),
}

fn do_pairs<V>(row: &Row<'_>) -> Result<(GlobalTableKey, V), GlobalTablePairsError>
where
    V: DeserializeOwned + Send + 'static,
{
    let key_int: Option<i64> = row.get(0)?;
    let key_str: Option<String> = row.get(1)?;
    let key = match (key_int, key_str) {
        (Some(key_int), None) => Ok(GlobalTableKey::Int(key_int)),
        (None, Some(key_str)) => Ok(GlobalTableKey::Str(key_str)),
        (_, _) => Err(GlobalTablePairsError::InvalidKeys),
    }?;
    let value: Vec<u8> = row.get(2)?;
    let value: V = serde_sqlite_jsonb::from_slice(&value[..])?;

    Ok((key, value))
}

impl LuaUserData for GlobalTablePairs<serde_json::Value> {
    // implement call which is an async function that calls recv
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_meta_method_mut(LuaMetaMethod::Call, |lua, mut this, ()| async move {
            let value = this
                .0
                .recv()
                .await
                .transpose()
                .into_lua_err()?;
            let mut mv = LuaMultiValue::new();

            match value {
                Some((key, val)) => {
                    mv.push_back(lua.to_value(&key)?);
                    mv.push_back(lua.to_value(&val)?);
                }
                None => mv.push_front(LuaValue::Nil),
            }

            Ok(mv)
        });
    }
}

#[derive(Debug)]
pub struct Global {
    database: Database,
}

impl Global {
    pub fn new(database: &Database) -> Self {
        Self {
            database: database.clone(),
        }
    }
}

// global.name creates a new GlobalTable
impl LuaUserData for Global {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(LuaMetaMethod::Index, |_lua, this, key: String| {
            let table = GlobalTable::new(key, this.database.clone());
            block_in_place(|| table.create().into_lua_err())?;
            Ok(table)
        });

        // global.name = nil deletes the table, no other values are allowed
        methods.add_async_meta_method(
            LuaMetaMethod::NewIndex,
            |_, this, (key, value): (String, LuaValue)| async move {
                if value.is_nil() {
                    let table = GlobalTable::new(key, this.database.clone());
                    table.destroy().await.into_lua_err()?;
                    return Ok(());
                }
                Err(LuaError::external("cannot set value on global"))
            },
        );
    }
}

impl LuaUserData for GlobalTable {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_meta_method(
            LuaMetaMethod::Index,
            |lua, this, key: LuaValue| async move {
                let value: Option<serde_json::Value> =
                    this.get(key).await.into_lua_err()?;
                if let Some(ref value) = value {
                    Ok(lua.to_value(value)?)
                } else {
                    Ok(LuaValue::Nil)
                }
            },
        );

        methods.add_async_meta_method(
            LuaMetaMethod::NewIndex,
            |_, this, (key, value): (LuaValue, LuaValue)| async move {
                let key = match key {
                    LuaValue::Integer(i) => GlobalTableKey::from(i),
                    key => GlobalTableKey::from(key.to_string()?),
                };
                if value.is_nil() {
                    this.del(key).await.into_lua_err()?;
                    return Ok(());
                }
                this.set(key, value).await.into_lua_err()?;
                Ok(())
            },
        );

        methods.add_async_meta_method(LuaMetaMethod::Len, |_, this, ()| async move {
            let len = this.len().await.into_lua_err()?;
            Ok(len as i64)
        });
    }
}
