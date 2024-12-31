use clap::Parser;

use crate::runtime::Runtime;
pub use crate::runtime::Error;

#[derive(Debug, Parser)]
pub struct Run {
    /// the name of the script
    pub name: String,

    /// additional arguments to pass to the script
    pub args: Vec<String>,
}

impl Run {
    pub async fn run(self, runtime: Runtime) -> Result<(), Error> {
        runtime.run(self.name, self.args).await?;

        Ok(())
    }
}
