//! 共享的单次 ICMP Echo 原语，供 traceroute 与链路质量复用，
//! 把 Windows 的 `unsafe` FFI 收敛到一处。
//!
//! 注意：`ping.rs` 维持自己的连续发包循环（载荷大小可调、复用 handle），
//! 与此处一次性 echo 的用途不同，未合并。

use std::net::Ipv4Addr;

/// ICMP 状态码（IP_STATUS）。
pub const IP_SUCCESS: u32 = 0;
pub const IP_TTL_EXPIRED_TRANSIT: u32 = 11013;
pub const IP_REQ_TIMED_OUT: u32 = 11010;

/// 单次 echo 的结果。
#[derive(Debug, Clone, Copy)]
pub struct EchoResult {
    /// IP_STATUS 状态码；`u32::MAX` 表示本地调用失败（如句柄创建失败/平台不支持）。
    pub status: u32,
    /// 响应方地址（中间路由或目标）；超时为 `None`。
    pub addr: Option<Ipv4Addr>,
    /// 往返时延（毫秒）；仅在有响应时有效。
    pub rtt_ms: Option<u64>,
}

impl EchoResult {
    /// 是否成功抵达目标（区别于 TTL 过期的中间跳）。
    pub fn reached(&self) -> bool {
        self.status == IP_SUCCESS
    }
}

