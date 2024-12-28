use clap::Parser;
use prettytable::{Cell, Row};
use rusqlite::types::Value;

#[derive(Debug, Parser)]
pub struct Query {
    /// sql query to run
    pub query: String,
}

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("database error: {0}")]
    Database(#[from] tokio_rusqlite::Error),
}

impl Query {
    pub async fn run<D>(self, database: D) -> Result<(), QueryError>
    where
        D: AsRef<tokio_rusqlite::Connection>,
    {
        let query = self.query.clone();
        database
            .as_ref()
            .call(move |conn| {
                let mut stmt = conn.prepare(&query)?;
                let columns = stmt.column_count();

                let mut table = prettytable::Table::new();
                let names = Row::new(
                    stmt.column_names()
                        .iter()
                        .map(|name| Cell::new(name))
                        .collect(),
                );
                table.set_titles(names);

                stmt.query_map([], |row| {
                    let mut values = Vec::with_capacity(columns);
                    for i in 0..columns {
                        let row = row.get::<_, Value>(i)?;
                        let row = match row {
                            Value::Null => "NULL".to_string(),
                            Value::Integer(i) => i.to_string(),
                            Value::Real(r) => r.to_string(),
                            Value::Text(s) => s,
                            Value::Blob(_) => "blob".to_string(),
                        };
                        values.push(Cell::new(&row));
                    }
                    table.add_row(Row::new(values));

                    Ok(())
                })?
                .try_fold((), |(), item| item.map(|_| ()))?;

                if columns > 0 {
                    println!("{}", table);
                }

                Ok(())
            })
            .await?;

        Ok(())
    }
}
