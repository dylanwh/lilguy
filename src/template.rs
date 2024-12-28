use std::sync::Arc;

use minijinja::{path_loader, Environment};

#[derive(Debug, Clone)]
pub struct Template{ env: Arc<Environment<'static>> }

impl Template {
    pub fn new<P>(path: P) -> Result<Self, minijinja::Error>
    where
        P: AsRef<std::path::Path>,
    {
        let mut environment = Environment::new();
        environment.set_loader(path_loader(path));
        Ok(Self{ env: Arc::new(environment) })
    }
}

impl AsRef<Environment<'static>> for Template {
    fn as_ref(&self) -> &Environment<'static> {
        &self.env
    }
}
