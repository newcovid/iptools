//! 公网信息归一化与多家端点解析。
//!
//! 各家返回 JSON 形状不同，统一解析成 `PublicInfo`。端点在 config 里声明 `kind`：
//! - `ipsb`     : https://api.ip.sb/geoip
//! - `ipinfo`   : https://ipinfo.io/json
//! - `plaintext`: 仅返回纯文本公网 IP（地理/ISP 留空）

use serde::Deserialize;

/// 归一化后的公网信息（地理/ISP 可能为空字符串）。
#[derive(Debug, Clone, PartialEq)]
pub struct PublicInfo {
    pub ip: String,
    pub city: String,
    pub region: String,
    pub country: String,
    pub isp: String,
}

#[derive(Debug, Deserialize)]
struct IpSb {
    ip: String,
    #[serde(default)]
    city: String,
    #[serde(default)]
    region: String,
    #[serde(default)]
    country: String,
    #[serde(default)]
    isp: String,
    #[serde(default)]
    asn_organization: String,
}

#[derive(Debug, Deserialize)]
struct IpInfo {
    ip: String,
    #[serde(default)]
    city: String,
    #[serde(default)]
    region: String,
    #[serde(default)]
    country: String,
    #[serde(default)]
    org: String,
}

/// 按端点 `kind` 解析响应体为 `PublicInfo`；无法解析或缺 IP 返回 None。
pub fn parse(kind: &str, body: &str) -> Option<PublicInfo> {
    match kind {
        "ipsb" => {
            let v: IpSb = serde_json::from_str(body).ok()?;
            if v.ip.trim().is_empty() {
                return None;
            }
            let isp = if !v.isp.trim().is_empty() {
                v.isp
            } else {
                v.asn_organization
            };
            Some(PublicInfo {
                ip: v.ip,
                city: v.city,
                region: v.region,
                country: v.country,
                isp,
            })
        }
        "ipinfo" => {
            let v: IpInfo = serde_json::from_str(body).ok()?;
            if v.ip.trim().is_empty() {
                return None;
            }
            Some(PublicInfo {
                ip: v.ip,
                city: v.city,
                region: v.region,
                country: v.country,
                isp: v.org,
            })
        }
        "plaintext" => {
            let ip = body.trim();
            // 粗校验：必须像个 IP（含 '.' 或 ':'，无空白/HTML）。
            if ip.is_empty() || ip.len() > 64 || ip.contains(char::is_whitespace) || ip.contains('<')
            {
                return None;
            }
            Some(PublicInfo {
                ip: ip.to_string(),
                city: String::new(),
                region: String::new(),
                country: String::new(),
                isp: String::new(),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ipsb() {
        let body = r#"{"ip":"1.2.3.4","city":"Tokyo","region":"Tokyo","country":"Japan","isp":"Acme","asn_organization":"AcmeAS"}"#;
        let info = parse("ipsb", body).unwrap();
        assert_eq!(info.ip, "1.2.3.4");
        assert_eq!(info.city, "Tokyo");
        assert_eq!(info.isp, "Acme");
    }

    #[test]
    fn parse_ipsb_isp_falls_back_to_asn_org() {
        let body = r#"{"ip":"1.2.3.4","isp":"","asn_organization":"Cloudflare"}"#;
        assert_eq!(parse("ipsb", body).unwrap().isp, "Cloudflare");
    }

    #[test]
    fn parse_ipinfo_uses_org_as_isp() {
        let body = r#"{"ip":"5.6.7.8","city":"Berlin","region":"Berlin","country":"DE","org":"AS3320 DT"}"#;
        let info = parse("ipinfo", body).unwrap();
        assert_eq!(info.ip, "5.6.7.8");
        assert_eq!(info.country, "DE");
        assert_eq!(info.isp, "AS3320 DT");
    }

    #[test]
    fn parse_plaintext_ok_and_reject_html() {
        assert_eq!(parse("plaintext", "  9.9.9.9\n").unwrap().ip, "9.9.9.9");
        assert!(parse("plaintext", "<html>err</html>").is_none());
        assert!(parse("plaintext", "").is_none());
    }

    #[test]
    fn parse_unknown_kind_is_none() {
        assert!(parse("whatever", "{}").is_none());
    }

    #[test]
    fn parse_missing_ip_is_none() {
        assert!(parse("ipsb", r#"{"city":"X"}"#).is_none());
    }
}
