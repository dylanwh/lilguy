use minijinja::{path_loader, Environment};
use mlua::prelude::*;
use std::{path::Path, thread};
use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    oneshot,
};

#[derive(Debug, Clone)]
pub struct Template {
    sender: UnboundedSender<Message>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Template(#[from] minijinja::Error),

    #[error("connection closed")]
    ConnectionClosed,
}

type Result<T> = std::result::Result<T, Error>;

type CallFn = Box<dyn FnOnce(&mut Environment<'static>) + Send + 'static>;

enum Message {
    Execute(CallFn),
}

impl Template {
    pub fn new<P>(directory: P) -> Self
    where
        P: AsRef<Path>,
    {
        let mut env = Environment::new();
        env.set_loader(path_loader(directory));

        let (sender, receiver) = unbounded_channel::<Message>();
        thread::spawn(move || event_loop(env, receiver));

        Self { sender }
    }

    pub async fn call<F, R>(&self, function: F) -> Result<R>
    where
        F: FnOnce(&mut Environment) -> Result<R> + 'static + Send,
        R: Send + 'static,
    {
        let (sender, receiver) = oneshot::channel::<Result<R>>();

        self.sender
            .send(Message::Execute(Box::new(move |conn| {
                let value = function(conn);
                let _ = sender.send(value);
            })))
            .map_err(|_| Error::ConnectionClosed)?;

        receiver
            .await
            .map_err(|_| Error::ConnectionClosed)?
    }
}

fn event_loop(mut env: Environment<'static>, mut receiver: UnboundedReceiver<Message>) {
    while let Some(message) = receiver.blocking_recv() {
        match message {
            Message::Execute(f) => f(&mut env),
        }
    }
}

impl LuaUserData for Template {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        // render(name, context)
        methods.add_async_method(
            "render",
            |_, this, (name, context): (String, LuaValue)| async move {
                this.call(move |env| {
                    let template = env.get_template(name.as_str())?;
                    let rendered = template.render(context)?;
                    Ok(rendered)
                })
                .await
                .map_err(|e| mlua::Error::external(e))
            },
        );
    }
}
