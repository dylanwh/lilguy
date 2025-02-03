use std::ops::Deref;

use axum::{
    body::{to_bytes, Body},
    extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket},
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
    globals.set("cookies", lua.create_function(cookies)?)?;

    lua.set_named_registry_value(
        "COOKIE_SECRET",
        lua.create_userdata(LuaCookieSecret(Key::generate()))?,
    )?;

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
                if let Some(value) = value {
                    let cookie = Cookie::build((key, value))
                        .http_only(true)
                        .same_site(SameSite::Strict)
                        .permanent()
                        .secure(this.secure)
                        .build();
                    signed_jar.add(cookie);
                } else {
                    let cookie = Cookie::build(key)
                        .http_only(true)
                        .same_site(SameSite::Strict)
                        .secure(this.secure)
                        .removal()
                        .build();
                    this.jar.add(cookie);
                };
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

pub fn cookies(_: &Lua, secure: bool) -> Result<LuaCookies, LuaError> {
    let jar = CookieJar::new();
    let cookies = LuaCookies { jar, secure };
    Ok(cookies)
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

    let mut jar = CookieJar::new();
    for cookie in parts.headers.get_all("cookie") {
        let cookie = cookie.to_str().map_err(LuaError::external)?.to_string();
        let cookie = Cookie::parse(cookie).map_err(LuaError::external)?;
        jar.add_original(cookie);
    }
    let cookies = lua.create_userdata(LuaCookies { jar, secure: false })?;
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
    req.set("cookies", &cookies)?;

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

pub struct LuaWebSocket {
    socket: WebSocket,
}

impl LuaWebSocket {
    pub fn new(socket: WebSocket) -> Self {
        Self { socket }
    }
}

fn message_from_lua(value: LuaValue) -> Result<Message, LuaError> {
    match value {
        LuaValue::String(s) => {
            let text: &str = &s.to_str()?;
            Ok(Message::Text(Utf8Bytes::from(text)))
        }
        LuaValue::Table(t) => {
            let r#type = t.get::<String>("type")?;
            match r#type.as_str() {
                "binary" => {
                    let data = t.get::<String>("data")?;
                    Ok(Message::Binary(Bytes::from(data.as_bytes().to_owned())))
                }
                "ping" => {
                    let data = t.get::<String>("data")?;
                    Ok(Message::Ping(Bytes::from(data.as_bytes().to_owned())))
                }
                "pong" => {
                    let data = t.get::<String>("data")?;
                    Ok(Message::Pong(Bytes::from(data.as_bytes().to_owned())))
                }
                "close" => {
                    let code = t.get::<Option<u16>>("code")?;
                    let reason = t.get::<Option<LuaString>>("reason")?;
                    match (code, reason) {
                        (Some(code), Some(reason)) => {
                            let reason: &str = &reason.to_str()?;
                            Ok(Message::Close(Some(CloseFrame {
                                code,
                                reason: Utf8Bytes::from(reason),
                            })))
                        }
                        (Some(_), None) | (None, Some(_)) => {
                            Err(LuaError::external("missing code or reason"))
                        }
                        (None, None) => Ok(Message::Close(None)),
                    }
                }
                _ => Err(LuaError::external("invalid message type")),
            }
        }
        _ => Err(LuaError::external("invalid message type")),
    }
}

macro_rules! lua_message {
    ($lua:ident, $type:expr, $data:expr) => {{
        let table = $lua.create_table()?;
        let data = $lua.create_string(&$data)?;
        table.set("type", $type)?;
        table.set("data", data)?;
        Ok(LuaValue::Table(table))
    }};
}

fn lua_from_message(lua: &Lua, message: Message) -> LuaResult<LuaValue> {
    match message {
        Message::Text(utf8_bytes) => lua.to_value(utf8_bytes.as_str()),
        Message::Binary(bytes) => {
            lua_message!(lua, "binary", bytes)
        }
        Message::Ping(bytes) => {
            lua_message!(lua, "ping", bytes)
        }
        Message::Pong(bytes) => {
            lua_message!(lua, "pong", bytes)
        }
        Message::Close(close_frame) => {
            let table = lua.create_table()?;
            table.set("type", "close")?;
            if let Some(close_frame) = close_frame {
                table.set("code", close_frame.code)?;
                table.set("reason", lua.create_string(close_frame.reason.as_str())?)?;
            }
            Ok(LuaValue::Table(table))
        }
    }
}

impl LuaUserData for LuaWebSocket {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_async_method_mut("send", |_, mut this, message: LuaValue| async move {
            let message = message_from_lua(message)?;
            tracing::trace!(?message, "sending message");
            this.socket.send(message).await.map_err(LuaError::external)
        });

        methods.add_async_method_mut("recv", |lua, mut this, _: ()| async move {
            if let Some(message) = this.socket.recv().await {
                let message = message.map_err(LuaError::external)?;
                tracing::trace!(?message, "received message");
                lua_from_message(&lua, message)
            } else {
                Ok(LuaValue::Nil)
            }
        });

        methods.add_method("protocol", |_, this, _: ()| {
            Ok(this
                .socket
                .protocol()
                .map(|p| p.to_str().unwrap_or(""))
                .unwrap_or("")
                .to_string())
        });
    }
}
