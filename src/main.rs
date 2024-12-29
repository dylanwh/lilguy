mod command;
mod database;
mod runtime;
mod template;
mod reload;

use command::Args;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    ignore_not_found(dotenv::dotenv())?;
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env())
        .init();

    let args = Args::new();

    args.run().await?;

    Ok(())
}

/// Ignore `NotFound` errors from `dotenv::dotenv()`.
/// We don't care if the `.env` file is missing but the other errors are important.
fn ignore_not_found<T>(result: Result<T, dotenv::Error>) -> Result<(), dotenv::Error> {
    use dotenv::Error::Io;
    use std::io::ErrorKind::NotFound;

    match result {
        Ok(_) => Ok(()),
        Err(Io(ref io_err)) if io_err.kind() == NotFound => Ok(()),
        Err(err) => Err(err),
    }
}
