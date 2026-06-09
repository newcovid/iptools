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

        invoke(
            &con,
            &path,
            "EnableStatic",
            &[
                ("IPAddress", str_array(&[ip.to_string()])),
                ("SubnetMask", str_array(&[mask.to_string()])),
            ],
        )?;

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
