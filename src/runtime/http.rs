use std::{char::MAX, ops::Deref};

use axum::{
    body::{to_bytes, Body},
    http::{HeaderMap, HeaderName, HeaderValue, Response},
    response::IntoResponse,
};
use bytes::Bytes;
use http::{version, Request};
use mlua::prelude::*;
use reqwest::{Method, RequestBuilder, StatusCode};
use tokio_util::codec::Decoder;

pub fn register(lua: &Lua) -> Result<(), super::Error> {
    let client = reqwest::Client::builder()
        .user_agent(format!("lilguy/{}", env!("CARGO_PKG_VERSION")))
        .build()?;
    let fetch_client = FetchClient::from(client);
    lua.set_named_registry_value("fetch_client", fetch_client)?;

    let globals = lua.globals();
    globals.set(
        "fetch_client",
        lua.named_registry_value::<LuaValue>("fetch_client")?,
    )?;
    globals.set("fetch", lua.create_async_function(fetch)?)?;

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
    let headers = lua.create_userdata(LuaHeaders(parts.headers))?;
    let body = to_bytes(body, 1024 * 1024 * 16)
        .await
        .map_err(LuaError::external)?;

    req.set("method", method)?;
    req.set("headers", headers)?;
    req.set("body", lua.create_string(&body)?)?;

    // set request metatable that prevents new fields or modifying the headers field
    // req.set_metatable(lua.named_registry_value::<LuaTable>("request_mt")?.into());

    Ok(req)
}

pub fn new_response(lua: &Lua) -> Result<LuaTable, LuaError> {
    let res = lua.create_table()?;
    let headers = lua.create_userdata(LuaHeaders::new())?;
    res.set("status", 200)?;
    res.set("headers", headers)?;
    res.set("body", "")?;
    Ok(res)
}

async fn create_fetch_response(lua: &Lua, response: reqwest::Response) -> Result<LuaTable, LuaError> {
    let response = axum::http::Response::from(response);
    let (parts, body) = response.into_parts();
    let body = Body::from(Bytes::copy_from_slice(body.as_bytes().unwrap_or_default()));
    let response = axum::http::Response::from_parts(parts, body);

    create_response(lua, response).await
}

pub async fn create_response(lua: &Lua, response: axum::http::Response<Body>) -> Result<LuaTable, LuaError> {
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

    // set response metatable that prevents new fields or modifying the headers field
    // res.set_metatable(lua.named_registry_value::<LuaTable>("response_mt")?.into());

    Ok(res)
}

pub struct FetchClient(reqwest::Client);

impl From<reqwest::Client> for FetchClient {
    fn from(client: reqwest::Client) -> Self {
        Self(client)
    }
}

impl Deref for FetchClient {
    type Target = reqwest::Client;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl LuaUserData for FetchClient {}
