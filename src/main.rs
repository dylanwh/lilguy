mod command;
mod database;
mod watch;
mod runtime;
mod template;
mod routes;

use std::io::IsTerminal;
use tracing_subscriber::{
    fmt::format::FmtSpan, EnvFilter,
};

use command::Args;

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    ignore_not_found(dotenv::dotenv())?;
    init_tracing_subscriber();

    tracing::debug!("starting up");

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

fn init_tracing_subscriber() {
    // Set up filter based on RUST_LOG env var or default to "info"
    let my_crate = env!("CARGO_PKG_NAME").replace("-", "_");
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(format!("info,{my_crate}=info")));

    let is_terminal = std::io::stderr().is_terminal();

    // Create a single formatting layer with all desired features
    let subscriber = tracing_subscriber::fmt::Subscriber::builder()
        .with_target(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(false)
        .with_line_number(false)
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
        .with_env_filter(filter)
        .with_ansi(is_terminal)
        .compact()
        .with_writer(std::io::stderr);

    // Set the subscriber as the default
    tracing::subscriber::set_global_default(subscriber.finish())
        .expect("Failed to set tracing subscriber");
}
