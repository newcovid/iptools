//! 网卡 IP 配置写入（静态 IP / DHCP）。
//!
//! Windows 实现通过 `wmi` crate 调用 `Win32_NetworkAdapterConfiguration` 的
//! EnableStatic / SetGateways / SetDNSServerSearchOrder / EnableDHCP 方法。
//! 用 `wmi` 封装 COM/WMI（VARIANT/SAFEARRAY 由其安全处理），避免手写易错的 FFI。
//!
//! **会真实改写系统网络栈，需管理员权限。** 调用方负责校验与二次确认。
//! 函数为阻塞式，应在 `spawn_blocking` 中调用。`guid` 为网卡 GUID
//! （等于 WMI `SettingID`，见 `InterfaceInfo::guid`）。
//!
//! ⚠️ 首次使用请在非关键网卡上验证；任何失败都返回 `Err`，不会 panic。

#![allow(dead_code)]

/// 设为静态：`gateway` 可空；`dns` 按优先顺序，可空。
pub fn apply_static(
    guid: &str,
    ip: &str,
    mask: &str,
    gateway: Option<&str>,
    dns: &[String],
) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        win::apply_static(guid, ip, mask, gateway, dns)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (guid, ip, mask, gateway, dns);
        Err("not supported on this platform".to_string())
    }
}

/// 切换为 DHCP 自动获取（地址 + DNS）。
pub fn apply_dhcp(guid: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        win::apply_dhcp(guid)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = guid;
        Err("not supported on this platform".to_string())
    }
}

#[cfg(target_os = "windows")]
mod win {
    use std::collections::HashMap;
    use wmi::{Variant, WMIConnection};

    const CLASS: &str = "Win32_NetworkAdapterConfiguration";

    fn connect() -> Result<WMIConnection, String> {
        WMIConnection::new().map_err(|e| format!("WMI 连接失败: {e}"))
    }

    /// 按 SettingID(=GUID) 找到网卡的 Index，组成实例路径。
    fn instance_path(con: &WMIConnection, guid: &str) -> Result<String, String> {
        // GUID 来自系统枚举，仅含十六进制与 {}-，无需担心 WQL 注入；仍去掉引号以防万一
        let safe_guid = guid.replace('\'', "");
        let q = format!(
            "SELECT Index FROM {CLASS} WHERE SettingID = '{}'",
            safe_guid
        );
        let rows: Vec<HashMap<String, Variant>> =
            con.raw_query(q).map_err(|e| format!("查询网卡失败: {e}"))?;
        let row = rows.first().ok_or_else(|| "未找到对应网卡".to_string())?;
        let index = match row.get("Index") {
            Some(Variant::I4(n)) => *n as i64,
            Some(Variant::UI4(n)) => *n as i64,
            _ => return Err("网卡 Index 缺失".to_string()),
        };
        Ok(format!("{CLASS}.Index={index}"))
    }

    /// 把 Win32_NetworkAdapterConfiguration 方法的返回码翻成可读原因。
    /// 见 <https://learn.microsoft.com/windows/win32/cimwin32prov/enablestatic-method-in-class-win32-networkadapterconfiguration>。
    fn wmi_return_desc(method: &str, rv: i64) -> String {
        let reason = match rv {
            64 => "该平台不支持此方法",
            65 => "未知失败",
            66 => "子网掩码无效",
            67 => "处理实例时发生错误",
            68 => "输入参数无效",
            69 => "指定的网关超过 5 个",
            70 => "IP 地址无效",
            71 => "网关地址无效",
            72 => "访问注册表时出错",
            84 => "该网卡未启用 IP（IPEnabled=false）",
            _ => "操作失败",
        };
        format!("{method} 失败（{reason}，错误码 {rv}）")
    }

