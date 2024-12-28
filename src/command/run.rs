use clap::Parser;
use mlua::prelude::LuaError;

use crate::runtime::Runtime;
pub type RunError = LuaError;

#[derive(Debug, Parser)]
pub struct Run {
    /// the name of the script
    pub name: String,

    /// additional arguments to pass to the script
    pub args: Vec<String>,
}

impl Run {
    pub async fn run(self, runtime: Runtime) -> Result<(), RunError> {
        runtime.run(self.name, self.args).await?;

        Ok(())
    }
}
