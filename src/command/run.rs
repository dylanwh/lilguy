use std::path::PathBuf;

use clap::Parser;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

pub use crate::runtime::Error;
use crate::runtime::Runtime;

#[derive(Debug, Parser)]
pub struct Run {
    #[clap(short, long, default_value = "app.lua")]
    pub app: PathBuf,

    /// function to call
    #[clap(default_value = "main")]
    pub func: String,

    /// additional arguments to pass to the script
    pub args: Vec<String>,
}
impl Run {
    #[tracing::instrument(level = "debug")]
    pub async fn run(self, token: &CancellationToken, tracker: &TaskTracker) -> Result<(), Error> {
        let runtime = Runtime::new();
        runtime.start(token, tracker, &self.app, false).await?;
        runtime.run(self.func, self.args).await?;

        Ok(())
    }
}
