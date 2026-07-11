//! 会话参数持久化。
//!
//! 把各页面/各诊断子工具用户输入过的参数（IP/端口/发包间隔/超时/载荷大小/CIDR…）
//! 落进 `config.json` 的 `session` 段，重启后回灌，避免每次都从默认值重来。
//!
//! 设计约定：
//! - 每个工具持有一份运行时状态（`TextInput`/数值），与本文件的 `*Persist`
//!   纯数据结构互转：工具实现 `export_persist()`（导出快照）与 `apply_persist()`
//!   （回灌）。`App` 在每次按键/鼠标后做一次「脏检查」快照对比，仅在值真正变化时
//!   写盘（见 `app.rs::maybe_persist`），既保证「重启不丢」又避免高频磁盘写入。
//! - 所有结构体用容器级 `#[serde(default)]` + 自定义 `Default`，对缺字段/旧配置
//!   逐字段回退到默认值，向后/向前兼容（新增字段不会让旧 `config.json` 解析失败）。
//! - 链路质量「按网卡保存」：`adapters` 以网卡稳定标识（GUID→MAC→名称回退）为键，
//!   存各自整套参数；`selected` 记住上次选中的网卡，重启后按键重新定位并自动载入。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 全部页面/工具的会话参数聚合。挂在 `Config.session` 下随配置文件一并读写。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SessionState {
    pub scanner: ScannerPersist,
    pub ping: PingPersist,
    pub port_scan: PortScanPersist,
    pub trace: TracePersist,
    pub lan_speed: LanSpeedPersist,
    pub link_quality: LinkQualityPersist,
    pub adapter_edit: AdapterEditPersist,
    pub ui: UiPersist,
    pub history: HistoryPersist,
}

/// 界面位置记忆：上次所在标签页 + 诊断子工具，重启回到原处。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct UiPersist {
    /// 标签页索引（0=概览…5=设置，对应 `CurrentTab` 判别值）。
    pub last_tab: u8,
    /// 诊断子工具索引（0=Ping…5=内网测速，对应 `DiagnosticTool` 声明序）。
    pub last_diag_tool: u8,
}

/// 目标历史（MRU）：三个池——IP/主机、CIDR、适配器编辑。最近在前。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct HistoryPersist {
    /// IP/主机历史（ping/trace/端口扫描/链路质量/内网对端共享）。
    pub targets: Vec<String>,
    /// CIDR 历史（扫描页独立）。
    pub cidrs: Vec<String>,
    /// 适配器编辑历史（IP/掩码/网关/DNS 共享）。
    pub adapter: Vec<String>,
}

/// 扫描页：CIDR 网段。**始终为空串**——每次启动重新按活动网卡推断默认值，
/// 用户历史由 MRU 池（`HistoryPersist.cidrs`）独立管理。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ScannerPersist {
    pub cidr: String,
}

/// 诊断 · Ping：目标 + 发包间隔/超时/载荷大小。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PingPersist {
    pub target: String,
    pub interval_ms: u64,
    pub timeout_ms: u64,
    pub packet_size: u64,
}

impl Default for PingPersist {
    fn default() -> Self {
        Self {
            target: "8.8.8.8".to_string(),
            interval_ms: 1000,
            timeout_ms: 2000,
            packet_size: 32,
        }
    }
}

/// 诊断 · 端口扫描：目标 + 起止端口 + 超时（与界面一致，按文本保存）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PortScanPersist {
    pub target: String,
    pub start_port: String,
    pub end_port: String,
    pub timeout_ms: String,
}

impl Default for PortScanPersist {
    fn default() -> Self {
        Self {
            target: "127.0.0.1".to_string(),
            start_port: "1".to_string(),
            end_port: "1024".to_string(),
            timeout_ms: "300".to_string(),
        }
    }
}

/// 诊断 · 路由跟踪：目标 + 最大跳数 + 超时。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TracePersist {
    pub target: String,
    pub max_hops: String,
    pub timeout_ms: String,
}

impl Default for TracePersist {
    fn default() -> Self {
        Self {
            target: "8.8.8.8".to_string(),
            max_hops: "30".to_string(),
            timeout_ms: "1000".to_string(),
        }
    }
}

/// 诊断 · 内网测速：模式（server/client）+ 对端 IP + 端口 + 协议/方向/时长/流数/包大小/速率。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LanSpeedPersist {
    /// "server" 或 "client"。
    pub mode: String,
    pub peer: String,
    pub port: String,
    /// "tcp" 或 "udp"。
    pub proto: String,
    /// "up" / "down" / "bidir"。
    pub direction: String,
    pub duration: String,
    pub streams: String,
    pub payload: String,
    pub rate: String,
}

impl Default for LanSpeedPersist {
    fn default() -> Self {
        Self {
            mode: "server".to_string(),
            peer: String::new(),
            port: "50505".to_string(),
            proto: "tcp".to_string(),
            direction: "up".to_string(),
            duration: "10".to_string(),
            streams: "1".to_string(),
            payload: "65536".to_string(),
            rate: "0".to_string(),
        }
    }
}

/// 链路质量一套参数（按网卡各存一份）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LinkParams {
    pub target: String,
    pub count: String,
    pub interval_ms: String,
    pub timeout_ms: String,
    pub packet_size: String,
}

