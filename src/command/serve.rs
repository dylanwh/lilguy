use axum::{
    body::Body,
    extract::{Request, State},
    http::{header::IntoHeaderName, HeaderMap, HeaderName, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use clap::Parser;
use mlua::prelude::*;
use tokio::net::TcpListener;
use tower_http::services::ServeDir;

use crate::runtime::RuntimeInitError;

use super::{AppContext, AppState};

#[derive(Debug, Parser)]
pub struct Serve {
    /// the address to bind to
    #[clap(short, long, default_value = "0.0.0.0:8000")]
    pub listen: String,
}

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("runtime init error: {0}")]
    Runtime(#[from] RuntimeInitError),
}

impl Serve {
    pub async fn run(self, ctx: AppContext) -> Result<(), ServeError> {
        let listener = TcpListener::bind(&self.listen).await?;

        let app = Router::new()
            .nest_service("/assets", ServeDir::new(ctx.assets_dir()))
            .route("/", any(handle_request))
            .route("/*path", any(handle_request))
            .with_state(ctx.state().await?);

        if let Err(err) = axum::serve(listener, app).await {
            tracing::error!("Error serving web: {err}");
        }

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
#[error("lua error: {source}")]
struct LuaServeError {
    #[from]
    source: LuaError,
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
    // request
    State(state): State<AppState>,
    request: Request<Body>,
) -> Result<Response<Body>, LuaServeError> {
    let lua = state.as_ref();
    let globals = lua.globals();

    let lua_request = lua.create_table()?;
    lua_request.set("method", request.method().as_str())?;
    lua_request.set("path", request.uri().path())?;
    lua_request.set("query", request.uri().query())?;
    lua_request.set("headers", {
        let headers = request.headers();
        let lua_headers = lua.create_table()?;
        for (key, value) in headers {
            let key = key.as_str();
            let value = value
                .to_str()
                .map_err(|_| LuaError::external("invalid header value"))?;
            // if header exists, append to existing value using comma as per RFC 2616
            let value = match lua_headers.get::<Option<String>>(key)? {
                Some(existing) => format!("{existing}, {value}"),
                _ => value.to_string(),
            };
            lua_headers.set(key, value)?;
        }
        lua_headers
    })?;

    let serve = globals.get::<LuaFunction>("serve")?;
    let (lua_status, lua_headers, lua_body) = serve
        .call_async::<(u16, LuaTable, String)>(lua_request)
        .await?;

    let mut response = Response::builder().status(lua_status);
    let headers = response
        .headers_mut()
        .ok_or_else(|| LuaError::external("response headers not set"))?;
    lua_headers.for_each(|key: String, value: String| {
        headers.append(
            HeaderName::from_bytes(key.as_bytes())
                .map_err(|_| LuaError::external(format!("invalid header name: {key}")))?,
            value
                .parse()
                .map_err(|_| LuaError::external(format!("invalid header value: {value}")))?,
        );
        Ok(())
    })?;

    Ok(response
        .body(Body::from(lua_body))
        .map_err(|_| LuaError::external("error creating response body"))?)
}
