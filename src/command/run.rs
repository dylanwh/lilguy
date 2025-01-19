use clap::Parser;

pub use crate::runtime::Error;
use crate::runtime::{Options, Runtime};


#[derive(Debug, Parser)]
pub struct Run {
    /// the name of the script
    pub name: String,

    /// additional arguments to pass to the script
    pub args: Vec<String>,
}
impl Run {
    #[tracing::instrument(level = "debug")]
    pub async fn run(self, runtime: Runtime) -> Result<(), Error> {
        runtime.start(Options { reload: false }).await?;
        runtime.run(self.name, self.args).await?;

        Ok(())
    }
}
