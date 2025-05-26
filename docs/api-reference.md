# API Reference

## mDNS API

**Note on Service Types:** Service types used in this API (e.g., for browsing with `mdns.browse` or for the `type_domain` in `mdns.service_info`) must be fully qualified domain names (FQDNs). This means they should include the protocol (e.g., `_tcp` or `_udp`), the `.local` domain, and critically, end with a trailing dot. For example: `"_http._tcp.local."` or `"_myservice._udp.local."`.

### `mdns.browse(service_type, callbacks_table)`

Discovers services of a given type on the local network. This function initiates a browse operation and calls the appropriate functions provided in the `callbacks_table` as mDNS events occur.

**Parameters:**

*   `service_type` (String): The fully qualified service type to browse for, ending with a trailing dot (e.g., `"_http._tcp.local."` or `"_mydevice._tcp.local."`).
*   `callbacks_table` (Table): A Lua table containing callback functions for different mDNS events. The keys in this table must be the exact snake_case strings specified below. All callbacks are optional, but providing them is necessary to receive notifications for the corresponding events.
    *   `search_started = function(stype)`: Called when the mDNS search operation has successfully started for the given `stype` (String, the service type being browsed).
    *   `service_found = function(stype, fullname)`: Called when a new service instance is initially discovered.
        *   `stype` (String): The service type that was being browsed for.
        *   `fullname` (String): The full service instance name (e.g., `"My Web Server._http._tcp.local"`). This name can typically be used for resolution if needed.
    *   `service_resolved = function(service_info)`: Called when a discovered service's details (including IP addresses and TXT records) have been resolved.
        *   `service_info` (Table): A "Service Info Object" (see the "Service Info Object" section below for its detailed structure, including fields like `type`, `subtype`, `full_name`, `hostname`, `port`, `addresses`).
    *   `service_removed = function(stype, fullname)`: Called when a previously discovered service instance is no longer advertised or has expired.
        *   `stype` (String): The service type that was being browsed for.
        *   `fullname` (String): The full service instance name of the removed service.
    *   `search_stopped = function(stype)`: Called when the mDNS search operation for the given `stype` (String) has stopped, either due to a call to `mdns.stop_browse()` or an internal reason (e.g., network interface change).

**Returns:**

*   This function does not have a meaningful return value. Any returned value should not be used.

**Example:**

```lua
local service_to_browse = "_http._tcp.local." -- Example service type, fully qualified with a trailing dot

mdns.browse(service_to_browse, {
  search_started = function(stype)
    print("Search successfully started for service type: " .. stype)
  end,
  service_found = function(stype, fname)
    print("Service found: Fullname - '" .. fname .. "', Type - '" .. stype .. "'")
    -- Note: At this point, you only have basic info. 
    -- The 'service_resolved' callback will provide more details like IP addresses.
  end,
  service_resolved = function(s_info)
    print("Service resolved: " .. s_info.full_name)
    print("  Type: " .. s_info.type)
    if s_info.subtype and s_info.subtype ~= "" then
      print("  Subtype: " .. s_info.subtype)
    end
    print("  Hostname: " .. s_info.hostname)
    print("  Port: " .. s_info.port)
    if s_info.addresses and #s_info.addresses > 0 then
      for i, addr in ipairs(s_info.addresses) do
        print("  Address " .. i .. ": " .. addr)
      end
    else
      print("  Addresses: Not available")
    end
    -- The 'Service Info Object' (s_info) does not include TXT records as per its definition.
    -- If TXT records were needed, they would typically be part of a more detailed resolution step
    -- or if the 'service_found' callback provided them initially (implementation dependent).
  end,
  service_removed = function(stype, fname)
    print("Service removed: Fullname - '" .. fname .. "', Type - '" .. stype .. "'")
  end,
  search_stopped = function(stype)
    print("Search stopped for service type: " .. stype)
  end
})

-- To stop browsing for this service type later, you might call:
-- mdns.stop_browse(service_to_browse) -- service_to_browse would be "_http._tcp.local."
```

### `mdns.stop_browse(service_type)`

Stops all active mDNS service discovery operations for a given service type.

**Parameters:**

*   `service_type` (String): The fully qualified service type for which to stop browsing, ending with a trailing dot (e.g., `"_http._tcp.local."`). Any active browse operations initiated with this exact service type will be terminated.

**Example:**

```lua
-- Start browsing for HTTP services
mdns.browse("_http._tcp.local.", function(info) -- Assuming this is an older callback style for brevity
  -- For the new callback style, you'd pass a table of callbacks.
  -- This example focuses on the service type string.
  if info.action == "add" then -- This is a simplified, old callback example for context
    print("HTTP Service found: " .. info.service.name)
  end
end)

-- Start browsing for FTP services
mdns.browse("_ftp._tcp.local.", function(info) -- Simplified, old callback example
  if info.action == "add" then
    print("FTP Service found: " .. info.service.name)
  end
end)

-- ... later, when you no longer need to discover HTTP services
mdns.stop_browse("_http._tcp.local.")
print("Stopped browsing for HTTP services. FTP browsing may still be active.")

-- To stop FTP browsing as well:
-- mdns.stop_browse("_ftp._tcp.local.")
-- print("Stopped browsing for FTP services.")
```

