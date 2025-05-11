use std::path::PathBuf;

use clap::Parser;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::runtime::Runtime;

#[derive(Debug, Parser)]
pub struct Run {
    #[clap(short, long, default_value = "app.lua")]
    pub app: PathBuf,

    /// function to call
    #[clap(default_value = "main")]
    pub func: String,

    /// additional arguments to pass to the script
    #[clap(allow_hyphen_values = true, trailing_var_arg = true)]
    pub args: Vec<String>,
}
impl Run {
    #[tracing::instrument(level = "debug")]
    pub async fn run(
        self,
        tracker: &TaskTracker,
        token: &CancellationToken,
    ) -> Result<(), eyre::Report> {
        let runtime = Runtime::new();
        runtime.start(tracker, token, &self.app, false).await?;
        runtime.run(self.func, self.args).await?;

        Ok(())
    }
}
