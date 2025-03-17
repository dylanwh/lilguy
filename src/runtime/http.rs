use axum::{
    body::{to_bytes, Body},
    http::{HeaderMap, HeaderName, HeaderValue},
};
use bytes::Bytes;
use cookie::{Cookie, CookieJar, Key};
use http::{header::ToStrError, Request};
use mlua::prelude::*;
use parking_lot::Mutex;
use reqwest::{Client, Method, RequestBuilder};
use rusqlite::OptionalExtension;
use std::{ops::Deref, sync::Arc};

use crate::database::Database;

const FETCH_CLIENT: &str = "fetch_client";
const REQUEST_MT: &str = "request_mt";
const RESPONSE_MT: &str = "response_mt";
const COOKIE_KEY: &str = "cookie_key";

pub fn register(lua: &Lua) -> LuaResult<()> {
    let globals = lua.globals();

    let client = Client::builder()
        .user_agent(format!("lilguy/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(LuaError::external)?;
    let fetch_client = FetchClient::from(client);
    lua.set_named_registry_value(FETCH_CLIENT, fetch_client)?;

    let request_mt = lua.create_table()?;
    request_mt.set("__index", globals.get::<Option<LuaTable>>("Request")?)?;

    let response_mt = lua.create_table()?;
    response_mt.set("__index", globals.get::<Option<LuaTable>>("Response")?)?;

    lua.set_named_registry_value(REQUEST_MT, request_mt)?;
    lua.set_named_registry_value(RESPONSE_MT, response_mt)?;

    globals.set("fetch", lua.create_async_function(fetch)?)?;

    Ok(())
}

pub async fn set_cookie_key(lua: &Lua, db: &Database) -> LuaResult<()> {
    let key = db
        .call(|conn| {
            let txn = conn.transaction()?;
            let key: Option<Vec<u8>> = txn
                .query_row(
                    "SELECT value FROM lg_internal WHERE name = 'cookie_key'",
                    [],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(key) = key {
                Ok(Key::derive_from(&key))
            } else {
                let key = Key::try_generate().unwrap();
                txn.execute(
                    "INSERT INTO lg_internal (name, value) VALUES ('cookie_key', ?)",
                    [key.master()],
                )?;
                txn.commit()?;
                Ok(key)
            }
        })
        .await
        .map_err(LuaError::external)?;

    lua.set_named_registry_value(COOKIE_KEY, LuaCookieKey(key))?;

    Ok(())
}

#[derive(Debug, Default)]
pub struct LuaHeaders(HeaderMap);

impl LuaHeaders {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_inner(self) -> HeaderMap {
        self.0
    }
}

impl LuaUserData for LuaHeaders {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(LuaMetaMethod::Index, |_lua, this, key: String| {
            let key = HeaderName::from_bytes(key.as_bytes())
                .map_err(|_| LuaError::external("invalid header name"))?;
            let value = this
                .0
                .get(key)
                .map(|v| v.to_str().unwrap_or(""))
                .unwrap_or("");
            Ok(value.to_string())
        });
        methods.add_meta_method_mut(
            LuaMetaMethod::NewIndex,
            |_lua, this, (key, value): (String, String)| {
                let key = HeaderName::from_bytes(key.as_bytes())
                    .map_err(|_| LuaError::external("invalid header name"))?;
                this.0.append(
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

pub struct LuaCookieJar {
    key: Key,
    jar: Arc<Mutex<CookieJar>>,
    secure: bool,
}

impl LuaCookieJar {
    pub fn new(key: Key, headers: &HeaderMap<HeaderValue>) -> Result<Self, LuaCookieJarError> {
        let mut jar = CookieJar::new();
        for cookie in headers.get_all("cookie") {
            let cookie = cookie.to_str()?.to_owned();
            let cookie = Cookie::parse(cookie)?;
            jar.add_original(cookie);
        }
        let jar = Mutex::new(jar);
        let jar = Arc::new(jar);

        Ok(Self {
            key,
            jar,
            secure: false,
        })
    }

    pub fn jar(&self) -> parking_lot::ArcMutexGuard<parking_lot::RawMutex, cookie::CookieJar> {
        self.jar.lock_arc()
    }
}

pub struct LuaCookieKey(pub Key);

impl LuaCookieKey {
    pub fn key(&self) -> Key {
        self.0.clone()
    }
}

impl LuaUserData for LuaCookieKey {}

#[derive(Debug, thiserror::Error)]
pub enum LuaCookieJarError {
    #[error("invalid cookie")]
    InvalidCookie(#[from] cookie::ParseError),

    #[error("invalid header value")]
    InvalidHeaderValue(#[from] ToStrError),
}

impl LuaUserData for LuaCookieJar {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("get", |_, this, name: String| {
            let jar = this.jar.lock();
            let cookie = jar.get(&name).map(|c| c.value().to_string());
            Ok(cookie)
        });
        methods.add_method("get_signed", |_, this, name: String| {
            let jar = this.jar.lock();
            let cookie = jar
                .signed(&this.key)
                .get(&name)
                .map(|c| c.value().to_string());
            Ok(cookie)
        });
        methods.add_method("get_private", |_, this, name: String| {
            let jar = this.jar.lock();
            let cookie = jar
                .private(&this.key)
                .get(&name)
                .map(|c| c.value().to_string());
            Ok(cookie)
        });
        methods.add_method("set", |_, this, (name, value): (String, Option<String>)| {
            let cookie = match value {
                Some(value) => Cookie::build((name, value))
                    .same_site(cookie::SameSite::Lax)
                    .path("/")
                    .permanent()
                    .http_only(true)
                    .secure(this.secure)
                    .build(),
                None => Cookie::build(name)
                    .same_site(cookie::SameSite::Lax)
                    .path("/")
                    .permanent()
                    .http_only(true)
                    .secure(this.secure)
                    .removal()
                    .build(),
            };
            let mut jar = this.jar.lock();
            jar.add(cookie);
            Ok(())
        });

        methods.add_method(
            "set_signed",
            |_, this, (name, value): (String, Option<String>)| {
                let cookie = match value {
                    Some(value) => Cookie::build((name, value))
                        .same_site(cookie::SameSite::Lax)
                        .path("/")
                        .permanent()
                        .http_only(true)
                        .secure(this.secure)
                        .build(),
                    None => Cookie::build(name)
                        .same_site(cookie::SameSite::Lax)
                        .path("/")
                        .permanent()
                        .http_only(true)
                        .secure(this.secure)
                        .removal()
                        .build(),
                };
                let mut jar = this.jar.lock();
                jar.signed_mut(&this.key).add(cookie);
                Ok(())
            },
        );

        methods.add_method(
            "set_private",
            |_, this, (name, value): (String, Option<String>)| {
                let cookie = match value {
                    Some(value) => Cookie::build((name, value))
                        .same_site(cookie::SameSite::Lax)
                        .path("/")
                        .permanent()
                        .http_only(true)
                        .secure(this.secure)
                        .build(),
                    None => Cookie::build(name)
                        .same_site(cookie::SameSite::Lax)
                        .path("/")
                        .permanent()
                        .http_only(true)
                        .secure(this.secure)
                        .removal()
                        .build(),
                };
                let mut jar = this.jar.lock();
                jar.private_mut(&this.key).add(cookie);
                Ok(())
            },
        );
    }
}

/// fetch(url [, options])
///
/// this is intended to be largely compatible with fetch() in the browser supporting:
/// - method: GET, POST, PUT, DELETE, etc
/// - headers: { ["Content-Type"] = "application/json" }
/// - body: string or someething with __tostring
#[allow(unused)]
async fn fetch(lua: Lua, (url, options): (String, Option<LuaTable>)) -> LuaResult<LuaTable> {
    let client = lua.named_registry_value::<LuaUserDataRef<FetchClient>>("fetch_client")?;
    let mut request: RequestBuilder = match options {
        Some(options) => {
            let method = options
                .get::<Option<String>>("method")?
                .unwrap_or("get".to_string());
            let method = Method::from_bytes(method.as_bytes()).map_err(LuaError::external)?;
            let mut request = client.request(method, &url);
            if let Some(headers) = options.get::<Option<LuaTable>>("headers")? {
                let headers = headers
                    .pairs::<String, String>()
                    .map(|(pair)| {
                        let (key, value) = pair?;
                        Ok((
                            HeaderName::from_bytes(key.as_bytes()).map_err(LuaError::external)?,
                            HeaderValue::from_str(&value).map_err(LuaError::external)?,
                        ))
                    })
                    .collect::<LuaResult<HeaderMap>>()?;
                request = request.headers(headers);
            }
            if let Some(body) = options.get::<Option<String>>("body")? {
                request = request.body(body);
            }
            request
        }
        None => client.get(&url),
    };
    let response = request.send().await.map_err(LuaError::external)?;
    let res = create_fetch_response(&lua, response).await?;

    Ok(res)
}

pub async fn create_request(lua: &Lua, request: Request<Body>) -> Result<LuaTable, LuaError> {
    let (parts, body) = request.into_parts();
    let req = lua.create_table()?;
    let method = parts.method.as_str();
    let content_type = parts
        .headers
        .get("content-type")
        .map(|v| v.to_str().unwrap_or(""))
        .unwrap_or("")
        .to_owned();

    let key = lua
        .named_registry_value::<LuaUserDataRef<LuaCookieKey>>(COOKIE_KEY)?
        .key();
    let cookie_jar =
        lua.create_userdata(LuaCookieJar::new(key, &parts.headers).map_err(LuaError::external)?)?;
    let headers = lua.create_userdata(LuaHeaders(parts.headers))?;
    let body = to_bytes(body, 1024 * 1024 * 16)
        .await
        .map_err(LuaError::external)?;

    req.set("method", method)?;
    req.set("headers", headers)?;
    req.set("path", parts.uri.path())?;
    let query: serde_json::Map<String, serde_json::Value> =
        serde_qs::from_str(parts.uri.query().unwrap_or("")).map_err(LuaError::external)?;
    req.set("query", lua.to_value(&query)?)?;
    req.set("cookie_jar", &cookie_jar)?;

    match content_type.as_str() {
        "application/x-www-form-urlencoded" => {
            let body: serde_json::Value =
                serde_urlencoded::from_bytes(&body).map_err(LuaError::external)?;
            req.set("body", lua.to_value(&body)?)
        }
        _ => req.set("body", lua.create_string(&body)?),
    }?;

    req.set_metatable(lua.named_registry_value::<LuaTable>(REQUEST_MT)?.into());

    Ok(req)
}

pub fn new_response(lua: &Lua) -> Result<LuaTable, LuaError> {
    let res = lua.create_table()?;
    res.set("status", 200)?;
    res.set("headers", lua.create_userdata(LuaHeaders::new())?)?;
    res.set("body", "")?;
    res.set_metatable(lua.named_registry_value::<LuaTable>(RESPONSE_MT)?.into());
    Ok(res)
}

async fn create_fetch_response(
    lua: &Lua,
    response: reqwest::Response,
) -> Result<LuaTable, LuaError> {
    let response = axum::http::Response::from(response);
    let (parts, body) = response.into_parts();
    let body = Body::from(Bytes::copy_from_slice(body.as_bytes().unwrap_or_default()));
    let response = axum::http::Response::from_parts(parts, body);

    create_response(lua, response).await
}

pub async fn create_response(
    lua: &Lua,
    response: axum::http::Response<Body>,
) -> Result<LuaTable, LuaError> {
    let (parts, body) = response.into_parts();
    let res = lua.create_table()?;
    let status = parts.status.as_u16();
    let headers = lua.create_userdata(LuaHeaders(parts.headers))?;
    let body = to_bytes(body, 1024 * 1024 * 16)
        .await
        .map_err(LuaError::external)?;

    res.set("status", status)?;
    res.set("headers", headers)?;
    res.set("body", lua.create_string(&body)?)?;
    res.set_metatable(lua.named_registry_value::<LuaTable>(RESPONSE_MT)?.into());

    Ok(res)
}

// default not found handler - usually overridden by the user
pub fn not_found(_: &Lua, (_, res): (LuaTable, LuaTable)) -> LuaResult<()> {
    res.set("status", 404)?;
    Ok(())
}

pub struct FetchClient(Client);

impl From<Client> for FetchClient {
    fn from(client: Client) -> Self {
        Self(client)
    }
}

impl Deref for FetchClient {
    type Target = Client;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl LuaUserData for FetchClient {}
