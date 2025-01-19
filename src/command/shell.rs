use eyre::Result;

use crate::{repl::Repl, runtime::{Options, Runtime}};

use super::Context;

#[derive(Debug, clap::Parser)]
pub struct Shell {}

impl Shell {
    #[tracing::instrument(level = "debug")]
    pub async fn run(self, context: &Context, runtime: Runtime) -> Result<()> {
        runtime.start(Options { reload: false }).await?;
        let repl = Repl {
            token: context.token.clone(),
            tracker: context.tracker.clone(),
            lua: runtime.lua()?,
            config: context.config.clone(),
            output: context.output.clone(),
        };
        repl.start().await?;
        Ok(())
    }
}
