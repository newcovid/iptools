# 链路质量诊断增强 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让诊断页「链路质量」可指定网卡、采集专业有线/无线参数、持续采样 RSSI，并用多维加权模型给出评级。

**Architecture:** 新增 `utils/wlan.rs` 收敛丰富 WLAN 查询；`icmp.rs` 增 `echo_once_from`（`IcmpSendEcho2Ex` 绑定源地址）；`net.rs` 给 `InterfaceInfo` 加链路速率；`link_quality.rs` 重写为网卡选择 + 持续采样 + 多维评分 + 丰富布局，纯函数（评分/标签/信道）拆为可单测单元。

**Tech Stack:** Rust、ratatui 0.28、windows-rs 0.58（WiFi/IpHelper）、ipconfig 0.3、tokio。

参考规格：`docs/superpowers/specs/2026-06-09-link-quality-enhancement-design.md`

**关键约定（务必遵守）：**
- 新增任何 UI 文案，必须同时加进 `assets/locales/en-US.json` 与 `assets/locales/zh-CN.json`，否则 `cargo test` 的 `locale_keys_are_in_sync` 失败。
- 提交信息用中文，并以 `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>` 结尾。
- 每个任务结束后 crate 必须能编译（`cargo check`）。

---

## File Structure

| 文件 | 职责 |
|---|---|
| `src/utils/wlan.rs`（新增） | 纯标签/换算函数（频率→频段+信道、PHY 标签+代际、auth/cipher 标签、RSSI 近似）+ Windows FFI `query(guid)`；`WirelessInfo` 结构。 |
| `src/utils/mod.rs` | 注册 `pub mod wlan;`。 |
| `src/utils/net.rs` | `InterfaceInfo` 增 `link_speed_bps: Option<u64>` 并填充。 |
| `src/modules/diagnostics/icmp.rs` | 新增 `echo_once_from`（源绑定）。 |
| `src/modules/diagnostics/link_quality.rs` | 底部新增 `mod score`（纯评分函数+测试）；其余整体重写。 |
| `assets/locales/en-US.json` / `zh-CN.json` | 新增文案 key。 |

---

## Task 1: `InterfaceInfo` 增加链路速率字段

**Files:**
- Modify: `src/utils/net.rs`

`InterfaceInfo` 只在 `net.rs` 内构造（已确认全仓仅此一处 `InterfaceInfo {`），故加字段只需改结构体定义、Windows 构造处；非 Windows 分支返回空 Vec 无需改。

- [ ] **Step 1: 结构体加字段**

在 `src/utils/net.rs` 的 `pub struct InterfaceInfo { ... }` 末尾 `guid: String,` 之后加一行：

```rust
    /// 协商链路速率（bit/s）；取自 ipconfig 的 TransmitLinkSpeed。非 Windows 为 None。
    pub link_speed_bps: Option<u64>,
```

- [ ] **Step 2: Windows 构造处填充**

在 `src/utils/net.rs` 的 `result.push(InterfaceInfo { ... })` 内，`guid: adapter.adapter_name().to_string(),` 之后加一行：

```rust
                    link_speed_bps: {
                        let s = adapter.transmit_link_speed();
                        if s == 0 || s == u64::MAX { None } else { Some(s) }
                    },
```

- [ ] **Step 3: 编译验证**

Run: `cargo check`
Expected: 通过（无错误；可能有既有 warning）。

- [ ] **Step 4: Commit**

```bash
git add src/utils/net.rs
git commit -m "feat(net): InterfaceInfo 增加协商链路速率字段 link_speed_bps" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 2: `wlan.rs` 纯换算/标签函数（TDD）

**Files:**
- Create: `src/utils/wlan.rs`
- Modify: `src/utils/mod.rs`

这些函数不依赖 windows API，可跨平台编译与单测；FFI 部分在 Task 3 加。

- [ ] **Step 1: 注册模块**

在 `src/utils/mod.rs` 中加入（与其他 `pub mod` 并列，按字母序就近放置）：

```rust
pub mod wlan;
```

- [ ] **Step 2: 写 `wlan.rs` 结构体 + 纯函数 + 失败测试**

创建 `src/utils/wlan.rs`，内容如下（先只放结构体、纯函数与测试；FFI 在 Task 3 追加）：

```rust
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
```

- [ ] **Step 3: 跑测试确认通过**

Run: `cargo test --lib wlan`
Expected: 5 个测试全部 PASS。

- [ ] **Step 4: Commit**

```bash
git add src/utils/mod.rs src/utils/wlan.rs
git commit -m "feat(wlan): 无线信息纯换算/标签函数（频段信道/PHY/认证/RSSI）+ 单测" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 3: `wlan.rs` Windows FFI 查询

**Files:**
- Modify: `src/utils/wlan.rs`

WLAN 查询是 unsafe FFI，无法单测；以 `cargo check` + 后续手动验证为准。参照 `net.rs::get_ssid_map_via_win32` 的句柄管理。

- [ ] **Step 1: 追加 FFI `query` 与非 Windows stub**

在 `src/utils/wlan.rs` 末尾（`#[cfg(test)] mod tests` **之前**）追加：

```rust
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

#[cfg(not(target_os = "windows"))]
pub fn query(_guid: &str) -> Option<WirelessInfo> {
    None
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check`
Expected: 通过。若报某常量/类型未导出，按 windows 0.58 `Win32::NetworkManagement::WiFi` 模块路径修正 `use`（参考 `net.rs` 已用的同模块导入）。

- [ ] **Step 3: Commit**

```bash
git add src/utils/wlan.rs
git commit -m "feat(wlan): Windows WLAN 查询 query()（当前关联 + BSS RSSI/频率）" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 4: `icmp.rs` 源绑定探测 `echo_once_from`

**Files:**
- Modify: `src/modules/diagnostics/icmp.rs`

- [ ] **Step 1: 追加 `echo_once_from`（Windows）与 stub**

在 `src/modules/diagnostics/icmp.rs` 中，现有 `#[cfg(target_os = "windows")] pub fn echo_once(...)` 函数 **之后** 追加：

