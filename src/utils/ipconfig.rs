//! 网卡 IP 配置写入（静态 IP / DHCP）。
//!
//! Windows 实现使用 WMI `Win32_NetworkAdapterConfiguration` 的
//! EnableStatic / SetGateways / SetDNSServerSearchOrder / EnableDHCP 方法。
//! **会真实改写系统网络栈，需管理员权限。** 调用方负责校验与二次确认。
//!
//! 函数为阻塞式，应在 `spawn_blocking` 中调用。`guid` 为网卡 GUID
//! （等于 WMI `SettingID`，见 `InterfaceInfo::guid`）。
//!
//! 当前为占位实现（Phase B）：仅做 UI/校验/确认流程联调，尚未接入真实写入。

#![allow(dead_code)]

/// 设为静态：`gateway` 可空；`dns` 按优先顺序，可空。
pub fn apply_static(
    _guid: &str,
    _ip: &str,
    _mask: &str,
    _gateway: Option<&str>,
    _dns: &[String],
) -> Result<(), String> {
    Err("apply not wired yet (Phase B stub)".to_string())
}

/// 切换为 DHCP 自动获取（地址 + DNS）。
pub fn apply_dhcp(_guid: &str) -> Result<(), String> {
    Err("apply not wired yet (Phase B stub)".to_string())
}
