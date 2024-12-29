use std::{path::PathBuf, sync::Arc};

use minijinja::{path_loader, Environment};
use parking_lot::RwLock;

use crate::reload::Reload;

#[derive(Debug, Clone)]
pub struct Template {
    dir: PathBuf,
    env: Arc<RwLock<Environment<'static>>>,
}

impl Template {
    pub fn new<P>(path: P) -> Result<Self, minijinja::Error>
    where
        P: AsRef<std::path::Path>,
    {
        let mut environment = Environment::new();
        environment.set_loader(path_loader(path.as_ref()));
        Ok(Self {
            dir: path.as_ref().to_path_buf(),
            env: Arc::new(RwLock::new(environment)),
        })
    }

    pub fn render<S>(&self, name: &str, context: S) -> Result<String, minijinja::Error>
    where
        S: serde::Serialize,
    {
        let env = self.env.read();
        let template = env.get_template(name)?;
        let rendered = template.render(context)?;
        Ok(rendered)
    }
}

impl Reload for Template {
    fn name(&self) -> &'static str {
        "template"
    }

    fn reload(&self, _: Vec<PathBuf>) {
        println!("reloading templates");
        self.env.write().clear_templates();
    }

    fn files(&self) -> Vec<(PathBuf, notify::RecursiveMode)> {
        vec![(
            self.dir.clone(),
            notify::RecursiveMode::Recursive,
        )]
    }
}
