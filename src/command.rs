// pub mod render;
mod new;
mod query;
mod run;
mod serve;
mod shell;

use clap::{Parser, Subcommand};
use eyre::Result;
use serde::{Deserialize, Serialize};
use shell::Shell;
use std::{path::PathBuf, sync::Arc};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::Output;

use super::runtime::Runtime;
use new::New;
use query::Query;
use run::Run;
use serve::Serve;

#[derive(Debug, Parser)]
pub struct Args {
    /// the subcommand to run
    #[clap(subcommand)]
    pub command: Command,

    /// the path to the Config file (defaults to a platform specific location)
    #[clap(short = 'c', long = "config")]
    pub config_path: Option<PathBuf>,

    /// timeout - when ctrl-c is pressed the app will wait no longer than this before exiting
    #[clap(short = 'T', long, default_value = "30")]
    pub timeout: u64,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    pub shell: crate::repl::Config,
}

impl Args {
    pub fn new() -> Self {
        Self::parse()
    }

    fn config_path(&self) -> PathBuf {
        self.config_path
            .clone()
            .or_else(|| {
                dirs::config_dir().map(|dir| dir.join(env!("CARGO_PKG_NAME")).join("config.toml"))
            })
            .expect("could not determine config path")
    }

    async fn read_config(&self) -> Result<Config> {
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
    ) -> Result<()> {
        let config = Arc::new(self.read_config().await?);

        self.command.run(token, tracker, config, output).await
    }
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
    async fn run(
        self,
        token: CancellationToken,
        tracker: TaskTracker,
        config: Arc<Config>,
        output: Output,
    ) -> Result<()> {
        match self {
            Command::New(new) => {
                new.run().await?;
                token.cancel();
            }
            Command::Serve(serve) => {
                serve.run(&token, &tracker, &config, &output).await?;
            }
            Command::Run(run) => {
                run.run(&token, &tracker).await?;
                token.cancel();
            }
            Command::Query(query) => {
                query.run().await?;
            }
            Command::Shell(shell) => {
                shell.run(&token, &tracker, &config, &output).await?;
            }
        }
        tracker.close();
        tracker.wait().await;

        Ok(())
    }
}
