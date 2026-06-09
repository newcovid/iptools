//! 共享的单次 ICMP Echo 原语，供 traceroute 与链路质量复用，
//! 把 Windows 的 `unsafe` FFI 收敛到一处。
//!
//! 注意：`ping.rs` 维持自己的连续发包循环（载荷大小可调、复用 handle），
//! 与此处一次性 echo 的用途不同，未合并。

use std::net::Ipv4Addr;

/// ICMP 状态码（IP_STATUS）。
pub const IP_SUCCESS: u32 = 0;
pub const IP_TTL_EXPIRED_TRANSIT: u32 = 11013;
#[allow(dead_code)]
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
        IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho, ICMP_ECHO_REPLY, IP_OPTION_INFORMATION,
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

#[cfg(not(target_os = "windows"))]
pub fn echo_once(_dest: Ipv4Addr, _ttl: u8, _timeout_ms: u32) -> EchoResult {
    EchoResult {
        status: u32::MAX,
        addr: None,
        rtt_ms: None,
    }
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
        IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho2Ex, ICMP_ECHO_REPLY, IP_OPTION_INFORMATION,
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
