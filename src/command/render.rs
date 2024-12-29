use std::{collections::HashMap, path::PathBuf, str::FromStr};

use clap::Parser;

use crate::template::Template;

#[derive(Debug, Parser)]
pub struct Render {
    /// the name of the template
    pub file: String,

    /// the output file, defaults to stdout if not provided
    #[clap(short, long)]
    pub output: Option<PathBuf>,

    /// additional variables to pass to the template
    #[clap(short = 'D', long = "define", value_name = "KEY=VALUE")]
    pub defines: Vec<Define>,
}

#[derive(Debug, Clone)]
pub struct Define(String, String);

impl FromStr for Define {
    type Err = eyre::Report;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.split_once('=') {
            None => Err(eyre::eyre!("invalid define")),
            Some((key, value)) => Ok(Self(key.to_string(), value.to_string())),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("template error: {0}")]
    Template(#[from] minijinja::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl Render {
    pub async fn run(self, template: Template) -> Result<(), RenderError> {
        let defines: HashMap<_, _> = self.defines.iter().map(|d| (&d.0, &d.1)).collect();
        let rendered = template.render(&self.file, &defines)?;

        match self.output {
            Some(path) => {
                let mut file = tokio::fs::File::create(path).await?;
                tokio::io::AsyncWriteExt::write_all(&mut file, rendered.as_bytes()).await?;
            }
            None => {
                println!("{}", rendered);
            }
        }

        Ok(())
    }
}