    /// 在实例上执行一个 WMI 方法；`params` 为 (参数名, 值) 列表。
    ///
    /// 注意：`get_method` 对**无入参**的方法（如 `EnableDHCP`）返回 `None`，
    /// 这是正常的——此时应直接以 `None` 入参执行，而非当作“方法不存在”报错。
    fn invoke(
        con: &WMIConnection,
        path: &str,
        method: &str,
        params: &[(&str, Variant)],
    ) -> Result<(), String> {
        let method_sig = con
            .get_object(CLASS)
            .map_err(|e| format!("get_object: {e}"))?
            .get_method(method)
            .map_err(|e| format!("get_method {method}: {e}"))?;

        let in_params = match method_sig {
            Some(sig) => {
                let inst = sig
                    .spawn_instance()
                    .map_err(|e| format!("spawn_instance: {e}"))?;
                for (name, val) in params {
                    inst.put_property(name, val.clone())
                        .map_err(|e| format!("设置参数 {name} 失败: {e}"))?;
                }
                Some(inst)
            }
            // 无入参方法：忽略传入的 params（本就为空），以 None 执行。
            None => None,
        };

        let out = con
            .exec_method(path, method, in_params.as_ref())
            .map_err(|e| format!("执行 {method} 失败: {e}"))?;

        if let Some(out) = out {
            let rv = match out.get_property("ReturnValue") {
                Ok(Variant::I4(n)) => n as i64,
                Ok(Variant::UI4(n)) => n as i64,
                _ => 0,
            };
            // 0 成功；1 成功但需重启；其余为错误码
            if rv != 0 && rv != 1 {
                return Err(wmi_return_desc(method, rv));
            }
        }
        Ok(())
    }

    fn str_array(items: &[String]) -> Variant {
        Variant::Array(items.iter().cloned().map(Variant::String).collect())
    }

    pub fn apply_static(
        guid: &str,
        ip: &str,
        mask: &str,
        gateway: Option<&str>,
        dns: &[String],
    ) -> Result<(), String> {
        let con = connect()?;
        let path = instance_path(&con, guid)?;

        // 设置静态 IP 和子网掩码
        invoke(
            &con,
            &path,
            "EnableStatic",
            &[
                ("IPAddress", str_array(&[ip.to_string()])),
                ("SubnetMask", str_array(&[mask.to_string()])),
            ],
        )?;

        // 显式释放 DHCP 租约，确保 DHCPEnabled=false
        // EnableStatic 只设置 IP，不会自动禁用 DHCP 标志
        // 这样 Windows 设置界面才能正确显示"手动"状态
        let _ = invoke(&con, &path, "ReleaseDHCPLease", &[]);

        if let Some(gw) = gateway {
            invoke(
                &con,
                &path,
                "SetGateways",
                &[("DefaultIPGateway", str_array(&[gw.to_string()]))],
            )?;
        }

        if !dns.is_empty() {
            invoke(
                &con,
                &path,
                "SetDNSServerSearchOrder",
                &[("DNSServerSearchOrder", str_array(dns))],
            )?;
        }

        Ok(())
    }

    pub fn apply_dhcp(guid: &str) -> Result<(), String> {
        let con = connect()?;
        let path = instance_path(&con, guid)?;
        // EnableDHCP 让地址回到自动获取。该方法无入参，invoke 会以 None 执行。
        invoke(&con, &path, "EnableDHCP", &[])?;
        // 让 DNS 也回到自动获取：SetDNSServerSearchOrder 传空数组（在 WMI 中表达为
        // VT_NULL）即恢复为 DHCP 下发的 DNS。尽力而为——失败不影响 IP 已切回 DHCP 的主结果。
        let _ = invoke(
            &con,
            &path,
            "SetDNSServerSearchOrder",
            &[("DNSServerSearchOrder", str_array(&[]))],
        );
        Ok(())
    }
}

/// Linux IP 写入后端：分层探测 nmcli → netplan → ip。平台无关编译（仅用 Command/fs）。
pub(crate) mod linux {
    #![allow(dead_code)]
    use std::process::Command;

    /// 点分掩码 → 前缀长度（连续 1 才合法）。
    pub fn mask_to_prefix(mask: &str) -> Option<u8> {
        let ip: std::net::Ipv4Addr = mask.parse().ok()?;
        let bits = u32::from(ip);
        let ones = bits.leading_ones();
        let expected = if ones == 0 { 0 } else { u32::MAX << (32 - ones) };
        if bits == expected { Some(ones as u8) } else { None }
    }