### `mdns.service_info(type_domain, my_name, host_name, ip, port, [properties])`

Creates a service information object. This object encapsulates all necessary details for registering a service with mDNS and can be passed to `mdns.register`.

**Parameters:**

*   `type_domain` (String): The fully qualified service type and domain, ending with a trailing dot (e.g., `"_http._tcp.local."` or `"_myservice._udp.local."`).
*   `my_name` (String): The instance name for the service (e.g., `"My Web Server"`). This is the human-readable name that will appear in service browsers.
*   `host_name` (String): The hostname of the device providing the service (e.g., `"mydevice.local"`). This should be a name resolvable on the local network.
*   `ip` (String): The IP address of the service. This should be the IP address of the host machine.
*   `port` (Number): The port number on which the service is running (e.g., `80` or `8080`).
*   `properties` (Table, optional): A key/value table of TXT records to be advertised with the service. Keys and values must be strings. (e.g., `{ version = "1.0", path = "/index.html" }`).

**Returns:**

*   (Table): A service information object containing all the provided parameters, structured for use with `mdns.register`.
*   Throws an error if the input parameters are invalid or if the service information object cannot be created for any reason.

**Example:**

```lua
-- Create a service info object for a web server
local web_service_info = mdns.service_info(
  "_http._tcp.local.", 
  "My Personal Web Server", 
  "ariel-pc.local", 
  "192.168.1.100", 
  80, 
  { 
    description = "Ariel's awesome web server",
    path = "/home.html"
  }
)

print("Service info object created for: " .. web_service_info.my_name)
-- This web_service_info object is now ready to be passed to mdns.register:
mdns.register(web_service_info)

-- Create a service info object for a custom service without TXT records
local custom_service_info = mdns.service_info(
  "_mycustomservice._udp.local.", 
  "My Custom UDP Service", 
  "custom-device.local", 
  "192.168.1.101", 
  12345
)

print("Service info object created for: " .. custom_service_info.my_name)
-- This custom_service_info object is now ready to be passed to mdns.register:
mdns.register(custom_service_info)
```

### `mdns.register(service_info_object)`

Registers a service on the local network using a service information object, making it discoverable via mDNS.

**Parameters:**

*   `service_info_object` (Table): A service information object created by `mdns.service_info()`. This object contains all the necessary details for the service to be registered, including its type, name, host, IP address, port, and any TXT properties.

**Example:**

```lua
-- Step 1: Create a service information object using mdns.service_info()
-- If mdns.service_info encounters an error (e.g., invalid parameters), 
-- it will throw an error, and the script might terminate or the error
-- would need to be caught using pcall if explicit handling is desired.
local my_web_server_info = mdns.service_info(
  "_http._tcp.local.",           -- type_domain: Service type and domain
  "My Awesome Web Server",      -- my_name: Instance name of the service
  "my-raspi.local",             -- host_name: Hostname of the device
  "192.168.1.123",              -- ip: IP address of the service
  8080,                         -- port: Port number
  {                             -- properties (TXT records)
    version = "2.0",
    status = "online",
    description = "My main web server running on Raspberry Pi"
  }
)

-- Step 2: Register the service using the created service_info_object
-- We assume my_web_server_info is valid here because an error would have been thrown otherwise.
mdns.register(my_web_server_info)
print("Service '" .. my_web_server_info.my_name .. "' registered successfully.")

-- Example for a service with no TXT properties
local my_other_service_info = mdns.service_info(
  "_myservice._udp.local.",
  "Background Data Service",
  "data-server.local",
  "192.168.1.124",
  9000
  -- No properties table means no TXT records
)

-- We assume my_other_service_info is valid here.
mdns.register(my_other_service_info)
print("Service '" .. my_other_service_info.my_name .. "' registered successfully.")
```

#### Service Info Object

This object represents the detailed information of a discovered mDNS service, often obtained through a resolution process after a service is found by `mdns.browse`.

The object contains the following fields:

*   `type` (String): The primary service type identifier.
    *   Example: `"_http._tcp"`
*   `subtype` (String): A service subtype. If no specific subtype is applicable, this might be an empty string or a default value depending on the mDNS implementation.
    *   Example: `"_printer"` (for a service of type `_ipp._tcp` that is also a printer), or `""` if no subtype.
*   `full_name` (String): The full service instance name, including the service type and domain.
    *   Example: `"My Kitchen Printer._ipp._tcp.local"`
*   `hostname` (String): The hostname of the device providing the service, as advertised on the mDNS network. This name should be resolvable on the local network.
    *   Example: `"kitchen-printer.local"`
*   `port` (Number): The port number on which the service is accessible.
    *   Example: `631` (for IPP printing)
*   `addresses` (Table of strings): A list (array-like table in Lua) of resolved IP address strings for the service. This can include both IPv4 and IPv6 addresses.
    *   Example: `{"192.168.1.105", "fe80::aabb:ccdd:eeff:1122"}`

