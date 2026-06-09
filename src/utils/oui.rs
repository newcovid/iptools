//! MAC 厂商（OUI）查询。
//!
//! 内嵌一份小而高置信度的常见 OUI 前缀表（虚拟化平台 / 树莓派 / Apple /
//! Cisco / Google 等），覆盖开发局域网里最常见的设备。查不到返回 `None`，
//! 由调用方显示 "-"。宁缺勿错：只收录把握较大的前缀。

/// 按 MAC 的前 3 个字节（OUI）查厂商。
/// 接受任意分隔符（`:`、`-` 或无）与大小写。
pub fn lookup(mac: &str) -> Option<&'static str> {
    let hex: String = mac
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(6)
        .collect::<String>()
        .to_uppercase();
    if hex.len() < 6 {
        return None;
    }
    let v = match hex.as_str() {
        // 虚拟化 / 虚拟网卡
        "000C29" | "005056" | "000569" | "001C14" => "VMware",
        "080027" | "0A0027" => "VirtualBox",
        "525400" => "QEMU/KVM",
        "00155D" => "Microsoft Hyper-V",
        "001C42" => "Parallels",
        "0003FF" => "Microsoft",
        // 树莓派
        "B827EB" | "DCA632" | "E45F01" | "28CDC1" => "Raspberry Pi",
        // Apple
        "001451" | "0017F2" | "3C0754" | "F0DBF8" | "A4B197" | "DC2B2A" => "Apple",
        // 网络设备
        "00000C" | "001121" => "Cisco",
        // Google / Nest
        "F4F5E8" | "3C5AB4" | "A47733" => "Google",
        _ => return None,
    };
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_prefixes_resolve() {
        assert_eq!(lookup("08:00:27:ab:cd:ef"), Some("VirtualBox"));
        assert_eq!(lookup("00:0c:29:11:22:33"), Some("VMware"));
        assert_eq!(lookup("b8-27-eb-00-00-00"), Some("Raspberry Pi"));
    }

    #[test]
    fn accepts_various_separators_and_case() {
        assert_eq!(lookup("000C29112233"), Some("VMware"));
        assert_eq!(lookup("52:54:00:AA:BB:CC"), Some("QEMU/KVM"));
    }

    #[test]
    fn unknown_or_malformed_is_none() {
        assert_eq!(lookup("ff:ff:ff:00:00:00"), None);
        assert_eq!(lookup("xx"), None);
        assert_eq!(lookup(""), None);
    }
}