    /// 生成受管 netplan YAML。`static_cfg = Some((ip, prefix, gw, dns))` 为静态；None 为 DHCP。
    pub fn netplan_yaml(
        iface: &str,
        static_cfg: Option<(&str, u8, Option<&str>, Vec<String>)>,
    ) -> String {
        let mut s = String::from("# Managed by iptools — do not edit by hand\nnetwork:\n  version: 2\n  ethernets:\n");
        s.push_str(&format!("    {iface}:\n"));
        match static_cfg {
            None => s.push_str("      dhcp4: true\n"),
            Some((ip, prefix, gw, dns)) => {
                s.push_str("      dhcp4: false\n");
                s.push_str(&format!("      addresses:\n        - {ip}/{prefix}\n"));
                if let Some(g) = gw {
                    s.push_str("      routes:\n");
                    s.push_str(&format!("        - {g}\n"));
                }
                if !dns.is_empty() {
                    s.push_str("      nameservers:\n        addresses:\n");
                    for d in &dns {
                        s.push_str(&format!("          - {d}\n"));
                    }
                }
            }
        }
        s
    }

    /// 后端种类。
    pub enum Backend {
        NetworkManager,
        Netplan,
        IpFallback,
    }

    /// 探测可用后端：NM 活动优先，其次 netplan，最后 ip 兜底。
    pub fn detect_backend() -> Backend {
        if let Ok(out) = Command::new("systemctl")
            .args(["is-active", "NetworkManager"])
            .output()
        {
            if out.status.success()
                && String::from_utf8_lossy(&out.stdout).trim() == "active"
            {
                return Backend::NetworkManager;
            }
        }
        if std::path::Path::new("/etc/netplan").exists() {
            if let Ok(rd) = std::fs::read_dir("/etc/netplan") {
                if rd.flatten().any(|e| {
                    e.path().extension().map_or(false, |x| x == "yaml")
                }) {
                    return Backend::Netplan;
                }
            }
        }
        Backend::IpFallback
    }

    /// 运行命令，失败时返回 stderr 文本。成功返回 Ok(())。
    pub fn run(cmd: &str, args: &[&str]) -> Result<(), String> {
        let out = Command::new(cmd)
            .args(args)
            .output()
            .map_err(|e| format!("无法执行 {cmd}: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "{cmd} 失败: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }

    /// 找接口对应的 NM 连接名（active 优先）。
    pub fn nm_connection_for(iface: &str) -> Option<String> {
        let out = Command::new("nmcli")
            .args(["-t", "-f", "NAME,DEVICE", "connection", "show", "--active"])
            .output()
            .ok()?;
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Some((name, dev)) = line.rsplit_once(':') {
                if dev == iface {
                    return Some(name.to_string());
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::linux::*;

    #[test]
    fn mask_to_prefix_works() {
        assert_eq!(mask_to_prefix("255.255.255.0"), Some(24));
        assert_eq!(mask_to_prefix("255.255.0.0"), Some(16));
        assert_eq!(mask_to_prefix("255.0.255.0"), None);
        assert_eq!(mask_to_prefix("oops"), None);
    }

    #[test]
    fn netplan_static_yaml_shape() {
        let y = netplan_yaml(
            "eth0",
            Some(("192.168.1.50", 24, Some("192.168.1.1"), vec!["1.1.1.1".into()])),
        );
        assert!(y.contains("eth0:"));
        assert!(y.contains("dhcp4: false"));
        assert!(y.contains("192.168.1.50/24"));
        assert!(y.contains("- 192.168.1.1"));
        assert!(y.contains("1.1.1.1"));
    }

    #[test]
    fn netplan_dhcp_yaml_shape() {
        let y = netplan_yaml("eth0", None);
        assert!(y.contains("eth0:"));
        assert!(y.contains("dhcp4: true"));
    }
}
