pub mod new;
pub mod query;
pub mod render;
pub mod run;
pub mod serve;

use crate::{
    database::{Database, DatabaseError},
    reload::Reloaders,
    runtime::{Runtime, RuntimeInitError},
    template::Template,
};
use clap::{Parser, Subcommand};
use std::{path::PathBuf, sync::Arc};

use new::{New, NewError};
use query::{Query, QueryError};
use render::{Render, RenderError};
use run::{Run, RunError};
use serve::{Serve, ServeError};

#[derive(Debug, Parser)]
pub struct Args {
    /// change to the specified directory before running the command
    #[clap(short = 'C', long = "chdir", default_value = ".")]
    pub root: PathBuf,

    /// app name
    #[clap(short, long, default_value = "app")]
    pub name: String,

    /// the subcommand to run
    #[clap(subcommand)]
    pub command: Command,
}

impl Args {
    pub fn new() -> Self {
        Self::parse()
    }

    pub async fn run(self) -> Result<(), CommandError> {
        let root = self.root.canonicalize()?;
        self.command.run(AppContext { root }).await
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CommandError {
    #[error("database error: {0}")]
    Database(#[from] DatabaseError),

    #[error("template error: {0}")]
    Template(#[from] minijinja::Error),

    #[error("runtime error: {0}")]
    Runtime(#[from] RuntimeInitError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("query error: {0}")]
    Query(#[from] QueryError),

    #[error("new error: {0}")]
    New(#[from] NewError),

    #[error("render error: {0}")]
    Render(#[from] RenderError),

    #[error("call error: {0}")]
    Call(#[from] RunError),

    #[error("serve error: {0}")]
    Serve(#[from] ServeError),
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// initialize a new project
    New(New),

    /// run sql queries
    #[clap(alias = "sql")]
    Query(Query),

    /// render a template
    Render(Render),

    /// run a lua function from app.lua
    Run(Run),

    /// run the web server
    Serve(Serve),
}

impl Command {
    async fn run(self, ctx: AppContext) -> Result<(), CommandError> {
        match self {
            Command::New(new) => {
                new.run(ctx).await?;
                Ok(())
            }
            Command::Query(query) => {
                let database = ctx.database().await?;
                query.run(database).await?;
                Ok(())
            }
            Command::Render(render) => {
                let templates = Template::new(ctx.templates_dir())?;
                render.run(templates).await?;
                Ok(())
            }
            Command::Run(run) => {
                let runtime = ctx.runtime().await?;
                run.run(runtime).await?;

                Ok(())
            }
            Command::Serve(serve) => {
                serve.run(ctx).await?;
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub runtime: Runtime,

    #[allow(unused)]
    pub reloaders: Arc<Reloaders>,

}

#[derive(Debug, thiserror::Error)]
pub enum AppStateError {
    #[error("runtime init error: {0}")]
    Runtime(#[from] RuntimeInitError),

    #[error("watcher error: {0}")]
    Watcher(#[from] notify::Error),
}

#[derive(Debug)]
pub struct AppContext {
    pub root: PathBuf,
}

impl AppContext {
    pub async fn runtime(&self) -> Result<Runtime, RuntimeInitError> {
        let runtime = Runtime::builder()
            .root(&self.root)
            .database(self.database().await?)
            .template(self.template()?)
            .build();

        runtime.init()?;

        Ok(runtime)
    }

    pub async fn state(self) -> Result<AppState, AppStateError> {
        let runtime = self.runtime().await?;
        let template = runtime.template.clone();
        let mut reloaders = Reloaders::new();
        reloaders.add(runtime.clone())?;
        reloaders.add(template.clone())?;

        let reloaders = Arc::new(reloaders);

        Ok(AppState {
            runtime,
            reloaders,
        })
    }

    pub async fn database(&self) -> Result<Database, DatabaseError> {
        Database::open(&self.root).await
    }

    pub fn template(&self) -> Result<Template, minijinja::Error> {
        Template::new(self.templates_dir())
    }

    pub fn templates_dir(&self) -> PathBuf {
        self.root.join("templates")
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.root.join("assets")
    }
}
