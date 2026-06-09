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
    } else if (5925..=7125).contains(&mhz) {
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
        8 => "WPA3-Personal",
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
    }
}