/// 向 `dest` 发送一个 TTL=`ttl` 的 ICMP Echo（32 字节载荷），等待至多 `timeout_ms`。
#[cfg(target_os = "windows")]
pub fn echo_once(dest: Ipv4Addr, ttl: u8, timeout_ms: u32) -> EchoResult {
    use std::ffi::c_void;
    use windows::Win32::NetworkManagement::IpHelper::{
        ICMP_ECHO_REPLY, IP_OPTION_INFORMATION, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho,
    };

    let dest_u32 = u32::from_le_bytes(dest.octets());
    let payload = [0u8; 32];
    const REPLY_SIZE: usize = 2048 + 65535;

    let handle = match unsafe { IcmpCreateFile() } {
        Ok(h) => h,
        Err(_) => {
            return EchoResult {
                status: u32::MAX,
                addr: None,
                rtt_ms: None,
            };
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
        IcmpSendEcho(
            handle,
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

/// 通用 unix 的 ICMP 报文构造与解析（纯函数，便于单测）。平台无关，始终编译。
pub(crate) mod unix_icmp {
    #![allow(dead_code)]

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum ReplyKind {
        EchoReply,
        TimeExceeded,
    }

    /// 标准 RFC1071 ones-complement 校验和。
    pub fn checksum(data: &[u8]) -> u16 {
        let mut sum = 0u32;
        let mut i = 0;
        while i + 1 < data.len() {
            sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
            i += 2;
        }
        if i < data.len() {
            sum += (data[i] as u32) << 8;
        }
        while (sum >> 16) != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        !(sum as u16)
    }

    /// 构造 ICMP Echo Request：type=8,code=0,checksum,id,seq + payload_len 字节 0 载荷。
    pub fn build_echo_request(id: u16, seq: u16, payload_len: usize) -> Vec<u8> {
        let mut pkt = vec![0u8; 8 + payload_len];
        pkt[0] = 8;
        pkt[1] = 0;
        pkt[4..6].copy_from_slice(&id.to_be_bytes());
        pkt[6..8].copy_from_slice(&seq.to_be_bytes());
        let c = checksum(&pkt);
        pkt[2..4].copy_from_slice(&c.to_be_bytes());
        pkt
    }

    /// 取 IPv4 头长度（IHL*4）。buf 须以 IP 头开头（IPv4 raw 套接字交付如此）。
    fn ip_header_len(buf: &[u8]) -> Option<usize> {
        let ihl = (buf.first()? & 0x0F) as usize * 4;
        if ihl >= 20 && ihl <= buf.len() {
            Some(ihl)
        } else {
            None
        }
    }

    /// 解析内核交付的 raw 套接字缓冲，匹配我们发出的 (id, seq)。不匹配/非关心类型 → None。
    pub fn parse_reply(buf: &[u8], want_id: u16, want_seq: u16) -> Option<ReplyKind> {
        let ihl = ip_header_len(buf)?;
        let icmp = buf.get(ihl..)?;
        let icmp_type = *icmp.first()?;
        match icmp_type {
            0 => {
                let id = u16::from_be_bytes([*icmp.get(4)?, *icmp.get(5)?]);
                let seq = u16::from_be_bytes([*icmp.get(6)?, *icmp.get(7)?]);
                if id == want_id && seq == want_seq {
                    Some(ReplyKind::EchoReply)
                } else {
                    None
                }
            }
            11 => {
                let inner = icmp.get(8..)?;
                let inner_ihl = ip_header_len(inner)?;
                let orig_icmp = inner.get(inner_ihl..)?;
                let id = u16::from_be_bytes([*orig_icmp.get(4)?, *orig_icmp.get(5)?]);
                let seq = u16::from_be_bytes([*orig_icmp.get(6)?, *orig_icmp.get(7)?]);
                if id == want_id && seq == want_seq {
                    Some(ReplyKind::TimeExceeded)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

#[cfg(unix)]
pub fn echo_once(dest: Ipv4Addr, ttl: u8, timeout_ms: u32) -> EchoResult {
    unix_send(None, dest, ttl, timeout_ms, 32)
}

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
        ICMP_ECHO_REPLY, IP_OPTION_INFORMATION, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho2Ex,
    };

    let src_u32 = u32::from_le_bytes(src.octets());
    let dest_u32 = u32::from_le_bytes(dest.octets());
    let payload = vec![0u8; payload_len.clamp(1, 1472)];
    const REPLY_SIZE: usize = 2048 + 65535;

    let handle = match unsafe { IcmpCreateFile() } {
        Ok(h) => h,
        Err(_) => {
            return EchoResult {
                status: u32::MAX,
                addr: None,
                rtt_ms: None,
            };
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

#[cfg(unix)]
pub fn echo_once_from(
    src: Ipv4Addr,
    dest: Ipv4Addr,
    ttl: u8,
    timeout_ms: u32,
    payload_len: usize,
) -> EchoResult {
    unix_send(Some(src), dest, ttl, timeout_ms, payload_len)
}

/// unix 下用 raw ICMP 套接字发一个 echo 并等回包。`src` 非空时绑定出口源 IP。
/// 收 Echo Reply → reached；收 Time Exceeded → 中间跳（addr 来自回包源地址）。
#[cfg(unix)]
fn unix_send(
    src: Option<Ipv4Addr>,
    dest: Ipv4Addr,
    ttl: u8,
    timeout_ms: u32,
    payload_len: usize,
) -> EchoResult {
    use socket2::{Domain, Protocol, SockAddr, Socket, Type};
    use std::mem::MaybeUninit;
    use std::net::SocketAddr;
    use std::time::{Duration, Instant};

    let sock = match Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::ICMPV4)) {
        Ok(s) => s,
        Err(_) => {
            return EchoResult {
                status: u32::MAX,
                addr: None,
                rtt_ms: None,
            };
        }
    };
    if sock.set_ttl(ttl as u32).is_err()
        || sock
            .set_read_timeout(Some(Duration::from_millis(timeout_ms.max(1) as u64)))
            .is_err()
    {
        return EchoResult {
            status: u32::MAX,
            addr: None,
            rtt_ms: None,
        };
    }
    if let Some(s) = src {
        if sock
            .bind(&SockAddr::from(SocketAddr::new(s.into(), 0)))
            .is_err()
        {
            return EchoResult {
                status: u32::MAX,
                addr: None,
                rtt_ms: None,
            };
        }
    }

    let id = (std::process::id() & 0xFFFF) as u16;
    // seq 取 ttl：raw ICMP 套接字会收到所有 ICMP 流量，trace 逐跳用不同 TTL，
    // 以 seq=ttl 区分各跳，避免上一跳迟到的 Time-Exceeded 被下一跳的套接字误配。
    let seq = ttl as u16;
    let pkt = unix_icmp::build_echo_request(id, seq, payload_len.clamp(0, 1472));

    let to = SockAddr::from(SocketAddr::new(dest.into(), 0));
    let start = Instant::now();
    if sock.send_to(&pkt, &to).is_err() {
        return EchoResult {
            status: u32::MAX,
            addr: None,
            rtt_ms: None,
        };
    }

    let deadline = start + Duration::from_millis(timeout_ms.max(1) as u64);
    let mut buf = [MaybeUninit::<u8>::uninit(); 1500];
    loop {
        if Instant::now() >= deadline {
            return EchoResult {
                status: IP_REQ_TIMED_OUT,
                addr: None,
                rtt_ms: None,
            };
        }
        match sock.recv_from(&mut buf) {
            Ok((n, from)) => {
                let data = unsafe { std::slice::from_raw_parts(buf.as_ptr() as *const u8, n) };
                match unix_icmp::parse_reply(data, id, seq) {
                    Some(unix_icmp::ReplyKind::EchoReply) => {
                        let rtt = start.elapsed().as_millis() as u64;
                        return EchoResult {
                            status: IP_SUCCESS,
                            addr: Some(dest),
                            rtt_ms: Some(rtt),
                        };
                    }
                    Some(unix_icmp::ReplyKind::TimeExceeded) => {
                        let rtt = start.elapsed().as_millis() as u64;
                        let router = from.as_socket_ipv4().map(|s| *s.ip()).unwrap_or(dest);
                        return EchoResult {
                            status: IP_TTL_EXPIRED_TRANSIT,
                            addr: Some(router),
                            rtt_ms: Some(rtt),
                        };
                    }
                    None => continue,
                }
            }
            Err(_) => {
                return EchoResult {
                    status: IP_REQ_TIMED_OUT,
                    addr: None,
                    rtt_ms: None,
                };
            }
        }
    }
}

/// 其它平台暂不提供 ICMP 后端。
#[cfg(all(not(unix), not(target_os = "windows")))]
pub fn echo_once(_dest: Ipv4Addr, _ttl: u8, _timeout_ms: u32) -> EchoResult {
    EchoResult {
        status: u32::MAX,
        addr: None,
        rtt_ms: None,
    }
}
#[cfg(all(not(unix), not(target_os = "windows")))]
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

#[cfg(test)]
mod tests {
    use super::unix_icmp::*;

    #[test]
    fn checksum_known_vector() {
        let pkt = build_echo_request(0x1234, 1, 0);
        assert_eq!(checksum(&pkt), 0);
        assert_eq!(pkt[0], 8);
        assert_eq!(pkt[1], 0);
    }

    #[test]
    fn echo_request_has_id_seq_and_payload() {
        let pkt = build_echo_request(0xBEEF, 7, 16);
        assert_eq!(pkt.len(), 8 + 16);
        assert_eq!(u16::from_be_bytes([pkt[4], pkt[5]]), 0xBEEF);
        assert_eq!(u16::from_be_bytes([pkt[6], pkt[7]]), 7);
    }

    #[test]
    fn parse_echo_reply_matches_id_seq() {
        let mut buf = vec![0u8; 20];
        buf[0] = 0x45;
        buf.extend_from_slice(&[0u8, 0, 0, 0, 0, 0, 0, 0]);
        buf[24..26].copy_from_slice(&0xABCDu16.to_be_bytes());
        buf[26..28].copy_from_slice(&5u16.to_be_bytes());
        assert!(matches!(
            parse_reply(&buf, 0xABCD, 5),
            Some(ReplyKind::EchoReply)
        ));
        assert!(parse_reply(&buf, 0x1111, 5).is_none());
    }

    #[test]
    fn parse_time_exceeded_matches_embedded_id_seq() {
        let mut buf = vec![0u8; 20];
        buf[0] = 0x45;
        buf.extend_from_slice(&[11u8, 0, 0, 0, 0, 0, 0, 0]);
        let mut inner_ip = vec![0u8; 20];
        inner_ip[0] = 0x45;
        buf.extend_from_slice(&inner_ip);
        buf.extend_from_slice(&[8u8, 0, 0, 0]);
        buf.extend_from_slice(&0x77AAu16.to_be_bytes());
        buf.extend_from_slice(&9u16.to_be_bytes());
        assert!(matches!(
            parse_reply(&buf, 0x77AA, 9),
            Some(ReplyKind::TimeExceeded)
        ));
    }
}