impl Default for LinkParams {
    fn default() -> Self {
        Self {
            target: "8.8.8.8".to_string(),
            count: "20".to_string(),
            interval_ms: "200".to_string(),
            timeout_ms: "1000".to_string(),
            packet_size: "32".to_string(),
        }
    }
}

/// 诊断 · 链路质量：按网卡键存参数 + 上次选中的网卡键。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LinkQualityPersist {
    /// 网卡稳定标识（GUID→MAC→名称回退）→ 该网卡的一套参数。
    /// 用 BTreeMap 让序列化顺序稳定，避免脏检查产生伪差异。
    pub adapters: BTreeMap<String, LinkParams>,
    /// 上次选中的网卡键；重启后按此键重新定位选中项。
    pub selected: Option<String>,
}

/// 适配器编辑：按网卡保存静态 IP 配置参数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AdapterEditParams {
    /// 是否使用 DHCP（false = 静态 IP）。
    pub use_dhcp: bool,
    /// 静态 IP 地址。
    pub ip: String,
    /// 子网掩码。
    pub mask: String,
    /// 默认网关。
    pub gateway: String,
    /// 首选 DNS。
    pub dns1: String,
    /// 备选 DNS。
    pub dns2: String,
}

impl Default for AdapterEditParams {
    fn default() -> Self {
        Self {
            use_dhcp: true,
            ip: String::new(),
            mask: String::new(),
            gateway: String::new(),
            dns1: String::new(),
            dns2: String::new(),
        }
    }
}

/// 适配器编辑持久化：按网卡键存参数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AdapterEditPersist {
    /// 网卡稳定标识（GUID）→ 该网卡的编辑参数。
    pub adapters: BTreeMap<String, AdapterEditParams>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrips() {
        let s = SessionState::default();
        let json = serde_json::to_string(&s).unwrap();
        let back: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn empty_object_falls_back_to_defaults() {
        // 旧配置无 session 段 → 反序列化为全默认值（容器级 #[serde(default)] 生效）。
        let s: SessionState = serde_json::from_str("{}").unwrap();
        assert_eq!(s, SessionState::default());
        assert_eq!(s.ping.target, "8.8.8.8");
        assert_eq!(s.port_scan.end_port, "1024");
        assert_eq!(s.lan_speed.mode, "server");
    }

    #[test]
    fn partial_fields_keep_other_defaults() {
        // 只给 ping.target，其余字段（含同结构体其他字段）应逐一回退默认值。
        let json = r#"{"ping":{"target":"1.1.1.1"}}"#;
        let s: SessionState = serde_json::from_str(json).unwrap();
        assert_eq!(s.ping.target, "1.1.1.1");
        assert_eq!(s.ping.interval_ms, 1000);
        assert_eq!(s.ping.timeout_ms, 2000);
        assert_eq!(s.ping.packet_size, 32);
        assert_eq!(s.lan_speed.port, "50505");
    }

    #[test]
    fn history_defaults_and_partial() {
        // 旧配置无 history 段 → 两池空。
        let s: SessionState = serde_json::from_str("{}").unwrap();
        assert!(s.history.targets.is_empty());
        assert!(s.history.cidrs.is_empty());

        // 只给 targets，cidrs 回退默认空。
        let json = r#"{"history":{"targets":["8.8.8.8","1.1.1.1"]}}"#;
        let s: SessionState = serde_json::from_str(json).unwrap();
        assert_eq!(s.history.targets, vec!["8.8.8.8", "1.1.1.1"]);
        assert!(s.history.cidrs.is_empty());
    }

    #[test]
    fn lan_speed_persist_backcompat_defaults() {
        // 旧配置只有 mode/peer/port，缺新字段应回退默认
        let json = r#"{"mode":"client","peer":"10.0.0.2","port":"5000"}"#;
        let p: LanSpeedPersist = serde_json::from_str(json).unwrap();
        assert_eq!(p.mode, "client");
        assert_eq!(p.proto, "tcp");
        assert_eq!(p.direction, "up");
        assert_eq!(p.duration, "10");
    }

    #[test]
    fn link_quality_per_adapter_roundtrip() {
        // 一无线一有线，各记不同目标 IP；序列化往返后两者都保留。
        let mut s = SessionState::default();
        s.link_quality.adapters.insert(
            "{GUID-WIFI}".to_string(),
            LinkParams {
                target: "192.168.1.1".to_string(),
                ..LinkParams::default()
            },
        );
        s.link_quality.adapters.insert(
            "{GUID-ETH}".to_string(),
            LinkParams {
                target: "10.0.0.1".to_string(),
                ..LinkParams::default()
            },
        );
        s.link_quality.selected = Some("{GUID-WIFI}".to_string());

        let json = serde_json::to_string(&s).unwrap();
        let back: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
        assert_eq!(back.link_quality.adapters["{GUID-ETH}"].target, "10.0.0.1");
        assert_eq!(
            back.link_quality.adapters["{GUID-WIFI}"].target,
            "192.168.1.1"
        );
        assert_eq!(back.link_quality.selected.as_deref(), Some("{GUID-WIFI}"));
    }
}
