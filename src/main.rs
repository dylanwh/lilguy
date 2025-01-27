mod command;
mod database;
mod repl;
mod routes;
mod runtime;
mod template;
mod watch;

use mimalloc::MiMalloc;
use parking_lot::Mutex;
use reedline::ExternalPrinter;
use std::{io::IsTerminal, process::exit, sync::Arc, time::Duration};
use tokio::time::sleep;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tracing_subscriber::{fmt::MakeWriter, EnvFilter};

use command::Args;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

#[derive(Clone)]
pub struct Output {
    writer: Arc<Mutex<Box<dyn std::io::Write + Send + Sync>>>,
    printer: Arc<Mutex<Option<ExternalPrinter<String>>>>,
}

impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Output").finish()
    }
}

impl Output {
    pub fn set_printer(&self, printer: ExternalPrinter<String>) {
        *self.printer.lock() = Some(printer);
    }
}

impl std::io::Write for Output {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Some(printer) = self.printer.lock().as_ref() {
            printer
                .print(String::from_utf8_lossy(buf).to_string())
                .map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "failed to write to external printer",
                    )
                })?;
            Ok(buf.len())
        } else {
            self.writer.lock().write(buf)
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.printer.lock().is_none() {
            self.writer.lock().flush()?;
        }
        Ok(())
    }
}

impl MakeWriter<'_> for Output {
    type Writer = Self;

    fn make_writer(&self) -> Self::Writer {
        self.clone()
    }
}

#[tokio::main]
async fn main() -> Result<(), eyre::Report> {
    let output = Output {
        writer: Arc::new(Mutex::new(Box::new(std::io::stderr()))),
        printer: Arc::new(Mutex::new(None)),
    };
    init_tracing_subscriber(output.clone());

    let args = Args::new();
    let token = CancellationToken::new();
    let tracker = TaskTracker::new();
    let timeout = Duration::from_secs(args.timeout);

    tokio::spawn({
        let token = token.clone();
        async move {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to listen for ctrl-c");
            token.cancel();
            sleep(timeout).await;
            eprintln!("timed out waiting for tasks to finish");
            exit(1);
        }
    });

    args.run(token, tracker, output).await
}

fn init_tracing_subscriber(output: Output) {
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
        // .with_span_events(FmtSpan::ENTER | FmtSpan::EXIT)
        .with_env_filter(filter)
        .with_ansi(is_terminal)
        .compact()
        .with_writer(output);

    // Set the subscriber as the default
    tracing::subscriber::set_global_default(subscriber.finish())
        .expect("Failed to set tracing subscriber");
}
