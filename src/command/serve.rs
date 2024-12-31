use std::{
    collections::HashMap,
    ops::Deref,
    sync::{Arc, Weak},
};

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header::CONTENT_TYPE, HeaderName, StatusCode},
    response::{IntoResponse, Response},
    routing::any,
    Router,
};
use bytes::BytesMut;
use clap::Parser;
use mlua::prelude::*;
use parking_lot::Mutex;
use tokio::net::TcpListener;
use tower_http::services::ServeDir;
use tower_http::trace::{self, TraceLayer};
use tracing::Level;

use crate::{routes::Routes, runtime::{self, Runtime}, template::Template};

#[derive(Debug, Parser)]
pub struct Serve {
    /// the address to bind to
    #[clap(short, long, default_value = "0.0.0.0:8000")]
    pub listen: String,
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
        runtime.start().await?;

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

struct LuaRequest {
    /// the route key, or "pattern" e.g. "/users/:id"
    route: Option<String>,
    /// the parameters extracted from the route, e.g. { id = "1" }
    params: HashMap<String, String>,

    req: Request<Body>,
}

impl LuaRequest {
    fn new(req: Request<Body>, path: Option<path_tree::Path>) -> Self {
        let route = path.as_ref().map(|p| p.pattern().to_string());
        let params = path
            .map(|p| {
                p.params()
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        Self { route, params, req }
    }
}

impl Deref for LuaRequest {
    type Target = Request<Body>;

    fn deref(&self) -> &Self::Target {
        &self.req
    }
}

#[derive(Debug, Clone, Default)]
struct LuaResponse {
    inner: Arc<Mutex<Response<BytesMut>>>,
}

#[derive(Debug, Clone)]
struct LuaResponseHeaders {
    inner: Weak<Mutex<Response<BytesMut>>>,
}

impl LuaUserData for LuaRequest {}

impl LuaUserData for LuaResponse {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("status", |_, this| {
            let inner = this.inner.lock();
            Ok(inner.status().as_u16())
        });
        fields.add_field_method_set("status", |_, this, status: u16| {
            let mut inner = this.inner.lock();
            *inner.status_mut() = StatusCode::from_u16(status).map_err(LuaError::external)?;
            Ok(())
        });

        fields.add_field_method_set("headers", |_, this, new_headers: LuaTable| {
            let mut inner = this.inner.lock();
            let headers = inner.headers_mut();
            headers.clear();
            new_headers.for_each(|key: String, value: String| {
                headers.append(
                    HeaderName::from_bytes(key.as_bytes())
                        .map_err(|_| LuaError::external("invalid header name"))?,
                    value
                        .parse()
                        .map_err(|_| LuaError::external("invalid header value"))?,
                );
                Ok(())
            })?;
            Ok(())
        });
        fields.add_field_method_get("headers", |_, this| {
            Ok(LuaResponseHeaders {
                inner: Arc::downgrade(&this.inner),
            })
        });
        fields.add_field_method_set("body", |_, this, text: String| {
            let mut inner = this.inner.lock();
            *inner.body_mut() = BytesMut::from(text.as_bytes());
            Ok(())
        });
    }

    // fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
    // }
}

impl LuaUserData for LuaResponseHeaders {
    // __index, __newindex metamethods

    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(LuaMetaMethod::Index, |_lua, this, key: String| {
            let inner = this
                .inner
                .upgrade()
                .ok_or_else(|| LuaError::external("response has been dropped"))?;
            let inner = inner.lock();
            let key = HeaderName::from_bytes(key.as_bytes())
                .map_err(|_| LuaError::external("invalid header name"))?;
            let value = inner
                .headers()
                .get(key)
                .map(|v| v.to_str().unwrap_or(""))
                .unwrap_or("");
            Ok(value.to_string())
        });
        methods.add_meta_method(
            LuaMetaMethod::NewIndex,
            |_lua, this, (key, value): (String, String)| {
                let inner = this
                    .inner
                    .upgrade()
                    .ok_or_else(|| LuaError::external("response has been dropped"))?;
                let mut inner = inner.lock();
                let key = HeaderName::from_bytes(key.as_bytes())
                    .map_err(|_| LuaError::external("invalid header name"))?;
                inner.headers_mut().append(
                    key,
                    value
                        .parse()
                        .map_err(|_| LuaError::external("invalid header value"))?,
                );
                Ok(())
            },
        );
    }
}

impl IntoResponse for LuaResponse {
    fn into_response(self) -> Response<Body> {
        let inner = match Arc::try_unwrap(self.inner) {
            Ok(inner) => inner.into_inner(),
            Err(outer) => {
                tracing::warn!("lua response had to be cloned because of existing references");
                outer.lock().clone()
            }
        };
        inner.map(|b| Body::from(b.freeze()))
    }
}

async fn handle_request(
    // request
    State(runtime): State<Runtime>,
    request: Request<Body>,
) -> Result<LuaResponse, LuaServeError> {
    let lua = runtime.lua()?;
    let globals = lua.globals();

    let routes = globals.get::<LuaUserDataRef<Routes>>("routes")?;
    let path = request.uri().path().to_owned();
    let res = LuaResponse::default();
    match routes.find(&path) {
        Some((handler, path)) => {
            let req = LuaRequest::new(request, Some(path));
            handler.call_async::<()>((req, res.clone())).await?;
        }
        None => {
            let Some(not_found) = globals.get::<Option<LuaFunction>>("not_found")? else {
                return Err(LuaServeError::Status(StatusCode::NOT_FOUND));
            };
            let req = LuaRequest::new(request, None);
            not_found.call_async::<()>((req, res.clone())).await?;
        }
    };

    Ok(res)
}
