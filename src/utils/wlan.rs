//! 无线网卡丰富信息查询（Windows）。
//!
//! 纯换算/标签函数（频率→频段/信道、PHY 标签、auth/cipher 标签、RSSI 近似）
//! 与平台无关，便于单测；Windows FFI `query` 经 WLAN API 取当前关联与 BSS 信息。

/// 某块无线网卡当前关联的丰富信息。
#[derive(Debug, Clone)]
pub struct WirelessInfo {
    pub ssid: String,
    pub bssid: String,
    pub signal_quality: u32, // 0-100
    pub rssi_dbm: i32,
    pub phy_type: String,
    pub wifi_gen: u8, // 4/5/6/7；0=未知/传统
    pub band: String,
    pub channel: u32,
    pub freq_mhz: u32,
    pub rx_rate_mbps: u32,
    pub tx_rate_mbps: u32,
    pub auth: String,
    pub cipher: String,
}

/// 由信道中心频率（kHz）推导 (频段, 信道号)。
pub fn band_and_channel(freq_khz: u32) -> (String, u32) {
    let mhz = freq_khz / 1000;
    if (2412..=2484).contains(&mhz) {
        let ch = if mhz == 2484 { 14 } else { (mhz - 2407) / 5 };
        ("2.4 GHz".to_string(), ch)
    } else if (5150..=5895).contains(&mhz) {
        ("5 GHz".to_string(), (mhz - 5000) / 5)
    } else if (5955..=7125).contains(&mhz) {
        ("6 GHz".to_string(), (mhz - 5950) / 5)
    } else {
        ("-".to_string(), 0)
    }
}

/// DOT11_PHY_TYPE 原始值 → (友好标签, Wi-Fi 代际)。`band6` 用于区分 Wi-Fi 6/6E。
pub fn phy_label(phy: i32, band6: bool) -> (String, u8) {
    match phy {
        11 => ("802.11be · Wi-Fi 7".to_string(), 7),
        10 => (
            if band6 {
                "802.11ax · Wi-Fi 6E".to_string()
            } else {
                "802.11ax · Wi-Fi 6".to_string()
            },
            6,
        ),
        8 => ("802.11ac · Wi-Fi 5".to_string(), 5),
        7 => ("802.11n · Wi-Fi 4".to_string(), 4),
        6 => ("802.11g".to_string(), 0),
        4 => ("802.11a".to_string(), 0),
        5 | 2 => ("802.11b".to_string(), 0),
        _ => ("Unknown".to_string(), 0),
    }
}

/// DOT11_AUTH_ALGORITHM 原始值 → 友好标签。
pub fn auth_label(a: i32) -> String {
    match a {
        1 => "Open",
        2 => "WEP",
        3 => "WPA-Enterprise",
        4 => "WPA-Personal",
        5 => "WPA-None",
        6 => "WPA2-Enterprise",
        7 => "WPA2-Personal",
        8 => "WPA3-Enterprise",
        9 => "WPA3-Personal",
        10 => "OWE",
        11 => "WPA3-Enterprise",
        _ => "-",
    }
    .to_string()
}

/// DOT11_CIPHER_ALGORITHM 原始值 → 友好标签。
pub fn cipher_label(c: i32) -> String {
    match c {
        0 => "None",
        1 => "WEP-40",
        2 => "TKIP",
        4 => "CCMP (AES)",
        5 => "WEP-104",
        8 => "GCMP",
        9 => "GCMP-256",
        10 => "CCMP-256",
        _ => "-",
    }
    .to_string()
}

/// 取不到真实 RSSI 时，由信号质量(%)近似（Windows 标准：0%↔-100dBm，100%↔-50dBm）。
pub fn rssi_from_quality(q: u32) -> i32 {
    -100 + (q.min(100) as i32) / 2
}

