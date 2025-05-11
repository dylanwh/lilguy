use std::path::PathBuf;

use eyre::Result;
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{repl, runtime::Runtime, Output};

use super::Config;

#[derive(Debug, clap::Parser)]
pub struct Shell {
    /// the path to the Lua script to run
    #[clap(short, long, default_value = "app.lua")]
    pub app: PathBuf,

    /// reload files when they change
    #[clap(long, default_value = "false")]
    pub no_reload: bool,
}

impl Shell {
    #[tracing::instrument(level = "debug")]
    pub async fn run(
        self,
        tracker: &TaskTracker,
        token: &CancellationToken,
        config: &Config,
        output: &Output,
    ) -> Result<()> {
        let runtime = Runtime::new();
        runtime
            .start(tracker, token, &self.app, !self.no_reload)
            .await?;
        repl::start(token, tracker, config, output, runtime.lua()?).await?;
        Ok(())
    }
}
