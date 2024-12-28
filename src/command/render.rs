use std::{collections::BTreeMap, path::PathBuf, str::FromStr};

use clap::Parser;
use tokio::io::AsyncWriteExt;


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
    pub async fn run<T>(self, template: T) -> Result<(), RenderError>
    where
        T: AsRef<minijinja::Environment<'static>> + Send,
    {
        let defines = BTreeMap::from_iter(
            self.defines
                .iter()
                .map(|Define(ref k, ref v)| (k.as_str(), v.as_str())),
        );
        let content = template.as_ref().render_str(&self.file, &defines)?;
        if let Some(output) = &self.output {
            tokio::fs::write(output, content).await?;
        } else {
            tokio::io::stdout().write_all(content.as_bytes()).await?;
        }

        Ok(())
    }
}
