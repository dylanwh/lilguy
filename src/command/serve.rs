use std::{
    any::Any,
    collections::HashMap,
    io::Read,
    ops::Deref,
    sync::{Arc, Weak},
};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{Response, StatusCode},
    response::IntoResponse,
    routing::any,
    Router,
};
use bytes::{Bytes, BytesMut};
use clap::Parser;
use mlua::prelude::*;
use tokio::net::TcpListener;
use tower_http::services::ServeDir;
use tower_http::trace::{self, TraceLayer};
use tracing::Level;

use crate::{
    routes::Routes,
    runtime::{
        self,
        http::{create_request, new_response, LuaHeaders},
        Options, Runtime,
    },
};

#[derive(Debug, Parser)]
pub struct Serve {
    /// the address to bind to
    #[clap(short, long, default_value = "0.0.0.0:8000")]
    pub listen: String,

    /// do not reload the server when files change
    #[clap(long)]
    pub no_reload: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("runtime error: {0}")]
    Runtime(#[from] runtime::Error),
}

impl Serve {
    pub async fn run(self, runtime: Runtime) -> Result<(), Error> {
        let listener = TcpListener::bind(&self.listen).await?;
        runtime
            .start(Options {
                reload: !self.no_reload,
            })
            .await?;

        let assets_dir = runtime.assets_dir();

        let app = Router::new()
            .nest_service("/assets", ServeDir::new(assets_dir))
            .route("/", any(handle_request))
            .route("/*path", any(handle_request))
            .with_state(runtime)
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                    .on_request(trace::DefaultOnRequest::new().level(Level::INFO))
                    .on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
            );

        axum::serve(listener, app).await?;

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

    Ok(LuaResponse::new(res))
}

#[derive(Debug, Clone)]
pub struct LuaResponse {
    table: LuaTable,
}

impl LuaResponse {
    pub fn new(table: LuaTable) -> Self {
        Self { table }
    }
}

impl IntoResponse for LuaResponse {
    fn into_response(self) -> Response<Body> {
        let status = self.table.get::<u16>("status").unwrap_or(200);
        let headers = self
            .table
            .get::<LuaAnyUserData>("headers")
            .and_then(|headers| headers.take::<LuaHeaders>())
            .map(|headers| headers.into_inner())
            .ok();
        self
            .table
            .get::<LuaString>("body")
            .map(|body| Bytes::from(body.as_bytes().to_vec()))
            .map(|body| {
                let mut response: Response<Body> = Response::new(body.into());
                if let Some(headers) = headers {
                    *response.headers_mut() = headers;
                }
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
