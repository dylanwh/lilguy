use std::ops::Deref;

use axum::{
    body::{to_bytes, Body},
    http::{HeaderMap, HeaderName, HeaderValue},
};
use bytes::Bytes;
use cookie::{Cookie, CookieJar, Key, SameSite};
use http::Request;
use mlua::prelude::*;
use reqwest::{Client, Method, RequestBuilder};

const FETCH_CLIENT: &str = "fetch_client";
const REQUEST_MT: &str = "request_mt";
const RESPONSE_MT: &str = "response_mt";

pub fn register(lua: &Lua) -> Result<(), super::Error> {
    let globals = lua.globals();

    let client = Client::builder()
        .user_agent(format!("lilguy/{}", env!("CARGO_PKG_VERSION")))
        .build()?;
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

#[derive(Debug)]
pub struct LuaCookies {
    pub jar: CookieJar,
    secure: bool,
}

pub struct LuaCookieSecret(Key);

impl LuaCookieSecret {
    pub fn key(&self) -> &Key {
        &self.0
    }
    
    pub(crate) fn new(key: Key) -> Self {
        Self(key)
    }
}

impl LuaUserData for LuaCookieSecret {}

impl LuaUserData for LuaCookies {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(LuaMetaMethod::Index, |lua, this, key: String| {
            // we only support signed cookies for now, the signing key is static until release
            let cookie_secret: LuaUserDataRef<LuaCookieSecret> =
                lua.named_registry_value("COOKIE_SECRET")?;

            let signed_jar = this.jar.signed(cookie_secret.key());
            let cookie = signed_jar.get(&key).map(|c| c.value().to_string());

            Ok(cookie)
        });

        methods.add_meta_method_mut(
            LuaMetaMethod::NewIndex,
            |lua, this, (key, value): (String, Option<String>)| {
                // we only support signed cookies for now, the signing key is static until release

                let cookie_secret: LuaUserDataRef<LuaCookieSecret> =
                    lua.named_registry_value("COOKIE_SECRET")?;
                let mut signed_jar = this.jar.signed_mut(cookie_secret.key());
                let cookie = if let Some(value) = value {
                    Cookie::build((key, value))
                        .http_only(true)
                        .same_site(SameSite::Strict)
                        .permanent()
                        .secure(this.secure)
                        .build()
                } else {
                    Cookie::build(key)
                        .http_only(true)
                        .same_site(SameSite::Strict)
                        .secure(this.secure)
                        .removal()
                        .build()
                };
                signed_jar.add(cookie);
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

pub async fn create_request(
    lua: &Lua,
    request: Request<Body>,
) -> Result<LuaTable, LuaError> {
    let (parts, body) = request.into_parts();
    let req = lua.create_table()?;
    let method = parts.method.as_str();
    let content_type = parts
        .headers
        .get("content-type")
        .map(|v| v.to_str().unwrap_or(""))
        .unwrap_or("")
        .to_owned();

    // let signing_key = lua
    //     .named_registry_value::<LuaAnyUserData>("COOKIE_SECRET")?
    //     .borrow::<Key>()?;

    // let mut jar = CookieJar::new();
    // for cookie in parts.headers.get_all("cookie") {
    //     let cookie = cookie.to_str().map_err(LuaError::external)?.to_string();
    //     let cookie = Cookie::parse(cookie).map_err(LuaError::external)?;
    //     jar.add_original(cookie);
    // }
    // let cookies = lua.create_userdata(LuaCookies { jar, secure: false })?;
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
    // req.set("cookies", &cookies)?;

    match content_type.as_str() {
        "application/x-www-form-urlencoded" => {
            let body: serde_json::Value =
                serde_urlencoded::from_bytes(&body).map_err(LuaError::external)?;
            req.set("body", lua.to_value(&body)?)
        }
        _ => req.set("body", lua.create_string(&body)?),
    }?;

    req.set_metatable(lua.named_registry_value::<LuaTable>(REQUEST_MT)?.into());

    Ok((req ))
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
