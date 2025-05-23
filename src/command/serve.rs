use axum::{
    body::Body,
    extract::{self, ws::WebSocket, Request, State, WebSocketUpgrade},
    http::{Response, StatusCode},
    response::IntoResponse,
    routing::any,
    Router,
};
use bytes::Bytes;
use clap::Parser;
use eyre::Result;
use mlua::prelude::*;
use std::{path::PathBuf, time::Duration};
use tokio::{net::TcpListener, time::sleep};
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use tower_http::{
    services::ServeDir,
    timeout::TimeoutLayer,
    trace::{self, TraceLayer},
};
use tracing::Level;

use crate::{
    command::Config,
    repl,
    routes::Routes,
    runtime::{
        http::{create_request, new_response, LuaCookieJar, LuaHeaders, LuaWebSocket},
        Runtime,
    },
    Output,
};

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
        tracker: &TaskTracker,
        token: &CancellationToken,
        config: &Config,
        output: &Output,
    ) -> Result<()> {
        let runtime = Runtime::new();
        let listener = TcpListener::bind(&self.listen).await?;
        runtime
            .start(tracker, token, &self.app, !self.no_reload)
            .await?;

        let assets_dir = self.app.with_file_name("assets");

        let app = Router::new()
            .nest_service("/assets", ServeDir::new(assets_dir))
            .route("/ws/{*path}", any(handle_websocket_request))
            .route("/ws", any(handle_websocket_request))
            .route("/", any(handle_request))
            .route("/{*path}", any(handle_request))
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
        sleep(Duration::from_secs(1)).await;
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
    #[error("lilguy error: {0}")]
    Runtime(#[from] eyre::Report),

    #[error("lua error: {0}")]
    Lua(#[from] LuaError),
}

impl IntoResponse for LuaServeError {
    fn into_response(self) -> Response<Body> {
        tracing::error!(?self, "error handling request");
        Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::from(format!("error in lua serve function: {self}")))
            .expect("could not create response")
    }
}

async fn handle_request(
    State(runtime): State<Runtime>,
    request: Request<Body>,
) -> Result<LuaResponse, LuaServeError> {
    let lua = runtime.lua()?;
    let globals = lua.globals();
    let routes = globals.get::<LuaUserDataRef<Routes>>("routes")?;
    let (handler, path) = routes.find(request.uri().path());
    let (route, params) = if let Some(ref path) = path {
        (
            LuaValue::String(lua.create_string(path.pattern())?),
            LuaValue::Table(lua.create_table_from(path.params_iter())?),
        )
    } else {
        (LuaValue::Nil, LuaValue::Table(lua.create_table()?))
    };
    drop(path);
    let req = create_request(&lua, request).await?;
    req.set("route", route)?;
    req.set("params", params)?;

    let res = new_response(&lua)?;
    res.set("cookie_jar", req.get::<LuaAnyUserData>("cookie_jar")?)?;

    handler.call_async::<()>((req, &res)).await?;

    Ok(LuaResponse { res })
}

async fn handle_websocket_request(
    extract::Path(path): extract::Path<String>,
    ws: WebSocketUpgrade,
    State(runtime): State<Runtime>,
) -> Response<Body> {
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_websocket(socket, path, runtime).await {
            tracing::error!(?e, "error handling websocket");
        }
    })
}

async fn handle_websocket(socket: WebSocket, path: String, runtime: Runtime) -> Result<()> {
    let lua = runtime.lua()?;

    let globals = lua.globals();
    if let Some(on_ws_connect) = globals.get::<Option<LuaFunction>>("on_ws_connect")? {
        on_ws_connect
            .call_async::<()>((LuaWebSocket::new(socket), path))
            .await?;
    } else {
        tracing::error!("no on_ws_connect function defined");
    }

    Ok(())
}

#[derive(Debug, Clone)]
pub struct LuaResponse {
    res: LuaTable,
}

impl IntoResponse for LuaResponse {
    fn into_response(self) -> Response<Body> {
        let status = self.res.get::<u16>("status").unwrap_or(200);
        let mut headers = self
            .res
            .get::<LuaAnyUserData>("headers")
            .and_then(|headers| headers.take::<LuaHeaders>())
            .map(|headers| headers.into_inner())
            .ok()
            .unwrap_or_default();
        let cookie_jar = self
            .res
            .get::<LuaAnyUserData>("cookie_jar")
            .and_then(|cookies| cookies.take::<LuaCookieJar>());
        if let Ok(cookie_jar) = cookie_jar {
            for cookie in cookie_jar.jar().delta() {
                let Ok(value) = cookie.to_string().parse() else {
                    continue;
                };
                headers.append("set-cookie", value);
            }
        }
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
                    .expect("could not create response")
            })
    }
}
