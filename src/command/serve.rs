use axum::{
    body::Body,
    extract::{Request, State},
    http::{Response, StatusCode},
    response::IntoResponse,
    routing::any,
    Router,
};
use bytes::Bytes;
use clap::Parser;
use cookie::Key;
use eyre::{Context, ContextCompat, Result};
use mlua::prelude::*;
use std::{path::PathBuf, time::Duration};
use tokio::{io::AsyncWriteExt, net::TcpListener};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tower_http::trace::{self, TraceLayer};
use tower_http::{services::ServeDir, timeout::TimeoutLayer};
use tracing::Level;

use crate::{
    repl, routes::Routes, runtime::{
        self,
        http::{create_request, new_response, LuaCookieSecret, LuaCookies, LuaHeaders},
        Runtime,
    }, Output
};

use super::Config;

#[derive(Debug, Parser)]
pub struct Serve {
    /// the directory to serve files from
    #[clap(short, long, default_value = "app.lua")]
    pub app: PathBuf,

    /// the address to bind to
    #[clap(short, long, default_value = "0.0.0.0:8000")]
    pub listen: String,

    /// do not reload the server when files change
    #[clap(long)]
    pub no_reload: bool,

    #[clap(long)]
    pub silent: bool,

    #[clap(short, long)]
    pub open: bool,

    #[clap(short, long)]
    pub interactive: bool,
    // todo: --secure option that will take a certifcate bundle or use acme to get a certificate
}

impl Serve {
    #[tracing::instrument(level = "debug")]
    pub async fn run(
        self,
        token: &CancellationToken,
        tracker: &TaskTracker,
        config: &Config,
        output: &Output,
    ) -> Result<()> {
        let runtime = Runtime::new();
        let listener = TcpListener::bind(&self.listen).await?;
        runtime
            .start(&self.app, self.no_reload, &token, &tracker)
            .await?;

        let lua = runtime.lua()?;
        let cookie_secret = self.app.with_file_name("cookie_secret");
        let key = if cookie_secret.exists() {
            let key = tokio::fs::read(&cookie_secret).await?;
            Key::try_from(&key[..]).wrap_err("could not read cookie secret")?
        } else {
            let key = Key::try_generate().wrap_err("could not generate cookie secret")?;
            let mut file = tokio::fs::File::create(&cookie_secret).await?;
            file.write_all(key.master().as_ref()).await?;
            key
        };
        lua.set_named_registry_value("COOKIE_SECRET", LuaCookieSecret::new(key))?;

        let assets_dir = self.app.with_file_name("assets");

        let app = Router::new()
            .nest_service("/assets", ServeDir::new(assets_dir))
            .route("/", any(handle_request))
            .route("/*path", any(handle_request))
            .with_state(runtime.clone())
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                    .on_request(trace::DefaultOnRequest::new().level(Level::INFO))
                    .on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
            )
            .layer(TimeoutLayer::new(Duration::from_secs(60)));

        tracker.spawn({
            let token = token.clone();
            async move {
                let server = axum::serve(listener, app).with_graceful_shutdown(async move {
                    token.cancelled().await;
                });
                if let Err(err) = server.await {
                    tracing::error!(?err, "error serving application");
                }
            }
        });

        // wait a tick to ensure the server is up
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let url = format!("http://{}", self.listen);
        let url = url.replace("http://0.0.0.0", "http://127.0.0.1");

        if !self.silent {
            println!("listening on {url}");
        }

        if self.open {
            open::that(url)?;
        }

        if self.interactive {
            repl::start(token, tracker, config, output, runtime.lua()?).await?;
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
enum LuaServeError {
    #[error("runtime error: {0}")]
    Runtime(#[from] runtime::Error),

    #[error("lua error: {0}")]
    Lua(#[from] LuaError),

    #[error("http status: {0}")]
    Status(StatusCode),
}

impl IntoResponse for LuaServeError {
    fn into_response(self) -> Response<Body> {
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(format!("error in lua serve function: {self}")))
            .unwrap()
    }
}

async fn handle_request(
    State(runtime): State<Runtime>,
    request: Request<Body>,
) -> Result<LuaResponse, LuaServeError> {
    let lua = runtime.lua()?;
    let globals = lua.globals();

    let routes = globals.get::<LuaUserDataRef<Routes>>("routes")?;
    let path = request.uri().path().to_owned();
    let req = create_request(&lua, request).await?;
    let res = new_response(&lua)?;

    match routes.find(&path) {
        Some((handler, path)) => {
            req.set("route", path.pattern())?;
            req.set("params", lua.create_table_from(path.params())?)?;
            handler.call_async::<()>((req, &res)).await?;
        }
        None => {
            let Some(not_found) = globals.get::<Option<LuaFunction>>("not_found")? else {
                return Err(LuaServeError::Status(StatusCode::NOT_FOUND));
            };
            not_found.call_async::<()>((req, &res)).await?;
        }
    };

    Ok(LuaResponse { res })
}

#[derive(Debug, Clone)]
pub struct LuaResponse {
    // cookies: LuaAnyUserData,
    res: LuaTable,
}

impl IntoResponse for LuaResponse {
    fn into_response(self) -> Response<Body> {
        let status = self.res.get::<u16>("status").unwrap_or(200);
        let headers = self
            .res
            .get::<LuaAnyUserData>("headers")
            .and_then(|headers| headers.take::<LuaHeaders>())
            .map(|headers| headers.into_inner())
            .ok()
            .unwrap_or_default();
        // let cookies = self.cookies.take::<LuaCookies>().ok();
        // if let Some(cookies) = cookies {
        //     for cookie in cookies.jar.delta() {
        //         let Ok(value) = cookie.to_string().parse() else {
        //             continue;
        //         };
        //         headers.append("set-cookie", value);
        //     }
        // }
        self.res
            .get::<LuaString>("body")
            .map(|body| Bytes::from(body.as_bytes().to_vec()))
            .map(|body| {
                let mut response: Response<Body> = Response::new(body.into());
                *response.headers_mut() = headers;
                *response.status_mut() =
                    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

                response
            })
            .unwrap_or_else(|err| {
                tracing::error!(?err, "error creating response body");
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Body::empty())
                    .unwrap()
            })
    }
}