/// 查询指定 GUID 网卡当前关联的无线信息。`guid` 形如 `{XXXX-...}`（同 InterfaceInfo.guid）。
#[cfg(target_os = "windows")]
pub fn query(guid: &str) -> Option<WirelessInfo> {
    use std::ptr;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::NetworkManagement::WiFi::{
        dot11_BSS_type_infrastructure, wlan_intf_opcode_current_connection, WlanCloseHandle,
        WlanEnumInterfaces, WlanFreeMemory, WlanGetNetworkBssList, WlanOpenHandle,
        WlanQueryInterface, WLAN_BSS_LIST, WLAN_CONNECTION_ATTRIBUTES, WLAN_INTERFACE_INFO_LIST,
    };

    let want = guid.to_uppercase();

    unsafe {
        let mut negotiated_version = 0u32;
        let mut client_handle = HANDLE::default();
        if WlanOpenHandle(2, None, &mut negotiated_version, &mut client_handle) != 0 {
            return None;
        }

        let mut info = None;
        let mut interface_list: *mut WLAN_INTERFACE_INFO_LIST = ptr::null_mut();
        if WlanEnumInterfaces(client_handle, None, &mut interface_list) == 0
            && !interface_list.is_null()
        {
            let list = &*interface_list;
            for i in 0..list.dwNumberOfItems {
                let iface = &*list.InterfaceInfo.as_ptr().offset(i as isize);
                let iface_guid = iface.InterfaceGuid;
                let guid_str = format!("{{{:?}}}", iface_guid).to_uppercase();
                if guid_str != want {
                    continue;
                }

                // 1) 当前关联属性
                let mut data_ptr: *mut std::ffi::c_void = ptr::null_mut();
                let mut data_size = 0u32;
                if WlanQueryInterface(
                    client_handle,
                    &iface_guid,
                    wlan_intf_opcode_current_connection,
                    None,
                    &mut data_size,
                    &mut data_ptr,
                    None,
                ) != 0
                    || data_ptr.is_null()
                {
                    break;
                }
                let conn = &*(data_ptr as *const WLAN_CONNECTION_ATTRIBUTES);
                let assoc = conn.wlanAssociationAttributes;
                let sec = conn.wlanSecurityAttributes;

                let ssid_len = assoc.dot11Ssid.uSSIDLength as usize;
                let ssid = if ssid_len > 0 && ssid_len <= 32 {
                    String::from_utf8_lossy(&assoc.dot11Ssid.ucSSID[..ssid_len]).to_string()
                } else {
                    String::new()
                };
                let bssid = assoc
                    .dot11Bssid
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<Vec<_>>()
                    .join(":");
                let signal_quality = assoc.wlanSignalQuality;
                let rx_rate_mbps = assoc.ulRxRate / 1000;
                let tx_rate_mbps = assoc.ulTxRate / 1000;
                let phy_raw = assoc.dot11PhyType.0;
                let auth = auth_label(sec.dot11AuthAlgorithm.0);
                let cipher = cipher_label(sec.dot11CipherAlgorithm.0);

                // 2) BSS list 取真实 RSSI 与频率（按 BSSID 匹配当前 AP）
                let mut rssi_dbm = rssi_from_quality(signal_quality);
                let mut freq_khz = 0u32;
                let mut bss_list: *mut WLAN_BSS_LIST = ptr::null_mut();
                if WlanGetNetworkBssList(
                    client_handle,
                    &iface_guid,
                    None,
                    dot11_BSS_type_infrastructure,
                    sec.bSecurityEnabled,
                    None,
                    &mut bss_list,
                ) == 0
                    && !bss_list.is_null()
                {
                    let bl = &*bss_list;
                    for j in 0..bl.dwNumberOfItems {
                        let entry = &*bl.wlanBssEntries.as_ptr().offset(j as isize);
                        if entry.dot11Bssid == assoc.dot11Bssid {
                            rssi_dbm = entry.lRssi;
                            freq_khz = entry.ulChCenterFrequency;
                            break;
                        }
                    }
                    WlanFreeMemory(bss_list as *mut std::ffi::c_void);
                }

                let (band, channel) = band_and_channel(freq_khz);
                let band6 = band == "6 GHz";
                let (phy_type, wifi_gen) = phy_label(phy_raw, band6);

                info = Some(WirelessInfo {
                    ssid,
                    bssid,
                    signal_quality,
                    rssi_dbm,
                    phy_type,
                    wifi_gen,
                    band,
                    channel,
                    freq_mhz: freq_khz / 1000,
                    rx_rate_mbps,
                    tx_rate_mbps,
                    auth,
                    cipher,
                });

                WlanFreeMemory(data_ptr);
                break;
            }
            WlanFreeMemory(interface_list as *mut std::ffi::c_void);
        }

        WlanCloseHandle(client_handle, None);
        info
    }
}

