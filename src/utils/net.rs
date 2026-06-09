use std::net::{IpAddr, Ipv4Addr};

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
    /// 适配器 GUID（形如 `{XXXXXXXX-....}`），等同于 WMI
    /// `Win32_NetworkAdapterConfiguration.SettingID`，用于定位待配置的网卡。
    pub guid: String,
    /// 协商链路速率（bit/s）；取自 ipconfig 的 TransmitLinkSpeed。非 Windows 为 None。
    pub link_speed_bps: Option<u64>,
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

                for ip in adapter.ip_addresses().iter() {
                    match ip {
                        IpAddr::V4(v4) => {
                            ipv4.push(v4.to_string());
                            if cidr.is_none() {
                                // 找出该 IPv4 所属的“真实子网”前缀，而非 /32 主机路由。
                                // prefixes 同时包含子网(如 192.168.1.0/24)、主机路由
                                // (192.168.1.x/32) 与广播(192.168.1.255/32)；只匹配
                                // prefix_ip==ip 会错取 /32，导致掩码被算成 255.255.255.255，
                                // 进而使 EnableStatic 返回错误码 66。取网络地址匹配且
                                // 0<len<32 中最长（最具体）的一个。
                                let ip_bits = u32::from(*v4);
                                let mut best: Option<u32> = None;
                                for (prefix_ip, len) in adapter.prefixes() {
                                    if let IpAddr::V4(net) = prefix_ip {
                                        if *len == 0 || *len >= 32 {
                                            continue;
                                        }
                                        let mask = u32::MAX << (32 - *len);
                                        if ip_bits & mask == u32::from(*net) {
                                            best =
                                                Some(best.map_or(*len, |b| b.max(*len)));
                                        }
                                    }
                                }
                                if let Some(len) = best {
                                    cidr = Some(format!("{}/{}", v4, len));
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
                    guid: adapter.adapter_name().to_string(),
                    link_speed_bps: {
                        let s = adapter.transmit_link_speed();
                        if s == 0 || s == u64::MAX { None } else { Some(s) }
                    },
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

/// 查询指定 GUID 网卡当前的发送链路速率（bit/s）。
///
/// 用于链路质量测试期间**实时刷新**有线协商速率：协商速率通常恒定，但链路
/// 重协商 / 降速（如网线劣化使 1Gbps 退回 100Mbps）/ 断开时会变化或消失，
/// 实时读取可如实反映，而非显示开始时的快照。未找到 / 无效值 / 失败返回 `None`。
/// 比 `get_interfaces` 轻量：仅 `GetAdaptersAddresses`，不查 WLAN / DHCP。
#[cfg(target_os = "windows")]
pub fn link_speed_for_guid(guid: &str) -> Option<u64> {
    use windows::Win32::Foundation::ERROR_BUFFER_OVERFLOW;
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_PREFIX, IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::Networking::WinSock::AF_UNSPEC;

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

        if ret != 0 {
            return None;
        }

        let mut p_curr = buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !p_curr.is_null() {
            let adapter = &*p_curr;
            if !adapter.AdapterName.is_null() {
                let name = adapter.AdapterName.to_string().unwrap_or_default();
                if name.eq_ignore_ascii_case(guid) {
                    let s = adapter.TransmitLinkSpeed;
                    return if s == 0 || s == u64::MAX { None } else { Some(s) };
                }
            }
            p_curr = adapter.Next;
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
pub fn link_speed_for_guid(_guid: &str) -> Option<u64> {
    None
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

/// 解析设备主机名，多路回退以适配「系统 DNS 不可用/被 VPN 接管」的局域网场景：
///
/// 1. **反向 DNS**（`getnameinfo`）：走系统当前 DNS 解析器。最快，但若无 PTR 记录
///    常直接回填数字 IP；且 TUN/VPN 接管 DNS 时对内网设备多半失败。
/// 2. **NetBIOS 节点状态**（UDP/137，等价 `nbtstat -A`）：直接问设备本身要它的
///    NetBIOS 名称表，**不经系统 DNS**。Windows 主机、部分设备会响应。
/// 3. **mDNS 反向**（组播 224.0.0.251:5353 查 in-addr.arpa 的 PTR）：拿设备的
///    `xxx.local` 名，同样**绕开系统 DNS**。Apple/打印机/部分安卓会响应。
///
/// 三步均 best-effort、各带短超时；任一步拿到「看起来像名字」（非 IP 文本）的结果即返回。
pub fn resolve_hostname(ip: IpAddr) -> Option<String> {
    // 1. 反向 DNS：过滤掉「无 PTR 时回填的数字 IP」，否则会出现「名字栏显示 IP」。
    if let Some(name) = dns_lookup::lookup_addr(&ip).ok().filter(|n| looks_like_hostname(n)) {
        return Some(name);
    }

    // 2 & 3. 仅 IPv4 局域网设备适用 NetBIOS / mDNS。
    if let IpAddr::V4(v4) = ip {
        if let Some(name) = resolve_netbios(v4).filter(|n| looks_like_hostname(n)) {
            return Some(name);
        }
        if let Some(name) = resolve_mdns(v4).filter(|n| looks_like_hostname(n)) {
            return Some(name);
        }
    }
    None
}

/// 是否「看起来是主机名」：非空且不是纯 IP 文本（反向 DNS 无记录时常回填数字 IP）。
fn looks_like_hostname(s: &str) -> bool {
    let s = s.trim();
    !s.is_empty() && s.parse::<IpAddr>().is_err()
}

// -----------------------------------------------------------------------------
// NetBIOS 节点状态查询（UDP/137）—— 不依赖系统 DNS，直接问设备本身。
// -----------------------------------------------------------------------------

/// 向 `<ip>:137` 发 NetBIOS 节点状态请求，取回名称表里的「工作站名」(后缀 0x00、非组名)。
fn resolve_netbios(ip: Ipv4Addr) -> Option<String> {
    use std::net::UdpSocket;
    use std::time::Duration;

    // 节点状态请求报文（固定）：头部 + 问题(查询名 "*" 的首级编码 + Type NBSTAT + Class IN)。
    let mut req: Vec<u8> = vec![
        0x00, 0x00, // Transaction ID
        0x00, 0x00, // Flags
        0x00, 0x01, // Questions = 1
        0x00, 0x00, // Answer RRs
        0x00, 0x00, // Authority RRs
        0x00, 0x00, // Additional RRs
        0x20, // 名称长度 = 32（首级编码后）
    ];
    // 查询名 "*" 补 0x00 至 16 字节，再做首级编码：每字节高/低半字节各 + 'A'。
    let mut nb_name = [0u8; 16];
    nb_name[0] = b'*';
    for b in nb_name {
        req.push(b'A' + (b >> 4));
        req.push(b'A' + (b & 0x0F));
    }
    req.push(0x00); // 名称结束（根标签）
    req.extend_from_slice(&[0x00, 0x21]); // Type = NBSTAT
    req.extend_from_slice(&[0x00, 0x01]); // Class = IN

    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.set_read_timeout(Some(Duration::from_millis(250))).ok()?;
    sock.send_to(&req, (ip, 137)).ok()?;

    let mut buf = [0u8; 1024];
    let (n, _) = sock.recv_from(&mut buf).ok()?;
    parse_netbios_response(&buf[..n])
}

/// 解析 NetBIOS 节点状态响应的名称表，返回首个「工作站名」(后缀 0x00、非组)，
/// 否则退回首个非组唯一名。纯函数，便于单测。
fn parse_netbios_response(buf: &[u8]) -> Option<String> {
    if buf.len() < 13 {
        return None;
    }
    // 应答记录名长度：压缩指针(高2位=11)占 2 字节；否则 0x20 + 32 + 0x00 = 34 字节。
    let name_len = if buf[12] & 0xC0 == 0xC0 { 2 } else { 34 };
    // RDATA 偏移 = 头(12) + 应答名 + Type(2)+Class(2)+TTL(4)+RDLEN(2)
    let rdata = 12 + name_len + 10;
    if buf.len() <= rdata {
        return None;
    }
    let num = buf[rdata] as usize;
    let mut off = rdata + 1;
    let mut fallback: Option<String> = None;
    for _ in 0..num {
        if off + 18 > buf.len() {
            break;
        }
        let name = String::from_utf8_lossy(&buf[off..off + 15])
            .trim_end()
            .trim_end_matches('\u{0}')
            .trim_end()
            .to_string();
        let suffix = buf[off + 15];
        let flags = u16::from_be_bytes([buf[off + 16], buf[off + 17]]);
        let is_group = flags & 0x8000 != 0;
        off += 18;
        if name.is_empty() || is_group {
            continue;
        }
        if suffix == 0x00 {
            return Some(name); // 工作站名，最优
        }
        if fallback.is_none() {
            fallback = Some(name);
        }
    }
    fallback
}

// -----------------------------------------------------------------------------
// mDNS 反向解析（组播 224.0.0.251:5353）—— 拿设备的 xxx.local 名。
// -----------------------------------------------------------------------------

/// 组播查询 `d.c.b.a.in-addr.arpa` 的 PTR，解析出设备 `.local` 名。
fn resolve_mdns(ip: Ipv4Addr) -> Option<String> {
    use std::net::{SocketAddrV4, UdpSocket};
    use std::time::Duration;

    let o = ip.octets();
    let qname = format!("{}.{}.{}.{}.in-addr.arpa", o[3], o[2], o[1], o[0]);

    let mut req: Vec<u8> = vec![
        0x00, 0x00, // ID
        0x00, 0x00, // Flags
        0x00, 0x01, // Questions = 1
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // 其余计数 = 0
    ];
    for label in qname.split('.') {
        req.push(label.len() as u8);
        req.extend_from_slice(label.as_bytes());
    }
    req.push(0x00); // 名称结束
    req.extend_from_slice(&[0x00, 0x0C]); // Type = PTR
    req.extend_from_slice(&[0x80, 0x01]); // Class = IN + QU(请求单播应答)位

    let sock = UdpSocket::bind("0.0.0.0:0").ok()?;
    sock.set_read_timeout(Some(Duration::from_millis(250))).ok()?;
    let mdns = SocketAddrV4::new(Ipv4Addr::new(224, 0, 0, 251), 5353);
    sock.send_to(&req, mdns).ok()?;

    let mut buf = [0u8; 1500];
    let (n, _) = sock.recv_from(&mut buf).ok()?;
    parse_mdns_ptr(&buf[..n])
}

/// 从 DNS 响应中找首个 PTR 应答并解码其目标名（去掉尾部 `.local`）。纯函数，便于单测。
fn parse_mdns_ptr(buf: &[u8]) -> Option<String> {
    if buf.len() < 12 {
        return None;
    }
    let qd = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let an = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    if an == 0 {
        return None;
    }
    let mut off = 12;
    // 跳过问题区：每问 = 名称 + Type(2) + Class(2)。
    for _ in 0..qd {
        off = skip_dns_name(buf, off)?;
        off += 4;
    }
    // 遍历应答区找 PTR。
    for _ in 0..an {
        let after_name = skip_dns_name(buf, off)?;
        if after_name + 10 > buf.len() {
            return None;
        }
        let rtype = u16::from_be_bytes([buf[after_name], buf[after_name + 1]]);
        let rdlen = u16::from_be_bytes([buf[after_name + 8], buf[after_name + 9]]) as usize;
        let rdata = after_name + 10;
        if rdata + rdlen > buf.len() {
            return None;
        }
        if rtype == 0x000C {
            let (name, _) = decode_dns_name(buf, rdata)?;
            let name = name
                .trim_end_matches('.')
                .trim_end_matches(".local")
                .trim_end_matches('.')
                .to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
        off = rdata + rdlen;
    }
    None
}

/// 跳过一个 DNS 名称（处理压缩指针），返回其后的偏移。
fn skip_dns_name(buf: &[u8], mut off: usize) -> Option<usize> {
    loop {
        if off >= buf.len() {
            return None;
        }
        let len = buf[off];
        if len == 0 {
            return Some(off + 1);
        }
        if len & 0xC0 == 0xC0 {
            return Some(off + 2); // 压缩指针占 2 字节，名称到此结束
        }
        off += 1 + len as usize;
    }
}

/// 解析一个 DNS 名称为点分字符串（支持压缩指针）。返回 (名称, 名称之后的偏移)。
fn decode_dns_name(buf: &[u8], start: usize) -> Option<(String, usize)> {
    let mut labels: Vec<String> = Vec::new();
    let mut off = start;
    let mut next: Option<usize> = None; // 跟随指针前记录「真正下一个偏移」
    let mut hops = 0;
    loop {
        if off >= buf.len() {
            return None;
        }
        let len = buf[off];
        if len == 0 {
            if next.is_none() {
                next = Some(off + 1);
            }
            break;
        }
        if len & 0xC0 == 0xC0 {
            if off + 1 >= buf.len() {
                return None;
            }
            let ptr = (((len & 0x3F) as usize) << 8) | buf[off + 1] as usize;
            if next.is_none() {
                next = Some(off + 2);
            }
            hops += 1;
            if hops > 16 {
                return None; // 防止指针成环
            }
            off = ptr;
            continue;
        }
        let s = off + 1;
        let e = s + len as usize;
        if e > buf.len() {
            return None;
        }
        labels.push(String::from_utf8_lossy(&buf[s..e]).to_string());
        off = e;
    }
    Some((labels.join("."), next.unwrap_or(off)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostname_filter_rejects_ip_text() {
        assert!(looks_like_hostname("vivo"));
        assert!(looks_like_hostname("MyPC.local"));
        assert!(!looks_like_hostname("192.168.1.5"));
        assert!(!looks_like_hostname(""));
        assert!(!looks_like_hostname("  "));
        assert!(!looks_like_hostname("fe80::1"));
    }

    #[test]
    fn netbios_parses_workstation_name() {
        // 构造一个最小节点状态响应：头(12) + 非压缩应答名(34) + 固定 RR 头 + RDATA。
        let mut buf = vec![0u8; 12];
        // 应答名：0x20 + 32 字节(随便填) + 0x00
        buf.push(0x20);
        buf.extend(std::iter::repeat(b'A').take(32));
        buf.push(0x00);
        // Type(2) Class(2) TTL(4) RDLEN(2)
        buf.extend_from_slice(&[0x00, 0x21, 0x00, 0x01, 0, 0, 0, 0, 0x00, 0x00]);
        // RDATA：名称数 = 2
        buf.push(2);
        // 名1：组名(后缀 0x00 但是 group) → 应跳过
        let mut name1 = b"WORKGROUP      ".to_vec(); // 15 字节
        assert_eq!(name1.len(), 15);
        buf.append(&mut name1);
        buf.push(0x00); // 后缀
        buf.extend_from_slice(&[0x80, 0x00]); // flags：group 位
                                              // 名2：工作站唯一名（后缀 0x00、非组）
        let mut name2 = b"VIVO-PHONE     ".to_vec(); // 15 字节
        assert_eq!(name2.len(), 15);
        buf.append(&mut name2);
        buf.push(0x00); // 后缀 = 工作站
        buf.extend_from_slice(&[0x00, 0x00]); // flags：unique

        assert_eq!(parse_netbios_response(&buf).as_deref(), Some("VIVO-PHONE"));
    }

    #[test]
    fn mdns_decodes_ptr_target() {
        // 头：ID(2) Flags(2) QD=1 AN=1 NS=0 AR=0
        let mut buf = vec![0x00, 0x00, 0x84, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00];
        // 问题名：5.1.168.192.in-addr.arpa（这里简化用单标签也可，关键测 PTR 解码）
        for label in ["5", "1", "168", "192", "in-addr", "arpa"] {
            buf.push(label.len() as u8);
            buf.extend_from_slice(label.as_bytes());
        }
        buf.push(0x00);
        buf.extend_from_slice(&[0x00, 0x0C, 0x00, 0x01]); // Type PTR, Class IN
        // 应答：名称用压缩指针指回问题名(偏移 12)
        buf.extend_from_slice(&[0xC0, 0x0C]);
        buf.extend_from_slice(&[0x00, 0x0C, 0x00, 0x01]); // Type PTR, Class IN
        buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // TTL
        // RDATA：vivo-phone.local
        let rdata_start = buf.len() + 2;
        let mut rdata = Vec::new();
        for label in ["vivo-phone", "local"] {
            rdata.push(label.len() as u8);
            rdata.extend_from_slice(label.as_bytes());
        }
        rdata.push(0x00);
        buf.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
        buf.extend_from_slice(&rdata);
        let _ = rdata_start;

        assert_eq!(parse_mdns_ptr(&buf).as_deref(), Some("vivo-phone"));
    }
}