```rust
/// 从指定源地址 `src` 发送一个 ICMP Echo（绑定出口网卡），载荷 `payload_len` 字节。
/// 用于链路质量按网卡测量。复用 `IcmpSendEcho2Ex` 的 SourceAddress 参数。
#[cfg(target_os = "windows")]
pub fn echo_once_from(
    src: Ipv4Addr,
    dest: Ipv4Addr,
    ttl: u8,
    timeout_ms: u32,
    payload_len: usize,
) -> EchoResult {
    use std::ffi::c_void;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::NetworkManagement::IpHelper::{
        IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho2Ex, ICMP_ECHO_REPLY, IP_OPTION_INFORMATION,
    };

    let src_u32 = u32::from_le_bytes(src.octets());
    let dest_u32 = u32::from_le_bytes(dest.octets());
    let payload = vec![0u8; payload_len.min(1472)];
    const REPLY_SIZE: usize = 2048 + 65535;

    let handle = match unsafe { IcmpCreateFile() } {
        Ok(h) => h,
        Err(_) => {
            return EchoResult {
                status: u32::MAX,
                addr: None,
                rtt_ms: None,
            }
        }
    };

    let opts = IP_OPTION_INFORMATION {
        Ttl: ttl,
        Tos: 0,
        Flags: 0,
        OptionsSize: 0,
        OptionsData: std::ptr::null_mut(),
    };
    let mut reply_buffer = vec![0u8; REPLY_SIZE];

    let count = unsafe {
        IcmpSendEcho2Ex(
            handle,
            HANDLE::default(),
            None,
            None,
            src_u32,
            dest_u32,
            payload.as_ptr() as *const c_void,
            payload.len() as u16,
            Some(&opts as *const IP_OPTION_INFORMATION),
            reply_buffer.as_mut_ptr() as *mut c_void,
            REPLY_SIZE as u32,
            timeout_ms,
        )
    };

    unsafe {
        let _ = IcmpCloseHandle(handle);
    }

    if count == 0 {
        return EchoResult {
            status: IP_REQ_TIMED_OUT,
            addr: None,
            rtt_ms: None,
        };
    }

    let reply = unsafe { &*(reply_buffer.as_ptr() as *const ICMP_ECHO_REPLY) };
    let status = reply.Status;
    if status == IP_SUCCESS || status == IP_TTL_EXPIRED_TRANSIT {
        let o = reply.Address.to_le_bytes();
        EchoResult {
            status,
            addr: Some(Ipv4Addr::new(o[0], o[1], o[2], o[3])),
            rtt_ms: Some(reply.RoundTripTime as u64),
        }
    } else {
        EchoResult {
            status,
            addr: None,
            rtt_ms: None,
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn echo_once_from(
    _src: Ipv4Addr,
    _dest: Ipv4Addr,
    _ttl: u8,
    _timeout_ms: u32,
    _payload_len: usize,
) -> EchoResult {
    EchoResult {
        status: u32::MAX,
        addr: None,
        rtt_ms: None,
    }
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check`
Expected: 通过。若 `event` 参数类型不接受 `HANDLE::default()`，改传 `None`（视 windows 0.58 对 `Param<HANDLE>` 的实现）；`cargo check` 报错信息会直接指明期望类型。

- [ ] **Step 3: Commit**

```bash
git add src/modules/diagnostics/icmp.rs
git commit -m "feat(icmp): echo_once_from 源地址绑定探测（IcmpSendEcho2Ex）" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 5: 评分纯函数 `mod score`（TDD）

**Files:**
- Modify: `src/modules/diagnostics/link_quality.rs`（仅在文件末尾追加 `mod score`，暂不动其余代码）

- [ ] **Step 1: 在文件末尾追加 `mod score` + 测试**

在 `src/modules/diagnostics/link_quality.rs` 最末尾追加：

```rust
/// 多维评分纯函数：各维度映射为 0-100，加权汇总，划定评级。与 UI 解耦，便于单测。
pub(super) mod score {
    use super::Grade;

    /// 线性映射并夹紧到 0..100：value=best→100，value=worst→0（best/worst 大小关系任意）。
    pub fn lerp_score(value: f64, best: f64, worst: f64) -> f64 {
        if (best - worst).abs() < f64::EPSILON {
            return 0.0;
        }
        let t = (value - worst) / (best - worst);
        t.clamp(0.0, 1.0) * 100.0
    }

    pub fn latency_score(avg_ms: f64) -> f64 {
        lerp_score(avg_ms, 20.0, 300.0)
    }
    pub fn jitter_score(jitter_ms: f64) -> f64 {
        lerp_score(jitter_ms, 2.0, 80.0)
    }
    pub fn loss_score(loss_pct: f64) -> f64 {
        lerp_score(loss_pct, 0.0, 10.0)
    }
    pub fn signal_score(rssi_dbm: f64) -> f64 {
        lerp_score(rssi_dbm, -50.0, -85.0)
    }
    pub fn rate_score(mbps: f64) -> f64 {
        lerp_score(mbps, 433.0, 6.0)
    }
    pub fn phy_score(wifi_gen: u8) -> f64 {
        match wifi_gen {
            7 | 6 => 100.0,
            5 => 80.0,
            4 => 60.0,
            _ => 30.0,
        }
    }

    /// 加权汇总：dims 为 (子评分, 权重)。返回 0-100。
    pub fn overall(dims: &[(f64, f64)]) -> f64 {
        let wsum: f64 = dims.iter().map(|(_, w)| w).sum();
        if wsum <= 0.0 {
            return 0.0;
        }
        dims.iter().map(|(s, w)| s * w).sum::<f64>() / wsum
    }

