use clap::Parser;
use std::io::ErrorKind;

use super::AppContext;

#[derive(Debug, Parser)]
pub struct New {
    /// the name of the project
    pub name: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum NewError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl New {
    pub async fn run(self, AppContext { root, name }: AppContext) -> Result<(), NewError> {
        let name = self.name.unwrap_or(name);
        tokio::fs::create_dir_all(&root).await?;
        // refuse to overwrite existing project
        if root.join(format!("{name}.lua")).exists() {
            return Err(
                std::io::Error::new(ErrorKind::AlreadyExists, "project already exists").into(),
            );
        }
        tokio::fs::write(
            root.join(format!("{name}.lua")),
            b"function hello()\nprint('hello world')\nend\n",
        )
        .await?;
        tokio::fs::create_dir_all(root.join("templates")).await?;

        println!("created new project: {}", name);
        Ok(())
    }
}
