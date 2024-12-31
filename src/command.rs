pub mod new;
// pub mod query;
// pub mod render;
// pub mod run;
pub mod serve;

use crate::runtime::{self, Runtime};
use clap::{Parser, Subcommand};
use serve::Serve;
use std::path::PathBuf;

use new::{New, NewError};

#[derive(Debug, Parser)]
pub struct Args {
    /// change to the specified directory before running the command
    #[clap(short = 'C', long = "chdir", default_value = ".")]
    pub directory: PathBuf,

    /// the subcommand to run
    #[clap(subcommand)]
    pub command: Command,
}

impl Args {
    pub fn new() -> Self {
        Self::parse()
    }

    pub async fn run(self) -> Result<(), CommandError> {
        let directory = self.directory.canonicalize()?;
        std::env::set_current_dir(&directory)?;
        self.command.run(directory).await
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
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// initialize a new project
    New(New),

    // /// run sql queries
    // #[clap(alias = "sql")]
    // Query(Query),

    /// run the web server
    Serve(Serve),
}

impl Command {
    async fn run(self, cwd: PathBuf) -> Result<(), CommandError> {
        let runtime = Runtime::new(cwd.clone());

        match self {
            Command::New(new) => {
                new.run(cwd).await?;
                Ok(())
            }
            Command::Serve(serve) => {
                serve.run(runtime).await?;
                Ok(())
            }
        }
    }
}
