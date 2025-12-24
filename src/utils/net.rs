use std::net::IpAddr;
#[cfg(target_os = "windows")]
use std::net::Ipv4Addr;

#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    pub name: String,
    pub description: String,
    pub mac: String,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
    pub is_up: bool,
    pub ssid: Option<String>,
    pub dhcp_enabled: bool,
    pub is_physical: bool,
    pub interface_type: String,
    pub cidr: Option<String>,
}

#[cfg(target_os = "windows")]
pub fn get_interfaces() -> Vec<InterfaceInfo> {
    let mut result = Vec::new();
    let ssid_map = get_ssid_map_via_win32();
    let dhcp_map = get_dhcp_map_via_win32();

    match ipconfig::get_adapters() {
        Ok(adapters) => {
            for adapter in adapters {
                if adapter.if_type() == ipconfig::IfType::SoftwareLoopback
                    || adapter.if_type() == ipconfig::IfType::Tunnel
                {
                    continue;
                }

                let is_physical = matches!(
                    adapter.if_type(),
                    ipconfig::IfType::EthernetCsmacd | ipconfig::IfType::Ieee80211
                );

                let mac = adapter
                    .physical_address()
                    .map(|addr| {
                        addr.iter()
                            .map(|b| format!("{:02x}", b))
                            .collect::<Vec<String>>()
                            .join(":")
                    })
                    .unwrap_or_default();

                let mut ipv4 = Vec::new();
                let mut ipv6 = Vec::new();
                let mut cidr = None;

                for (i, ip) in adapter.ip_addresses().iter().enumerate() {
                    match ip {
                        IpAddr::V4(v4) => {
                            ipv4.push(v4.to_string());
                            if cidr.is_none() {
                                // 修复：prefixes 未使用的警告
                                if let Some(_prefixes) = adapter.prefixes().get(i) {
                                    for (prefix_ip, len) in adapter.prefixes() {
                                        if prefix_ip == ip {
                                            cidr = Some(format!("{}/{}", v4, len));
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        IpAddr::V6(v6) => ipv6.push(v6.to_string()),
                    }
                }

                let is_up = match adapter.oper_status() {
                    ipconfig::OperStatus::IfOperStatusUp => true,
                    _ => false,
                };

                let mut ssid = None;
                if is_up {
                    if let Some(s) = ssid_map.get(adapter.adapter_name()) {
                        ssid = Some(s.clone());
                    }
                }

                let dhcp_enabled = dhcp_map
                    .get(adapter.friendly_name())
                    .copied()
                    .unwrap_or(false);

                result.push(InterfaceInfo {
                    name: adapter.friendly_name().to_string(),
                    description: adapter.description().to_string(),
                    mac,
                    ipv4,
                    ipv6,
                    is_up,
                    ssid,
                    dhcp_enabled,
                    is_physical,
                    interface_type: format!("{:?}", adapter.if_type()),
                    cidr,
                });
            }
        }
        Err(_) => {}
    }

    result.sort_by(|a, b| b.is_up.cmp(&a.is_up).then_with(|| a.name.cmp(&b.name)));
    result
}

#[cfg(not(target_os = "windows"))]
pub fn get_interfaces() -> Vec<InterfaceInfo> {
    Vec::new()
}

// -----------------------------------------------------------------------------
// Windows Native API 辅助函数
// -----------------------------------------------------------------------------

#[cfg(target_os = "windows")]
fn get_ssid_map_via_win32() -> std::collections::HashMap<String, String> {
    use std::ptr;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::NetworkManagement::WiFi::{
        wlan_intf_opcode_current_connection, WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory,
        WlanOpenHandle, WlanQueryInterface, WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST,
    };

    let mut map = std::collections::HashMap::new();

    unsafe {
        let mut negotiated_version = 0;
        let mut client_handle = HANDLE::default();

        if WlanOpenHandle(2, None, &mut negotiated_version, &mut client_handle) != 0 {
            return map;
        }

        let mut interface_list: *mut WLAN_INTERFACE_INFO_LIST = ptr::null_mut();

        if WlanEnumInterfaces(client_handle, None, &mut interface_list) == 0 {
            if !interface_list.is_null() {
                let list = &*interface_list;

                for i in 0..list.dwNumberOfItems {
                    let info = list.InterfaceInfo.as_ptr().offset(i as isize);
                    let guid = (*info).InterfaceGuid;

                    let guid_str = format!("{{{:?}}}", guid).to_uppercase();

                    let mut data_ptr: *mut std::ffi::c_void = ptr::null_mut();
                    let mut data_size = 0;

                    let query_res = WlanQueryInterface(
                        client_handle,
                        &guid,
                        wlan_intf_opcode_current_connection,
                        None,
                        &mut data_size,
                        &mut data_ptr,
                        None,
                    );

                    if query_res == 0 && !data_ptr.is_null() {
                        let stats = &*(data_ptr as *const WLAN_CONNECTION_ATTRIBUTES);

                        let len = stats.wlanAssociationAttributes.dot11Ssid.uSSIDLength as usize;
                        if len > 0 && len <= 32 {
                            let ssid_bytes =
                                &stats.wlanAssociationAttributes.dot11Ssid.ucSSID[..len];
                            let ssid_str = String::from_utf8_lossy(ssid_bytes).to_string();
                            map.insert(guid_str, ssid_str);
                        }

                        WlanFreeMemory(data_ptr);
                    }
                }

                WlanFreeMemory(interface_list as *mut std::ffi::c_void);
            }
        }

        WlanCloseHandle(client_handle, None);
    }

    map
}

#[cfg(target_os = "windows")]
fn get_dhcp_map_via_win32() -> std::collections::HashMap<String, bool> {
    use windows::Win32::Foundation::ERROR_BUFFER_OVERFLOW;
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_PREFIX, IP_ADAPTER_ADDRESSES_LH,
        IP_ADAPTER_DHCP_ENABLED,
    };
    use windows::Win32::Networking::WinSock::AF_UNSPEC;

    let mut map = std::collections::HashMap::new();

    unsafe {
        let mut out_buf_len: u32 = 15000;
        let mut buffer: Vec<u8> = vec![0; out_buf_len as usize];

        let mut ret = GetAdaptersAddresses(
            AF_UNSPEC.0 as u32,
            GAA_FLAG_INCLUDE_PREFIX,
            None,
            Some(buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
            &mut out_buf_len,
        );

        if ret == ERROR_BUFFER_OVERFLOW.0 {
            buffer.resize(out_buf_len as usize, 0);
            ret = GetAdaptersAddresses(
                AF_UNSPEC.0 as u32,
                GAA_FLAG_INCLUDE_PREFIX,
                None,
                Some(buffer.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut out_buf_len,
            );
        }

        if ret == 0 {
            let mut p_curr = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;

            while !p_curr.is_null() {
                let adapter = &*p_curr;

                let name = if !adapter.FriendlyName.is_null() {
                    adapter.FriendlyName.to_string().unwrap_or_default()
                } else {
                    String::new()
                };

                let is_dhcp = (adapter.Anonymous2.Flags & IP_ADAPTER_DHCP_ENABLED) != 0;

                if !name.is_empty() {
                    map.insert(name, is_dhcp);
                }

                p_curr = adapter.Next;
            }
        }
    }

    map
}

#[cfg(target_os = "windows")]
pub fn resolve_mac_address(ip: Ipv4Addr) -> Option<String> {
    use std::ffi::CString;
    use windows::core::PCSTR; // 引入 PCSTR
    use windows::Win32::NetworkManagement::IpHelper::SendARP;
    use windows::Win32::Networking::WinSock::inet_addr;

    unsafe {
        let ip_str = CString::new(ip.to_string()).ok()?;
        // 修复：inet_addr 在 windows-rs 0.58 需要 PCSTR 参数
        // 强制转换 *const i8 为 *const u8，并包装为 PCSTR
        let dest_ip = inet_addr(PCSTR(ip_str.as_ptr() as *const u8));

        let src_ip = 0u32;
        let mut mac_buf = [0u8; 6];
        let mut mac_len = 6u32;

        let ret = SendARP(
            dest_ip,
            src_ip,
            mac_buf.as_mut_ptr() as *mut _,
            &mut mac_len,
        );

        if ret == 0 && mac_len == 6 {
            Some(format!(
                "{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
                mac_buf[0], mac_buf[1], mac_buf[2], mac_buf[3], mac_buf[4], mac_buf[5]
            ))
        } else {
            None
        }
    }
}

pub fn resolve_hostname(ip: IpAddr) -> Option<String> {
    // 启用 dns-lookup
    dns_lookup::lookup_addr(&ip).ok()
}
