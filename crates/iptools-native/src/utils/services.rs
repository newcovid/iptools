//! Offline TCP service-name lookup.
//!
//! Friendly names preserve the established display for common services. All other
//! assigned ports fall back to the bundled IANA Service Name registry from
//! `port-desc`; no network request or banner probe is performed during scans.

use std::sync::LazyLock;

use port_desc::{PortDescription, TransportProtocol};

static IANA_SERVICES: LazyLock<Option<PortDescription>> =
    LazyLock::new(|| PortDescription::default().ok());

pub fn tcp_service(port: u16) -> String {
    if let Some(name) = friendly_service(port) {
        return name.into();
    }
    IANA_SERVICES
        .as_ref()
        .map(|services| services.get_port_service_name(port, TransportProtocol::Tcp))
        .filter(|name| !name.is_empty())
        .unwrap_or("-")
        .to_string()
}

fn friendly_service(port: u16) -> Option<&'static str> {
    Some(match port {
        20 | 21 => "FTP",
        22 => "SSH",
        23 => "Telnet",
        25 => "SMTP",
        53 => "DNS",
        67 | 68 => "DHCP",
        80 => "HTTP",
        110 => "POP3",
        123 => "NTP",
        135 => "MSRPC",
        139 => "NetBIOS",
        143 => "IMAP",
        389 => "LDAP",
        443 => "HTTPS",
        445 => "SMB",
        587 => "SMTP/TLS",
        993 => "IMAPS",
        995 => "POP3S",
        1433 => "MSSQL",
        1521 => "Oracle",
        3306 => "MySQL",
        3389 => "RDP",
        5432 => "PostgreSQL",
        5900 => "VNC",
        6379 => "Redis",
        8080 => "HTTP-Alt",
        8443 => "HTTPS-Alt",
        9200 => "Elasticsearch",
        27017 => "MongoDB",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_friendly_names_and_covers_the_bundled_iana_registry() {
        assert_eq!(tcp_service(443), "HTTPS");
        assert_ne!(tcp_service(631), "-");
        assert_eq!(tcp_service(65_000), "-");
    }
}