/// Linux：guid 即接口名。组合 `iw link` + band/channel 纯函数。缺字段降级。
#[cfg(target_os = "linux")]
pub fn query(guid: &str) -> Option<WirelessInfo> {
    let link = linux::parse_iw_link(&run_iw(&["dev", guid, "link"])?)?;
    let freq_mhz = link.freq_mhz.unwrap_or(0);
    let (band, channel) = band_and_channel(freq_mhz * 1000); // 纯函数吃 kHz
    let signal_quality = link
        .signal_dbm
        .map(|d| (((d + 100).max(0).min(50)) as u32) * 2) // -100..-50dBm → 0..100%
        .unwrap_or(0);
    Some(WirelessInfo {
        ssid: link.ssid.unwrap_or_default(),
        bssid: link.bssid.unwrap_or_default(),
        signal_quality,
        rssi_dbm: link.signal_dbm.unwrap_or_else(|| rssi_from_quality(signal_quality)),
        phy_type: "-".to_string(),
        wifi_gen: 0,
        band,
        channel,
        freq_mhz,
        rx_rate_mbps: link.rx_mbps.unwrap_or(0),
        tx_rate_mbps: link.tx_mbps.unwrap_or(0),
        auth: "-".to_string(),
        cipher: "-".to_string(),
    })
}

#[cfg(all(unix, not(target_os = "linux")))]
pub fn query(_guid: &str) -> Option<WirelessInfo> {
    None
}

/// Linux：经 `iw dev <if> link` 取当前 SSID。
#[cfg(target_os = "linux")]
pub fn ssid_of(iface: &str) -> Option<String> {
    let out = run_iw(&["dev", iface, "link"])?;
    linux::parse_iw_link(&out).and_then(|l| l.ssid)
}

#[cfg(all(unix, not(target_os = "linux")))]
pub fn ssid_of(_iface: &str) -> Option<String> {
    None
}

/// 跑 `iw <args>`，返回 stdout；命令缺失/失败 → None。
#[cfg(target_os = "linux")]
fn run_iw(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new("iw").args(args).output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        None
    }
}

/// Linux 无线信息：解析 `iw` 输出的纯函数。平台无关，始终编译。
pub(crate) mod linux {
    #![allow(dead_code)]

    /// `iw dev <if> link` 解析结果（缺字段为 None）。
    #[derive(Debug, Default, Clone)]
    pub struct IwLink {
        pub ssid: Option<String>,
        pub bssid: Option<String>,
        pub signal_dbm: Option<i32>,
        pub freq_mhz: Option<u32>,
        pub tx_mbps: Option<u32>,
        pub rx_mbps: Option<u32>,
    }

