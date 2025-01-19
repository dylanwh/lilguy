// pub mod render;
pub mod new;
pub mod query;
pub mod run;
pub mod serve;
pub mod shell;

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use shell::Shell;
use std::{path::PathBuf, sync::Arc};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::Output;

use super::runtime::{self, Runtime};
use new::{New, NewError};
use query::Query;
use run::Run;
use serve::Serve;

#[derive(Debug, Parser)]
pub struct Args {
    /// the subcommand to run
    #[clap(subcommand)]
    pub command: Command,

    /// change to the specified directory before running the command
    #[clap(short = 'C', long = "chdir", default_value = ".")]
    pub directory: PathBuf,

    /// the path to the configuration file (defaults to a platform specific location)
    #[clap(short = 'c', long = "config")]
    pub config_path: Option<PathBuf>,

    /// timeout - when ctrl-c is pressed the app will wait no longer than this before exiting
    #[clap(short = 'T', long, default_value = "30")]
    pub timeout: u64,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    pub shell: shell::Config,
}

impl Args {
    pub fn new() -> Self {
        Self::parse()
    }

    fn config_path(&self) -> PathBuf {
        self.config_path
            .clone()
            .or_else(|| {
                dirs::config_dir().map(|dir| dir.join(env!("CARGO_PKG_NAME")).join("config"))
            })
            .expect("could not determine config path")
    }

    async fn read_config(&self) -> Result<Config, CommandError> {
        let config_path = self.config_path();
        match tokio::fs::read_to_string(&config_path).await {
            Ok(config) => {
                let config = toml::from_str(&config)?;
                Ok(config)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                let config = Config::default();
                if let Some(parent) = config_path.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::write(config_path, toml::to_string_pretty(&config)?).await?;
                Ok(config)
            }
            Err(err) => Err(err.into()),
        }
    }

    #[tracing::instrument(level = "debug", skip(output))]
    pub async fn run(
        self,
        token: CancellationToken,
        tracker: TaskTracker,
        output: Output,
    ) -> Result<(), CommandError> {
        let config = Arc::new(self.read_config().await?);
        let directory = self.directory.canonicalize()?;
        std::env::set_current_dir(&directory)?;
        let context = Context {
            token,
            tracker,
            directory,
            config,
            output,
        };

        self.command.run(context).await
    }
}

#[derive(Debug, Clone)]
pub struct Context {
    pub token: CancellationToken,
    pub tracker: TaskTracker,
    pub directory: PathBuf,
    pub config: Arc<Config>,
    pub output: Output,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// initialize a new project
    New(New),

    #[clap(alias = "sql")]
    Query(Query),

    /// run a function
    Run(Run),

    /// run the web server
    Serve(Serve),

    /// run the shell
    Shell(Shell),
}

impl Command {
    #[tracing::instrument(level = "debug")]
    async fn run(self, context: Context) -> Result<(), CommandError> {
        let runtime = Runtime::new(&context);

        match self {
            Command::New(new) => {
                new.run(&context).await?;
                context.token.cancel();
            }
            Command::Serve(serve) => {
                serve.run(&context, runtime).await?;
            }
            Command::Run(run) => {
                run.run(runtime).await?;
                context.token.cancel();
            }
            Command::Query(query) => {
                runtime.start_services()?;
                query.run(runtime.database()?).await?;
                context.token.cancel();
            }
            Command::Shell(shell) => {
                runtime.start_services()?;
                shell.run(&context, runtime).await?;
            }
        }
        context.tracker.close();
        context.tracker.wait().await;

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("runtime error: {0}")]
    Runtime(#[from] runtime::Error),

    #[error("template error: {0}")]
    Template(#[from] minijinja::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("new error: {0}")]
    New(#[from] NewError),

    #[error("serve error: {0}")]
    Serve(#[from] serve::Error),

    #[error("query error: {0}")]
    Query(#[from] query::Error),

    #[error("shell error: {0}")]
    Shell(#[from] shell::Error),

    #[error("config read error: {0}")]
    Config(#[from] toml::de::Error),

    #[error("config write error: {0}")]
    ConfigWrite(#[from] toml::ser::Error),
}
