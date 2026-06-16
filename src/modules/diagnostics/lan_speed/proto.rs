//! 内网测速线路协议与测量逻辑。
//!
//! 控制头用 JSON（长度前缀），UDP 数据报头用紧凑二进制。收发 worker 用纯
//! tokio TCP/UDP，跨平台。纯逻辑（编解码 / 丢包统计 / 速率换算）带单元测试。

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const DEFAULT_PORT: u16 = 50505;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Proto {
    Tcp,
    Udp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Up,
    Down,
    Bidir,
}

/// 客户端发给服务端的测试参数。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestSpec {
    pub proto: Proto,
    pub direction: Direction,
    pub duration_ms: u64,
    pub streams: u16,
    pub rate_mbps: u32,
    pub payload_size: u32,
}

/// 服务端对控制头的应答（就绪信号）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ack {
    pub ok: bool,
}

/// 编码为 `[u32 LE 长度][JSON body]`。
pub fn encode_frame<T: Serialize>(v: &T) -> Vec<u8> {
    let body = serde_json::to_vec(v).unwrap_or_default();
    let mut out = (body.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(&body);
    out
}

/// 从完整帧（含长度前缀）解析。长度不符返回 None。
pub fn decode_frame<T: DeserializeOwned>(frame: &[u8]) -> Option<T> {
    if frame.len() < 4 {
        return None;
    }
    let len = u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
    let body = frame.get(4..4 + len)?;
    serde_json::from_slice(body).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_roundtrip_spec() {
        let spec = TestSpec {
            proto: Proto::Tcp,
            direction: Direction::Bidir,
            duration_ms: 10_000,
            streams: 4,
            rate_mbps: 0,
            payload_size: 65536,
        };
        let frame = encode_frame(&spec);
        // 前 4 字节是 LE 长度，且总长 = 4 + 长度
        let len = u32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
        assert_eq!(frame.len(), 4 + len);
        let decoded: TestSpec = decode_frame(&frame).unwrap();
        assert_eq!(decoded, spec);
    }

    #[test]
    fn decode_rejects_short_frame() {
        let r: Option<TestSpec> = decode_frame(&[1, 2]);
        assert!(r.is_none());
    }
}