    /// 解析 `iw dev <if> link`。"Not connected" → None。
    pub fn parse_iw_link(out: &str) -> Option<IwLink> {
        if out.contains("Not connected") {
            return None;
        }
        let mut l = IwLink::default();
        for line in out.lines() {
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("Connected to ") {
                l.bssid = rest.split_whitespace().next().map(|s| s.to_string());
            } else if let Some(v) = t.strip_prefix("SSID: ") {
                l.ssid = Some(v.trim().to_string());
            } else if let Some(v) = t.strip_prefix("freq: ") {
                l.freq_mhz = v.trim().parse().ok();
            } else if let Some(v) = t.strip_prefix("signal: ") {
                l.signal_dbm = v.split_whitespace().next().and_then(|s| s.parse().ok());
            } else if let Some(v) = t.strip_prefix("tx bitrate: ") {
                l.tx_mbps = parse_bitrate_mbps(v);
            } else if let Some(v) = t.strip_prefix("rx bitrate: ") {
                l.rx_mbps = parse_bitrate_mbps(v);
            }
        }
        if l.bssid.is_some() || l.ssid.is_some() {
            Some(l)
        } else {
            None
        }
    }

    /// "866.7 MBit/s ..." → 866（取整 Mbps）。
    fn parse_bitrate_mbps(s: &str) -> Option<u32> {
        let num = s.split_whitespace().next()?;
        num.parse::<f64>().ok().map(|f| f as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_2g() {
        assert_eq!(band_and_channel(2_437_000), ("2.4 GHz".to_string(), 6));
        assert_eq!(band_and_channel(2_484_000), ("2.4 GHz".to_string(), 14));
    }

    #[test]
    fn channel_5g_6g() {
        assert_eq!(band_and_channel(5_180_000), ("5 GHz".to_string(), 36));
        assert_eq!(band_and_channel(5_955_000), ("6 GHz".to_string(), 1));
        assert_eq!(band_and_channel(1_000_000).0, "-".to_string());
        assert_eq!(band_and_channel(5_895_000), ("5 GHz".to_string(), 179));
        assert_eq!(band_and_channel(7_125_000), ("6 GHz".to_string(), 235));
        assert_eq!(band_and_channel(5_925_000).0, "-".to_string());
    }

    #[test]
    fn phy_labels() {
        assert_eq!(phy_label(8, false).1, 5);
        assert!(phy_label(8, false).0.contains("Wi-Fi 5"));
        assert_eq!(phy_label(10, true).0, "802.11ax · Wi-Fi 6E");
        assert_eq!(phy_label(10, false).1, 6);
        assert_eq!(phy_label(99, false), ("Unknown".to_string(), 0));
    }

    #[test]
    fn rssi_approx() {
        assert_eq!(rssi_from_quality(100), -50);
        assert_eq!(rssi_from_quality(0), -100);
        assert_eq!(rssi_from_quality(50), -75);
    }

    #[test]
    fn labels() {
        assert_eq!(auth_label(7), "WPA2-Personal");
        assert_eq!(cipher_label(4), "CCMP (AES)");
        assert_eq!(auth_label(8), "WPA3-Enterprise");
        assert_eq!(auth_label(9), "WPA3-Personal");
        assert_eq!(auth_label(99), "-");
        assert_eq!(cipher_label(99), "-");
    }

    #[test]
    fn iw_link_parses_fields() {
        let sample = "\
Connected to 11:22:33:44:55:66 (on wlan0)
\tSSID: MyHome-5G
\tfreq: 5180
\tRX: 9999 bytes (12 packets)
\tTX: 8888 bytes (10 packets)
\tsignal: -53 dBm
\ttx bitrate: 866.7 MBit/s
\trx bitrate: 780.0 MBit/s
";
        let p = super::linux::parse_iw_link(sample).expect("should parse");
        assert_eq!(p.ssid.as_deref(), Some("MyHome-5G"));
        assert_eq!(p.bssid.as_deref(), Some("11:22:33:44:55:66"));
        assert_eq!(p.signal_dbm, Some(-53));
        assert_eq!(p.freq_mhz, Some(5180));
        assert_eq!(p.tx_mbps, Some(866));
        assert_eq!(p.rx_mbps, Some(780));
    }

    #[test]
    fn iw_link_not_connected() {
        assert!(super::linux::parse_iw_link("Not connected.\n").is_none());
    }
}