    pub fn grade_from_score(s: f64) -> Grade {
        if s >= 85.0 {
            Grade::Excellent
        } else if s >= 70.0 {
            Grade::Good
        } else if s >= 50.0 {
            Grade::Fair
        } else {
            Grade::Poor
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn linear_endpoints_and_mid() {
            assert!((latency_score(20.0) - 100.0).abs() < 1e-6);
            assert!((latency_score(300.0) - 0.0).abs() < 1e-6);
            assert!((latency_score(160.0) - 50.0).abs() < 1.0);
            assert!((loss_score(0.0) - 100.0).abs() < 1e-6);
            assert!((loss_score(10.0) - 0.0).abs() < 1e-6);
            assert!((signal_score(-50.0) - 100.0).abs() < 1e-6);
            assert!((signal_score(-85.0) - 0.0).abs() < 1e-6);
            assert!((rate_score(433.0) - 100.0).abs() < 1e-6);
            assert!((rate_score(6.0) - 0.0).abs() < 1e-6);
        }

        #[test]
        fn phy_tiers() {
            assert_eq!(phy_score(6), 100.0);
            assert_eq!(phy_score(5), 80.0);
            assert_eq!(phy_score(4), 60.0);
            assert_eq!(phy_score(0), 30.0);
        }

        #[test]
        fn weighted_and_grade() {
            // 全满 → Excellent
            let dims = [(100.0, 40.0), (100.0, 35.0), (100.0, 25.0)];
            let o = overall(&dims);
            assert!((o - 100.0).abs() < 1e-6);
            assert_eq!(grade_from_score(o), Grade::Excellent);
            // 分级边界
            assert_eq!(grade_from_score(86.0), Grade::Excellent);
            assert_eq!(grade_from_score(72.0), Grade::Good);
            assert_eq!(grade_from_score(55.0), Grade::Fair);
            assert_eq!(grade_from_score(40.0), Grade::Poor);
        }
    }
}
```

> 注：`Grade` 已在文件上方定义且含 `#[derive(PartialEq, Eq)]`，可直接用于断言。

- [ ] **Step 2: 跑测试确认通过**

Run: `cargo test --lib score`
Expected: 3 个测试 PASS（其余既有代码不受影响）。

- [ ] **Step 3: Commit**

```bash
git add src/modules/diagnostics/link_quality.rs
git commit -m "feat(link): 多维评分纯函数 mod score + 单测" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 6: i18n 新增文案

**Files:**
- Modify: `assets/locales/en-US.json`
- Modify: `assets/locales/zh-CN.json`

在 Task 7 重写 `link_quality.rs` 前先补齐文案，避免运行期出现 `MISSING:`。

- [ ] **Step 1: 在 en-US.json 加入新 key**

在 `assets/locales/en-US.json` 既有 `diag_link_*` 段内（`"diag_link_unsupported"` 行附近）加入以下键（注意 JSON 逗号）：

```json
    "diag_link_interface": "Interface",
    "diag_link_interval": "Interval (ms)",
    "diag_link_packet": "Packet Size (B)",
    "diag_link_no_iface": "No active interface",
    "diag_link_min": "Min",
    "diag_link_max": "Max",
    "diag_link_score": "Score",
    "diag_link_dim_latency": "Latency",
    "diag_link_dim_jitter": "Jitter",
    "diag_link_dim_loss": "Loss",
    "diag_link_dim_signal": "Signal",
    "diag_link_dim_rate": "Rate",
    "diag_link_dim_phy": "Standard",
    "diag_link_weakest": "Weakest",
    "diag_link_speed": "Link Speed",
    "diag_link_media": "Media",
    "diag_link_media_up": "Connected",
    "diag_link_media_down": "Disconnected",
    "diag_link_ssid": "SSID",
    "diag_link_bssid": "BSSID",
    "diag_link_signal_q": "Signal Quality",
    "diag_link_rssi": "RSSI",
    "diag_link_phy": "PHY",
    "diag_link_band": "Band",
    "diag_link_channel": "Channel",
    "diag_link_rate_tx": "Tx Rate",
    "diag_link_rate_rx": "Rx Rate",
    "diag_link_auth": "Security",
    "diag_link_wifi_na": "Wireless info unavailable",
    "diag_link_rssi_history": "RSSI (dBm)"
```

- [ ] **Step 2: 在 zh-CN.json 加入对应中文 key**

在 `assets/locales/zh-CN.json` 对应段加入（key 必须与英文完全一致）：

```json
    "diag_link_interface": "网卡",
    "diag_link_interval": "间隔 (ms)",
    "diag_link_packet": "包大小 (字节)",
    "diag_link_no_iface": "无可用网卡",
    "diag_link_min": "最小",
    "diag_link_max": "最大",
    "diag_link_score": "得分",
    "diag_link_dim_latency": "延迟",
    "diag_link_dim_jitter": "抖动",
    "diag_link_dim_loss": "丢包",
    "diag_link_dim_signal": "信号",
    "diag_link_dim_rate": "速率",
    "diag_link_dim_phy": "制式",
    "diag_link_weakest": "短板",
    "diag_link_speed": "链路速率",
    "diag_link_media": "媒体状态",
    "diag_link_media_up": "已连接",
    "diag_link_media_down": "未连接",
    "diag_link_ssid": "SSID",
    "diag_link_bssid": "BSSID",
    "diag_link_signal_q": "信号质量",
    "diag_link_rssi": "RSSI",
    "diag_link_phy": "制式",
    "diag_link_band": "频段",
    "diag_link_channel": "信道",
    "diag_link_rate_tx": "发送速率",
    "diag_link_rate_rx": "接收速率",
    "diag_link_auth": "加密",
    "diag_link_wifi_na": "无线信息不可用",
    "diag_link_rssi_history": "RSSI (dBm)"
```

- [ ] **Step 3: 跑语言包一致性测试**

Run: `cargo test locale`
Expected: `locale_keys_are_in_sync` PASS（两份 JSON key 完全一致）。

- [ ] **Step 4: Commit**

```bash
git add assets/locales/en-US.json assets/locales/zh-CN.json
git commit -m "feat(i18n): 链路质量增强相关文案（网卡/维度/无线参数）" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 7: 重写 `link_quality.rs`（网卡选择 + 采样 + 评分 + 布局）

**Files:**
- Modify: `src/modules/diagnostics/link_quality.rs`（替换文件中 **`mod score` 之上** 的全部内容；`mod score` 保留在文件末尾）

把文件顶部到 `detect_medium` 结束的全部代码替换为下面内容。**保留** Task 5 追加的 `pub(super) mod score { ... }` 块在文件最末尾不动。

- [ ] **Step 1: 替换主体代码**

将 `link_quality.rs` 中从第 1 行起、直到原 `detect_medium` 函数结尾（即 `mod score` 之前的所有内容）整体替换为：

```rust
//! 链路质量评测（有线/无线）。
//!
//! 可选定具体网卡，探测从该网卡源 IP 发出（IcmpSendEcho2Ex）；测试期间持续采样
//! 延迟与无线射频状态（RSSI/信号质量/速率），按多维加权模型给出评级。

use super::{config_field_item, FocusArea};
use crate::keymap::Action;
use crate::ui::theme;
use crate::utils::i18n::I18n;
use crate::utils::textinput::{filter_host, TextInput};
use crate::utils::wlan::{self, WirelessInfo};
use crate::utils::{format, net};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph, Sparkline},
};
use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// 一轮探测采样：延迟 + 无线动态字段（仅无线时有值）。
#[derive(Debug, Clone, Copy)]
struct Sample {
    latency_ms: Option<u64>,
    rssi_dbm: Option<i32>,
    quality: Option<u32>,
}

#[derive(Debug)]
enum LinkEvent {
    Sample(Sample),
    Done,
    /// i18n 键
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grade {
    Excellent,
    Good,
    Fair,
    Poor,
}

impl Grade {
    fn i18n_key(self) -> &'static str {
        match self {
            Grade::Excellent => "diag_link_grade_excellent",
            Grade::Good => "diag_link_grade_good",
            Grade::Fair => "diag_link_grade_fair",
            Grade::Poor => "diag_link_grade_poor",
        }
    }
    fn color(self) -> Color {
        match self {
            Grade::Excellent => Color::Green,
            Grade::Good => Color::Cyan,
            Grade::Fair => Color::Yellow,
            Grade::Poor => Color::Red,
        }
    }
}

/// 一块可选网卡。
#[derive(Debug, Clone)]
struct IfaceChoice {
    name: String,
    ipv4: Ipv4Addr,
    guid: String,
    is_wifi: bool,
    link_speed_bps: Option<u64>,
    mac: String,
}

#[derive(Debug, Clone)]
struct LinkConfig {
    target: TextInput,
    count: TextInput,
    interval_ms: TextInput,
    timeout_ms: TextInput,
    packet_size: TextInput,
}

impl Default for LinkConfig {
    fn default() -> Self {
        Self {
            target: TextInput::with_text("8.8.8.8"),
            count: TextInput::with_text("20"),
            interval_ms: TextInput::with_text("200"),
            timeout_ms: TextInput::with_text("1000"),
            packet_size: TextInput::with_text("32"),
        }
    }
}

/// 开始测试时对所选网卡静态信息的快照。
#[derive(Debug, Clone)]
struct LinkSnapshot {
    iface_name: String,
    is_wifi: bool,
    link_speed_bps: Option<u64>,
    mac: String,
    ipv4: Ipv4Addr,
    wireless: Option<WirelessInfo>,
}

const N_FIELDS: usize = 6; // 0=网卡选择器 + 5 文本字段

pub struct LinkQualityTool {
    config: LinkConfig,
    config_state: ListState,
    ifaces: Vec<IfaceChoice>,
    iface_idx: usize,

    running: bool,
    error_key: Option<String>,

    samples: Vec<Sample>,
    lat_history: VecDeque<u64>,
    rssi_history: VecDeque<u64>, // 存 (rssi+100) 便于 sparkline 的 u64
    total: u64,

    snapshot: Option<LinkSnapshot>,

    tx: mpsc::Sender<LinkEvent>,
    rx: mpsc::Receiver<LinkEvent>,
    abort_flag: Arc<Mutex<bool>>,
}

impl LinkQualityTool {
    pub fn new() -> Self {
        let mut config_state = ListState::default();
        config_state.select(Some(0));
        let (tx, rx) = mpsc::channel(128);
        let mut s = Self {
            config: LinkConfig::default(),
            config_state,
            ifaces: Vec::new(),
            iface_idx: 0,
            running: false,
            error_key: None,
            samples: Vec::new(),
            lat_history: VecDeque::with_capacity(100),
            rssi_history: VecDeque::with_capacity(100),
            total: 0,
            snapshot: None,
            tx,
            rx,
            abort_flag: Arc::new(Mutex::new(false)),
        };
        s.refresh_ifaces();
        s
    }

    /// 重新枚举可选网卡（活跃物理网卡且有 IPv4），并夹紧当前选择。
    fn refresh_ifaces(&mut self) {
        let mut choices = Vec::new();
        for i in net::get_interfaces() {
            if !(i.is_up && i.is_physical && !i.ipv4.is_empty()) {
                continue;
            }
            let ipv4 = i
                .ipv4
                .iter()
                .find_map(|s| s.parse::<Ipv4Addr>().ok());
            let ipv4 = match ipv4 {
                Some(v) => v,
                None => continue,
            };
            let is_wifi = i.interface_type.contains("Ieee80211") || i.ssid.is_some();
            choices.push(IfaceChoice {
                name: i.name.clone(),
                ipv4,
                guid: i.guid.clone(),
                is_wifi,
                link_speed_bps: i.link_speed_bps,
                mac: i.mac.clone(),
            });
        }
        self.ifaces = choices;
        if self.ifaces.is_empty() {
            self.iface_idx = 0;
        } else if self.iface_idx >= self.ifaces.len() {
            self.iface_idx = self.ifaces.len() - 1;
        }
    }

    pub fn update(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                LinkEvent::Sample(s) => {
                    if let Some(l) = s.latency_ms {
                        push_cap(&mut self.lat_history, l, 100);
                    }
                    if let Some(r) = s.rssi_dbm {
                        push_cap(&mut self.rssi_history, (r + 100).max(0) as u64, 100);
                    }
                    self.samples.push(s);
                }
                LinkEvent::Done => self.running = false,
                LinkEvent::Error(key) => {
                    self.error_key = Some(key);
                    self.running = false;
                }
            }
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, action: Option<Action>, focus: FocusArea) {
        match focus {
            FocusArea::Main => {
                if action == Some(Action::Toggle) {
                    if self.running {
                        self.stop();
                    } else {
                        self.start();
                    }
                }
            }
            FocusArea::Config => self.handle_config_key(key, action),
            _ => {}
        }
    }

    fn handle_config_key(&mut self, key: KeyEvent, action: Option<Action>) {
        let idx = self.config_state.selected().unwrap_or(0);

        // 网卡选择器：Left/Right 循环切换
        if idx == 0 && !self.running {
            match action {
                Some(Action::Left) => {
                    self.refresh_ifaces();
                    if !self.ifaces.is_empty() {
                        self.iface_idx =
                            (self.iface_idx + self.ifaces.len() - 1) % self.ifaces.len();
                    }
                    return;
                }
                Some(Action::Right) => {
                    self.refresh_ifaces();
                    if !self.ifaces.is_empty() {
                        self.iface_idx = (self.iface_idx + 1) % self.ifaces.len();
                    }
                    return;
                }
                _ => {}
            }
        }

        // 文本字段（idx 1..=5）：带光标编辑
        if idx >= 1 && !self.running {
            let too_long =
                matches!(key.code, KeyCode::Char(_)) && self.field_mut(idx).value().len() >= 64;
            if !too_long {
                let consumed = if idx == 1 {
                    self.field_mut(idx).handle_key(key.code, filter_host)
                } else {
                    self.field_mut(idx)
                        .handle_key(key.code, |c| c.is_ascii_digit())
                };
                if consumed {
                    return;
                }
            }
        }

        match action {
            Some(Action::Down) => self.next_config(),
            Some(Action::Up) => self.prev_config(),
            _ => {}
        }
    }

    fn field_mut(&mut self, idx: usize) -> &mut TextInput {
        match idx {
            1 => &mut self.config.target,
            2 => &mut self.config.count,
            3 => &mut self.config.interval_ms,
            4 => &mut self.config.timeout_ms,
            _ => &mut self.config.packet_size,
        }
    }

    fn next_config(&mut self) {
        let i = self
            .config_state
            .selected()
            .map(|i| (i + 1) % N_FIELDS)
            .unwrap_or(0);
        self.config_state.select(Some(i));
    }

    fn prev_config(&mut self) {
        let i = self
            .config_state
            .selected()
            .map(|i| if i == 0 { N_FIELDS - 1 } else { i - 1 })
            .unwrap_or(0);
        self.config_state.select(Some(i));
    }

    /// 连通性统计 (已发, 已收, 丢包率%, min, avg, max, 抖动)。
    fn stats(&self) -> (u64, u64, f64, u64, u64, u64, u64) {
        let sent = self.samples.len() as u64;
        let lat: Vec<u64> = self.samples.iter().filter_map(|s| s.latency_ms).collect();
        let recv = lat.len() as u64;
        let loss = if sent > 0 {
            ((sent - recv) as f64 / sent as f64) * 100.0
        } else {
            0.0
        };
        let avg = if recv > 0 {
            lat.iter().sum::<u64>() / recv
        } else {
            0
        };
        let min = lat.iter().copied().min().unwrap_or(0);
        let max = lat.iter().copied().max().unwrap_or(0);
        let jitter = if lat.len() > 1 {
            let s: u64 = lat.windows(2).map(|w| w[1].abs_diff(w[0])).sum();
            s / (lat.len() as u64 - 1)
        } else {
            0
        };
        (sent, recv, loss, min, avg, max, jitter)
    }

    /// 无线 RSSI 统计 (min, avg, max)；无样本时返回 None。
    fn rssi_stats(&self) -> Option<(i32, i32, i32)> {
        let v: Vec<i32> = self.samples.iter().filter_map(|s| s.rssi_dbm).collect();
        if v.is_empty() {
            return None;
        }
        let min = *v.iter().min().unwrap();
        let max = *v.iter().max().unwrap();
        let avg = (v.iter().sum::<i32>()) / (v.len() as i32);
        Some((min, avg, max))
    }

    /// 计算各维度 (标签 i18n key, 子评分, 权重) 列表。
    fn dimensions(&self) -> Vec<(&'static str, f64, f64)> {
        let (_, _, loss, _, avg, _, jitter) = self.stats();
        let latency = score::latency_score(avg as f64);
        let jit = score::jitter_score(jitter as f64);
        let los = score::loss_score(loss);

        let is_wifi = self.snapshot.as_ref().map(|s| s.is_wifi).unwrap_or(false);
        if is_wifi {
            let (sig, rate, phy) = self.wifi_dim_scores();
            vec![
                ("diag_link_dim_loss", los, 25.0),
                ("diag_link_dim_latency", latency, 20.0),
                ("diag_link_dim_jitter", jit, 15.0),
                ("diag_link_dim_signal", sig, 25.0),
                ("diag_link_dim_rate", rate, 10.0),
                ("diag_link_dim_phy", phy, 5.0),
            ]
        } else {
            vec![
                ("diag_link_dim_loss", los, 40.0),
                ("diag_link_dim_latency", latency, 35.0),
                ("diag_link_dim_jitter", jit, 25.0),
            ]
        }
    }

    /// 无线三个维度子评分：信号(用采样 RSSI 均值)、速率(协商 Tx)、制式(代际)。
    fn wifi_dim_scores(&self) -> (f64, f64, f64) {
        let w = self.snapshot.as_ref().and_then(|s| s.wireless.as_ref());
        let rssi_avg = self
            .rssi_stats()
            .map(|(_, a, _)| a as f64)
            .or_else(|| w.map(|w| w.rssi_dbm as f64))
            .unwrap_or(-100.0);
        let sig = score::signal_score(rssi_avg);
        let rate = score::rate_score(w.map(|w| w.tx_rate_mbps as f64).unwrap_or(0.0));
        let phy = score::phy_score(w.map(|w| w.wifi_gen).unwrap_or(0));
        (sig, rate, phy)
    }

    fn overall_grade(&self) -> Option<(f64, Grade, usize)> {
        let (sent, recv, _, _, _, _, _) = self.stats();
        if sent == 0 || recv == 0 {
            return None;
        }
        let dims = self.dimensions();
        let pairs: Vec<(f64, f64)> = dims.iter().map(|(_, s, w)| (*s, *w)).collect();
        let o = score::overall(&pairs);
        // 最弱维度索引
        let weakest = dims
            .iter()
            .enumerate()
            .min_by(|a, b| a.1 .1.partial_cmp(&b.1 .1).unwrap())
            .map(|(i, _)| i)
            .unwrap_or(0);
        Some((o, score::grade_from_score(o), weakest))
    }

    fn start(&mut self) {
        self.refresh_ifaces();
        let iface = match self.ifaces.get(self.iface_idx) {
            Some(c) => c.clone(),
            None => {
                self.error_key = Some("diag_link_no_iface".to_string());
                return;
            }
        };

        let count: u64 = self.config.count.value().parse().unwrap_or(20).clamp(5, 100);
        let interval_ms: u64 = self
            .config
            .interval_ms
            .value()
            .parse()
            .unwrap_or(200)
            .clamp(50, 5000);
        let timeout_ms: u32 = self
            .config
            .timeout_ms
            .value()
            .parse()
            .unwrap_or(1000)
            .clamp(100, 10000);
        let packet_size: usize = self
            .config
            .packet_size
            .value()
            .parse()
            .unwrap_or(32)
            .clamp(0, 1472);
        let target = self.config.target.value().trim().to_string();
        if target.is_empty() {
            self.error_key = Some("diag_link_err".to_string());
            return;
        }

        // 静态快照：无线则查一次完整无线信息
        let wireless = if iface.is_wifi {
            wlan::query(&iface.guid)
        } else {
            None
        };
        self.snapshot = Some(LinkSnapshot {
            iface_name: iface.name.clone(),
            is_wifi: iface.is_wifi,
            link_speed_bps: iface.link_speed_bps,
            mac: iface.mac.clone(),
            ipv4: iface.ipv4,
            wireless,
        });

        self.running = true;
        self.error_key = None;
        self.samples.clear();
        self.lat_history.clear();
        self.rssi_history.clear();
        self.total = count;
        *self.abort_flag.lock().unwrap() = false;

        let tx = self.tx.clone();
        let abort = self.abort_flag.clone();
        let src = iface.ipv4;
        let guid = iface.guid.clone();
        let is_wifi = iface.is_wifi;

        tokio::spawn(async move {
            let dest: Ipv4Addr = match resolve_v4(&target).await {
                Some(v) => v,
                None => {
                    let _ = tx.send(LinkEvent::Error("diag_link_err".into())).await;
                    return;
                }
            };

            for i in 0..count {
                if *abort.lock().unwrap() {
                    return;
                }
                let res = match tokio::task::spawn_blocking(move || {
                    super::icmp::echo_once_from(src, dest, 128, timeout_ms, packet_size)
                })
                .await
                {
                    Ok(r) => r,
                    Err(_) => return,
                };

                if res.status == u32::MAX {
                    let _ = tx
                        .send(LinkEvent::Error("diag_link_unsupported".into()))
                        .await;
                    return;
                }

                let latency = if res.reached() { res.rtt_ms } else { None };

                // 无线动态采样
                let (rssi, quality) = if is_wifi {
                    let g = guid.clone();
                    match tokio::task::spawn_blocking(move || wlan::query(&g)).await {
                        Ok(Some(w)) => (Some(w.rssi_dbm), Some(w.signal_quality)),
                        _ => (None, None),
                    }
                } else {
                    (None, None)
                };

                let _ = tx
                    .send(LinkEvent::Sample(Sample {
                        latency_ms: latency,
                        rssi_dbm: rssi,
                        quality,
                    }))
                    .await;

                if i + 1 < count {
                    tokio::time::sleep(Duration::from_millis(interval_ms)).await;
                }
            }
            let _ = tx.send(LinkEvent::Done).await;
        });
    }

    fn stop(&mut self) {
        self.running = false;
        *self.abort_flag.lock().unwrap() = true;
    }

    // -------------------------------------------------------------------------
    // 绘图
    // -------------------------------------------------------------------------

    pub fn draw(
        &mut self,
        f: &mut Frame,
        main_area: Rect,
        config_area: Rect,
        i18n: &I18n,
        is_focused: bool,
        active_focus: FocusArea,
    ) {
        self.draw_main(f, main_area, i18n, is_focused, active_focus);
        self.draw_config(f, config_area, i18n, is_focused, active_focus);
    }

    fn draw_main(
        &self,
        f: &mut Frame,
        area: Rect,
        i18n: &I18n,
        is_focused: bool,
        active_focus: FocusArea,
    ) {
        let color = if is_focused && active_focus == FocusArea::Main {
            Color::Yellow
        } else {
            Color::Gray
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(i18n.t("diag_main_title"))
            .border_style(Style::default().fg(color));
        let inner = block.inner(area);
        f.render_widget(block, area);

        if !is_focused {
            let p = Paragraph::new(i18n.t("diag_msg_focus_hint"))
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(p, inner);
            return;
        }

        let is_wifi = self.snapshot.as_ref().map(|s| s.is_wifi).unwrap_or(false);
        let n_dims = if is_wifi { 6 } else { 3 };
        let rssi_rows = if is_wifi { 3 } else { 0 };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),                 // header
                Constraint::Length(1),                 // overall gauge
                Constraint::Length(n_dims as u16),     // dim bars
                Constraint::Length(4),                 // metrics grid
                Constraint::Min(3),                    // latency sparkline
                Constraint::Length(rssi_rows as u16),  // rssi sparkline (wifi)
                Constraint::Length(1),                 // status
            ])
            .split(inner);

        self.draw_header(f, chunks[0], i18n);
        self.draw_overall(f, chunks[1], i18n);
        self.draw_dim_bars(f, chunks[2], i18n);
        self.draw_metrics(f, chunks[3], i18n, is_wifi);

        let data: Vec<u64> = self.lat_history.iter().cloned().collect();
        let spark = Sparkline::default()
            .block(Block::default().borders(Borders::TOP).title(i18n.t("diag_link_history")))
            .data(&data)
            .style(Style::default().fg(theme::COLOR_PRIMARY));
        f.render_widget(spark, chunks[4]);

        if is_wifi {
            let rdata: Vec<u64> = self.rssi_history.iter().cloned().collect();
            let rspark = Sparkline::default()
                .block(Block::default().borders(Borders::TOP).title(i18n.t("diag_link_rssi_history")))
                .data(&rdata)
                .style(Style::default().fg(Color::Magenta));
            f.render_widget(rspark, chunks[5]);
        }

        self.draw_status(f, chunks[6], i18n);
    }

    fn draw_header(&self, f: &mut Frame, area: Rect, i18n: &I18n) {
        let mut spans = vec![Span::styled(
            format!("{}: ", i18n.t("diag_link_interface")),
            Style::default().fg(Color::Gray),
        )];
        match &self.snapshot {
            Some(s) => {
                let badge = if s.is_wifi {
                    i18n.t("diag_link_wireless")
                } else {
                    i18n.t("diag_link_wired")
                };
                spans.push(Span::styled(
                    format!("{} [{}]", s.iface_name, badge),
                    Style::default().fg(theme::COLOR_SECONDARY),
                ));
                if s.is_wifi {
                    if let Some(w) = &s.wireless {
                        spans.push(Span::styled(
                            format!("  {}", w.ssid),
                            Style::default().fg(Color::White),
                        ));
                    }
                } else if let Some(sp) = s.link_speed_bps {
                    spans.push(Span::styled(
                        format!("  {}", format::format_speed(sp / 8)),
                        Style::default().fg(Color::White),
                    ));
                }
            }
            None => {
                let name = self
                    .ifaces
                    .get(self.iface_idx)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| i18n.t("diag_link_no_iface"));
                spans.push(Span::styled(name, Style::default().fg(theme::COLOR_SECONDARY)));
            }
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn draw_overall(&self, f: &mut Frame, area: Rect, i18n: &I18n) {
        let (label, ratio, gcolor) = match self.overall_grade() {
            Some((sc, g, _)) => (
                format!("{}: {} ({:.0})", i18n.t("diag_link_grade"), i18n.t(g.i18n_key()), sc),
                (sc / 100.0).clamp(0.0, 1.0),
                g.color(),
            ),
            None => (format!("{}: -", i18n.t("diag_link_grade")), 0.0, Color::DarkGray),
        };
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(gcolor).bg(Color::DarkGray))
            .ratio(ratio)
            .label(label);
        f.render_widget(gauge, area);
    }

    fn draw_dim_bars(&self, f: &mut Frame, area: Rect, i18n: &I18n) {
        let weakest = self.overall_grade().map(|(_, _, w)| w);
        let dims = self.dimensions();
        let lines: Vec<Line> = dims
            .iter()
            .enumerate()
            .map(|(i, (key, sc, _))| {
                let bar = score_bar(*sc, 12);
                let c = bar_color(*sc);
                let mark = if Some(i) == weakest { " ◀" } else { "" };
                Line::from(vec![
                    Span::styled(format!("{:<8}", i18n.t(key)), Style::default().fg(Color::Gray)),
                    Span::styled(bar, Style::default().fg(c)),
                    Span::styled(format!(" {:>3.0}{}", sc, mark), Style::default().fg(c)),
                ])
            })
            .collect();
        f.render_widget(Paragraph::new(lines), area);
    }

    fn draw_metrics(&self, f: &mut Frame, area: Rect, i18n: &I18n, is_wifi: bool) {
        let (sent, _recv, loss, min, avg, max, jitter) = self.stats();
        let g = |k: &str| -> String { i18n.t(k) };
        let mut lines = vec![
            Line::from(vec![
                Span::styled(format!("{}: ", g("diag_link_avg")), Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("{}/{}/{} ms  ", min, avg, max),
                    Style::default().fg(Color::White),
                ),
                Span::styled(format!("{}: ", g("diag_link_jitter")), Style::default().fg(Color::Gray)),
                Span::styled(format!("{} ms", jitter), Style::default().fg(Color::White)),
            ]),
            Line::from(vec![
                Span::styled(format!("{}: ", g("diag_link_loss")), Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("{:.1}%  ", loss),
                    Style::default().fg(if loss > 5.0 { Color::Red } else { Color::Green }),
                ),
                Span::styled(format!("{}: ", g("diag_link_sent")), Style::default().fg(Color::Gray)),
                Span::styled(
                    format!("{}/{}", sent, self.total),
                    Style::default().fg(theme::COLOR_SECONDARY),
                ),
            ]),
        ];

        if is_wifi {
            let w = self.snapshot.as_ref().and_then(|s| s.wireless.as_ref());
            let rssi_txt = match self.rssi_stats() {
                Some((mn, av, mx)) => format!("{}/{}/{} dBm", mn, av, mx),
                None => w.map(|w| format!("{} dBm", w.rssi_dbm)).unwrap_or_else(|| "-".into()),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", g("diag_link_rssi")), Style::default().fg(Color::Gray)),
                Span::styled(format!("{}  ", rssi_txt), Style::default().fg(Color::White)),
                Span::styled(format!("{}: ", g("diag_link_channel")), Style::default().fg(Color::Gray)),
                Span::styled(
                    w.map(|w| format!("{} ({})", w.channel, w.band)).unwrap_or_else(|| "-".into()),
                    Style::default().fg(Color::White),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", g("diag_link_phy")), Style::default().fg(Color::Gray)),
                Span::styled(
                    w.map(|w| w.phy_type.clone()).unwrap_or_else(|| "-".into()),
                    Style::default().fg(Color::White),
                ),
                Span::styled(format!("  {}/{}: ", g("diag_link_rate_tx"), g("diag_link_rate_rx")), Style::default().fg(Color::Gray)),
                Span::styled(
                    w.map(|w| format!("{}/{} Mbps", w.tx_rate_mbps, w.rx_rate_mbps)).unwrap_or_else(|| "-".into()),
                    Style::default().fg(Color::White),
                ),
            ]));
        } else {
            let s = self.snapshot.as_ref();
            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", g("diag_link_speed")), Style::default().fg(Color::Gray)),
                Span::styled(
                    s.and_then(|s| s.link_speed_bps)
                        .map(|sp| format::format_speed(sp / 8))
                        .unwrap_or_else(|| "-".into()),
                    Style::default().fg(Color::White),
                ),
                Span::styled(format!("  {}: ", g("diag_link_media")), Style::default().fg(Color::Gray)),
                Span::styled(g("diag_link_media_up"), Style::default().fg(Color::Green)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("MAC: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    s.map(|s| s.mac.clone()).unwrap_or_else(|| "-".into()),
                    Style::default().fg(Color::White),
                ),
                Span::styled("  IPv4: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    s.map(|s| s.ipv4.to_string()).unwrap_or_else(|| "-".into()),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        f.render_widget(Paragraph::new(lines), area);
    }

    fn draw_status(&self, f: &mut Frame, area: Rect, i18n: &I18n) {
        let (text, style) = if let Some(key) = &self.error_key {
            (i18n.t(key), Style::default().fg(theme::COLOR_ERROR))
        } else if self.running {
            (
                format!("{} | {}", i18n.t("diag_status_running"), i18n.t("diag_msg_stop")),
                Style::default().fg(Color::Green),
            )
        } else {
            (
                format!("{} | {}", i18n.t("diag_status_stopped"), i18n.t("diag_msg_start")),
                Style::default().fg(Color::Red),
            )
        };
        f.render_widget(Paragraph::new(text).style(style), area);
    }

    fn draw_config(
        &mut self,
        f: &mut Frame,
        area: Rect,
        i18n: &I18n,
        is_focused: bool,
        active_focus: FocusArea,
    ) {
        let color = if is_focused && active_focus == FocusArea::Config {
            Color::Yellow
        } else {
            Color::Gray
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(i18n.t("diag_config_title"))
            .border_style(Style::default().fg(color));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let is_active = is_focused && active_focus == FocusArea::Config;
        let selected = self.config_state.selected();

        // 网卡选择器显示值（只读文本形式复用 config_field_item）
        let iface_display = match self.ifaces.get(self.iface_idx) {
            Some(c) => format!("{} ({})", c.name, c.ipv4),
            None => i18n.t("diag_link_no_iface"),
        };
        let iface_input = TextInput::with_text(&iface_display);

        let labels = [
            i18n.t("diag_link_interface"),
            i18n.t("diag_link_target"),
            i18n.t("diag_link_count"),
            i18n.t("diag_link_interval"),
            i18n.t("diag_link_timeout"),
            i18n.t("diag_link_packet"),
        ];

        let mut items: Vec<ListItem> = Vec::with_capacity(N_FIELDS);
        for i in 0..N_FIELDS {
            let is_sel = selected == Some(i);
            let active = is_sel && is_active && !self.running;
            if i == 0 {
                // 选择器：不显示光标，提示 ←→ 切换
                let hint = if active {
                    Some(i18n.t("diag_hint_switch"))
                } else {
                    None
                };
                items.push(config_field_item(&labels[0], is_sel, is_active, &iface_input, false, hint));
            } else {
                let input = match i {
                    1 => &self.config.target,
                    2 => &self.config.count,
                    3 => &self.config.interval_ms,
                    4 => &self.config.timeout_ms,
                    _ => &self.config.packet_size,
                };
                let hint = if active {
                    Some(if i == 1 {
                        i18n.t("diag_hint_input")
                    } else {
                        i18n.t("diag_hint_digits")
                    })
                } else {
                    None
                };
                items.push(config_field_item(&labels[i], is_sel, is_active, input, active, hint));
            }
        }

        f.render_widget(List::new(items), inner);
    }
}

/// 把值推入定长 ring buffer。
fn push_cap(buf: &mut VecDeque<u64>, v: u64, cap: usize) {
    if buf.len() >= cap {
        buf.pop_front();
    }
    buf.push_back(v);
}

/// 0-100 分数 → 定宽方块条字符串。
fn score_bar(score: f64, width: usize) -> String {
    let filled = ((score / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
}

fn bar_color(score: f64) -> Color {
    if score >= 85.0 {
        Color::Green
    } else if score >= 70.0 {
        Color::Cyan
    } else if score >= 50.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

/// 解析目标为 IPv4（直接 IP 或 DNS 解析取首个 v4）。
async fn resolve_v4(target: &str) -> Option<Ipv4Addr> {
    if let Ok(IpAddr::V4(v4)) = target.parse::<IpAddr>() {
        return Some(v4);
    }
    if let Ok(it) = tokio::net::lookup_host((target, 0u16)).await {
        for sa in it {
            if let IpAddr::V4(v4) = sa.ip() {
                return Some(v4);
            }
        }
    }
    None
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo check`
说明：`format::format_speed(bytes_per_sec: u64) -> String` 已确认；链路速率为 bit/s，故传 `sp / 8`（u64）。其余报错按编译器提示修正（多为 `use` 或借用），尤其留意 windows 0.58 WiFi 模块的导入路径。
Expected: 最终 `cargo check` 通过。

- [ ] **Step 3: 跑全部测试**

Run: `cargo test`
Expected: 全绿（含 `score`、`wlan`、`locale_keys_are_in_sync`）。

- [ ] **Step 4: Commit**

```bash
git add src/modules/diagnostics/link_quality.rs
git commit -m "feat(link): 链路质量重写——网卡选择/持续采样/多维评分/丰富布局" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Task 8: 编译发布构建 + 手动真机验证

**Files:** 无（验证任务）

FFI 与 TUI 无法自动化，需在 Windows（建议管理员权限）多网卡机器手动验证。

- [ ] **Step 1: Release 构建**

Run: `cargo build --release`
Expected: 成功，无错误。

- [ ] **Step 2: 手动验证清单**

Run: `cargo run`，进入 Diagnostics → 链路质量（Link Quality）：

- [ ] Config 栏第一项「网卡」用 ←/→ 可在多块活跃网卡间切换，显示 名称 (IPv4)。
- [ ] 选有线网卡 → 空格开始：表头显示 [有线] + 链路速率；分项条 3 条（丢包/延迟/抖动）；指标栏显示链路速率/媒体/MAC/IPv4；延迟曲线滚动；总评级 gauge 有分数。
- [ ] 选无线网卡 → 开始：表头显示 [无线] + SSID；分项条 6 条（含信号/速率/制式）；指标栏显示 RSSI(min/avg/max)、信道(频段)、PHY、Tx/Rx；出现第二条 RSSI 曲线；最弱维度有 ◀ 标记。
- [ ] 运行中再按空格可停止；目标置空开始报错；无可用网卡时报 “无可用网卡”。
- [ ] 切换语言（Ctrl+L）后所有新文案中英文均正确，无 `MISSING:`。

- [ ] **Step 3: 更新 CLAUDE.md 进度表（如验证通过）**

将 CLAUDE.md 诊断行中链路质量描述更新为支持「网卡选择 + 专业有线/无线参数 + 多维加权评级 + RSSI 持续采样」，并在「待办」记录真机验证结果。提交：

```bash
git add CLAUDE.md
git commit -m "docs(CLAUDE.md): 同步链路质量增强（网卡选择/无线参数/多维评分）" -m "Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Self-Review 结论

- **Spec 覆盖**：网卡选择(T7 config + refresh_ifaces) / 源绑定探测(T4 + start) / 无线参数(T2+T3 + draw_metrics) / 持续采样(start 循环 + rssi_history) / 多维加权评级(T5 + dimensions/overall_grade) / 有线参数(T1 + draw_metrics) / i18n(T6) / 非 Windows stub(T3,T4) / 测试(T2,T5,T6) 均有对应任务。
- **占位符**：无 TBD/TODO；所有代码步骤含完整代码。
- **类型一致性**：`Grade` 在 link_quality.rs 定义、`score::grade_from_score` 返回它、测试引用它；`WirelessInfo` 字段在 T2 定义、T3 填充、T7 读取（`tx_rate_mbps/rx_rate_mbps/rssi_dbm/wifi_gen/channel/band/phy_type/ssid` 一致）；`echo_once_from(src,dest,ttl,timeout_ms,payload_len)` 在 T4 定义、T7 同签名调用；`link_speed_bps` 在 T1 定义、T7 读取。
