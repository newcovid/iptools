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

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// 向异步写端发一帧。
pub async fn write_frame<W, T>(w: &mut W, v: &T) -> std::io::Result<()>
where
    W: AsyncWriteExt + Unpin,
    T: Serialize,
{
    let frame = encode_frame(v);
    w.write_all(&frame).await
}

/// 从异步读端读一帧（先读 4 字节长度，再读 body）。失败/超长返回 None。
pub async fn read_frame<R, T>(r: &mut R) -> Option<T>
where
    R: AsyncReadExt + Unpin,
    T: DeserializeOwned,
{
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await.ok()?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > 64 * 1024 {
        return None;
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await.ok()?;
    serde_json::from_slice(&body).ok()
}

/// 一条数据连接/socket 在本端扮演的角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Role {
    Send,
    Recv,
    Both,
}

/// 由「本端是否服务端 + 方向」推出角色。
/// Up=客户端→服务端；Down=服务端→客户端；Bidir=两端同时收发。
pub(crate) fn role_for(is_server: bool, dir: Direction) -> Role {
    match (is_server, dir) {
        (false, Direction::Up) | (true, Direction::Down) => Role::Send,
        (true, Direction::Up) | (false, Direction::Down) => Role::Recv,
        (_, Direction::Bidir) => Role::Both,
    }
}

/// 速率方向（双向时区分两路曲线）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Tx,
    Rx,
}

/// UDP 接收汇总（接收侧统计丢包）。
#[derive(Debug, Clone, Default)]
pub struct UdpSummary {
    pub received: u64,
    pub lost: u64,
    pub out_of_order: u64,
    pub jitter_ms: f64,
}

impl UdpSummary {
    pub fn loss_pct(&self) -> f64 {
        let total = self.received + self.lost;
        if total == 0 {
            0.0
        } else {
            self.lost as f64 / total as f64 * 100.0
        }
    }
}

/// 一次测试结束的汇总。
#[derive(Debug, Clone, Default)]
pub struct TestSummary {
    pub tx_bytes: u64,
    pub rx_bytes: u64,
    pub elapsed_ms: u64,
    pub udp: Option<UdpSummary>,
}

/// worker → UI 的事件。
#[derive(Debug)]
pub enum LanEvent {
    /// i18n 键
    Status(String),
    Progress {
        flow: Flow,
        total_bytes: u64,
        elapsed_ms: u64,
        inst_bps: u64,
    },
    Summary(TestSummary),
    /// i18n 键
    Error(String),
}

/// 平均吞吐（字节/秒）；elapsed_ms=0 时返回 0。
pub fn avg_bytes_per_sec(total_bytes: u64, elapsed_ms: u64) -> u64 {
    if elapsed_ms == 0 {
        return 0;
    }
    total_bytes.saturating_mul(1000) / elapsed_ms
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

    #[test]
    fn roles_follow_direction() {
        assert_eq!(role_for(false, Direction::Up), Role::Send);
        assert_eq!(role_for(true, Direction::Up), Role::Recv);
        assert_eq!(role_for(false, Direction::Down), Role::Recv);
        assert_eq!(role_for(true, Direction::Down), Role::Send);
        assert_eq!(role_for(true, Direction::Bidir), Role::Both);
        assert_eq!(role_for(false, Direction::Bidir), Role::Both);
    }

    #[test]
    fn avg_bytes_per_sec_basic() {
        assert_eq!(avg_bytes_per_sec(1_000_000, 1000), 1_000_000);
        assert_eq!(avg_bytes_per_sec(2_000_000, 1000), 2_000_000);
        assert_eq!(avg_bytes_per_sec(0, 0), 0); // 防除零
        assert_eq!(avg_bytes_per_sec(500_000, 500), 1_000_000);
    }
}
