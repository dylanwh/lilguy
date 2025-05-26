use std::{borrow::Cow, collections::HashMap, time::Duration};

use mdns_sd::{Receiver, ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};
use mlua::prelude::*;
use serde::{ser::SerializeMap, Deserialize, Serialize};

use super::ToLuaArray;

static MDNS_SERVICE_DAEMON: &str = "mdns.service_daemon";

pub fn register(lua: &Lua) -> LuaResult<()> {
    let globals = lua.globals();
    let daemon = LuaServiceDaemon(ServiceDaemon::new().into_lua_err()?);
    lua.set_named_registry_value(MDNS_SERVICE_DAEMON, daemon)?;

    let mdns = lua.create_table()?;
    mdns.set("browse", lua.create_async_function(mdns_browse)?)?;
    mdns.set("register", lua.create_function(mdns_register)?)?;
    mdns.set("stop_browse", lua.create_function(mdns_stop_browse)?)?;
    mdns.set("service_info", lua.create_function(mdns_service_info)?)?;
    globals.set("mdns", mdns)?;

    Ok(())
}

struct LuaServiceDaemon(ServiceDaemon);

impl LuaUserData for LuaServiceDaemon {}

#[derive(Debug, Clone)]
pub struct LuaServiceInfo(ServiceInfo);

impl Serialize for LuaServiceInfo {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let service_info = &self.0;
        let mut map = serializer.serialize_map(Some(6))?;
        map.serialize_entry("type", service_info.get_type())?;
        map.serialize_entry("subtype", &service_info.get_subtype())?;
        map.serialize_entry("fullname", service_info.get_fullname())?;
        map.serialize_entry("hostname", service_info.get_hostname())?;
        map.serialize_entry("port", &service_info.get_port())?;
        map.serialize_entry("addresses", &service_info.get_addresses())?;
        map.end()
    }
}

impl LuaUserData for LuaServiceInfo {
    fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("type", |lua, this| {
            let service_type = this.0.get_type();
            lua.create_string(service_type)
        });
        fields.add_field_method_get("subtype", |_lua, this| {
            let subtype = this.0.get_subtype().clone();
            Ok(subtype)
        });
        fields.add_field_method_get("fullname", |lua, this| {
            let fullname = this.0.get_fullname();
            lua.create_string(fullname)
        });
        fields.add_field_method_get("hostname", |lua, this| {
            let hostname = this.0.get_hostname();
            lua.create_string(hostname)
        });
        fields.add_field_method_get("port", |_lua, this| {
            let port = this.0.get_port();
            Ok(port)
        });
        fields.add_field_method_get("addresses", |lua, this| {
            let addresses = this.0.get_addresses();
            addresses
                .into_iter()
                .map(ToString::to_string)
                .to_lua_array(lua)
        });
    }
}

fn get_service_daemon(lua: &Lua) -> LuaResult<ServiceDaemon> {
    let daemon = lua.named_registry_value::<LuaAnyUserData>(MDNS_SERVICE_DAEMON)?;
    let daemon = daemon
        .borrow::<LuaServiceDaemon>()
        .map_err(|e| LuaError::RuntimeError(format!("Failed to borrow daemon: {}", e)))?;
    Ok(daemon.0.clone())
}

async fn mdns_browse(lua: Lua, (service_type, callbacks): (String, LuaTable)) -> LuaResult<()> {
    let daemon = get_service_daemon(&lua)?;
    let receiver = daemon.browse(&service_type).into_lua_err()?;

    let callbacks = Callbacks::new(callbacks)?;

    tokio::spawn(async move {
        while let Ok(event) = receiver.recv_async().await {
            if let Err(err) = process_event(&lua, event, &callbacks).await {
                tracing::error!("error processing mdns.browse event: {}", err);
            }
        }
    });

    Ok(())
}

fn mdns_register(lua: &Lua, service_info: LuaAnyUserData) -> LuaResult<()> {
    let daemon = get_service_daemon(&lua)?;
    let LuaServiceInfo(service_info) = service_info.borrow::<LuaServiceInfo>()?.clone();

    daemon.register(service_info).into_lua_err()
}

pub struct Callbacks {
    search_started: Option<LuaFunction>,
    service_found: Option<LuaFunction>,
    service_resolved: Option<LuaFunction>,
    service_removed: Option<LuaFunction>,
    search_stopped: Option<LuaFunction>,
}

impl Callbacks {
    fn new(table: LuaTable) -> LuaResult<Self> {
        let search_started: Option<LuaFunction> = table.get("search_started")?;
        let service_found: Option<LuaFunction> = table.get("service_found")?;
        let service_resolved: Option<LuaFunction> = table.get("service_resolved")?;
        let service_removed: Option<LuaFunction> = table.get("service_removed")?;
        let search_stopped: Option<LuaFunction> = table.get("search_stopped")?;
        if search_started.is_none()
            && service_found.is_none()
            && service_resolved.is_none()
            && service_removed.is_none()
            && search_stopped.is_none()
        {
            return Err(LuaError::RuntimeError(
                "at least one of search_started, service_found, service_resolved, service_removed, or search_stopped must be provided".to_string()
            ));
        }

        Ok(Self {
            search_started,
            service_found,
            service_resolved,
            service_removed,
            search_stopped,
        })
    }
}

async fn process_event(lua: &Lua, event: ServiceEvent, callbacks: &Callbacks) -> LuaResult<()> {
    match event {
        ServiceEvent::SearchStarted(service_type) => {
            if let Some(ref callback) = callbacks.search_started {
                callback.call_async::<()>((service_type,)).await?;
            }
        }
        ServiceEvent::ServiceFound(service_type, fullname) => {
            if let Some(ref callback) = callbacks.service_found {
                callback.call_async::<()>((service_type, fullname)).await?;
            }
        }
        ServiceEvent::ServiceResolved(service_info) => {
            if let Some(ref callback) = callbacks.service_resolved {
                callback
                    .call_async::<()>(lua.create_ser_userdata(LuaServiceInfo(service_info)))
                    .await?;
            }
        }
        ServiceEvent::ServiceRemoved(service_type, fullname) => {
            if let Some(ref callback) = callbacks.service_removed {
                callback.call_async::<()>((service_type, fullname)).await?;
            }
        }
        ServiceEvent::SearchStopped(service_type) => {
            if let Some(ref callback) = callbacks.search_stopped {
                callback.call_async::<()>((service_type,)).await?;
            }
        }
    }

    Ok(())
}

fn mdns_stop_browse(lua: &Lua, service_type: String) -> LuaResult<()> {
    let daemon = lua.named_registry_value::<LuaAnyUserData>(MDNS_SERVICE_DAEMON)?;
    daemon
        .borrow::<LuaServiceDaemon>()?
        .0
        .stop_browse(&service_type)
        .into_lua_err()?;

    Ok(())
}

fn mdns_service_info(
    lua: &Lua,
    (ty_domain, my_name, host_name, ip, port, properties): (
        String,
        String,
        String,
        String,
        u16,
        Option<HashMap<String, String>>,
    ),
) -> LuaResult<LuaAnyUserData> {
    let service_info = ServiceInfo::new(
        &ty_domain,
        &my_name,
        &host_name,
        ip,
        port,
        properties.unwrap_or_default(),
    )
    .into_lua_err()?;

    lua.create_ser_userdata(LuaServiceInfo(service_info))
}
