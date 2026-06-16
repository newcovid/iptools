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
/// `encode_frame` 的对称对照实现；网络路径走 `read_frame`（流式），此函数主要供
/// 单测校验帧格式，故标 `allow(dead_code)` 保持构建无警告。
#[allow(dead_code)]
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

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::mpsc;

const RECV_BUF: usize = 64 * 1024;

/// 多流 TCP 会话：按角色起每条连接的 send/recv，250ms 采样上报，结束发 Summary。
pub(crate) async fn run_tcp_session(
    conns: Vec<TcpStream>,
    role: Role,
    spec: TestSpec,
    tx: mpsc::Sender<LanEvent>,
    abort: Arc<Mutex<bool>>,
) {
    let tx_bytes = Arc::new(AtomicU64::new(0));
    let rx_bytes = Arc::new(AtomicU64::new(0));
    let _ = tx
        .send(LanEvent::Status("diag_lan_status_connected".into()))
        .await;

    let start = Instant::now();
    let deadline = start + Duration::from_millis(spec.duration_ms);
    let has_tx = matches!(role, Role::Send | Role::Both);
    let has_rx = matches!(role, Role::Recv | Role::Both);

    // 采样上报任务
    let reporter = {
        let rtx = tx.clone();
        let rtxb = tx_bytes.clone();
        let rrxb = rx_bytes.clone();
        let rabort = abort.clone();
        tokio::spawn(async move {
            let mut last = Instant::now();
            let (mut last_tx, mut last_rx) = (0u64, 0u64);
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                if *rabort.lock().unwrap() || Instant::now() >= deadline {
                    break;
                }
                let now = Instant::now();
                let since = now.duration_since(last).as_secs_f64().max(0.001);
                let cur_tx = rtxb.load(Ordering::Relaxed);
                let cur_rx = rrxb.load(Ordering::Relaxed);
                let elapsed = now.duration_since(start).as_millis() as u64;
                if has_tx {
                    let inst = ((cur_tx - last_tx) as f64 / since) as u64;
                    let _ = rtx
                        .send(LanEvent::Progress {
                            flow: Flow::Tx,
                            total_bytes: cur_tx,
                            elapsed_ms: elapsed,
                            inst_bps: inst,
                        })
                        .await;
                }
                if has_rx {
                    let inst = ((cur_rx - last_rx) as f64 / since) as u64;
                    let _ = rtx
                        .send(LanEvent::Progress {
                            flow: Flow::Rx,
                            total_bytes: cur_rx,
                            elapsed_ms: elapsed,
                            inst_bps: inst,
                        })
                        .await;
                }
                last = now;
                last_tx = cur_tx;
                last_rx = cur_rx;
            }
        })
    };

    let payload = spec.payload_size.max(1) as usize;
    let mut handles = Vec::new();
    for stream in conns {
        let txb = tx_bytes.clone();
        let rxb = rx_bytes.clone();
        let ab = abort.clone();
        handles.push(tokio::spawn(tcp_conn_worker(
            stream, role, payload, deadline, txb, rxb, ab,
        )));
    }
    for h in handles {
        let _ = h.await;
    }
    let _ = reporter.await;

    let elapsed = start.elapsed().as_millis() as u64;
    let _ = tx
        .send(LanEvent::Summary(TestSummary {
            tx_bytes: tx_bytes.load(Ordering::Relaxed),
            rx_bytes: rx_bytes.load(Ordering::Relaxed),
            elapsed_ms: elapsed,
            udp: None,
        }))
        .await;
    let _ = tx.send(LanEvent::Status("diag_lan_done".into())).await;
}

async fn tcp_conn_worker(
    stream: TcpStream,
    role: Role,
    payload: usize,
    deadline: Instant,
    tx_bytes: Arc<AtomicU64>,
    rx_bytes: Arc<AtomicU64>,
    abort: Arc<Mutex<bool>>,
) {
    let (rd, wr) = stream.into_split();
    match role {
        Role::Send => tcp_send_half(wr, payload, deadline, tx_bytes, abort).await,
        Role::Recv => tcp_recv_half(rd, deadline, rx_bytes, abort).await,
        Role::Both => {
            let ab2 = abort.clone();
            let h1 = tokio::spawn(tcp_send_half(wr, payload, deadline, tx_bytes, abort));
            let h2 = tokio::spawn(tcp_recv_half(rd, deadline, rx_bytes, ab2));
            let _ = h1.await;
            let _ = h2.await;
        }
    }
}

async fn tcp_send_half(
    mut wr: OwnedWriteHalf,
    payload: usize,
    deadline: Instant,
    tx_bytes: Arc<AtomicU64>,
    abort: Arc<Mutex<bool>>,
) {
    let buf = vec![0u8; payload];
    loop {
        if *abort.lock().unwrap() {
            break;
        }
        // 先发一次再判 deadline：保证每条流至少写一个 payload，
        // 避免 worker 被调度到 deadline 之后就零字节退出（短时长/高负载下会发生）。
        match wr.write_all(&buf).await {
            Ok(_) => {
                tx_bytes.fetch_add(payload as u64, Ordering::Relaxed);
            }
            Err(_) => break,
        }
        if Instant::now() >= deadline {
            break;
        }
    }
    let _ = wr.shutdown().await;
}

async fn tcp_recv_half(
    mut rd: OwnedReadHalf,
    deadline: Instant,
    rx_bytes: Arc<AtomicU64>,
    abort: Arc<Mutex<bool>>,
) {
    let mut buf = vec![0u8; RECV_BUF];
    loop {
        if *abort.lock().unwrap() {
            break;
        }
        // 先收一次（带 500ms 超时）再判 deadline：即便本 worker 启动偏晚，
        // 也能把对端已写入 TCP 缓冲的数据读出来，避免零字节退出。
        match tokio::time::timeout(Duration::from_millis(500), rd.read(&mut buf)).await {
            Ok(Ok(0)) => break,
            Ok(Ok(n)) => {
                rx_bytes.fetch_add(n as u64, Ordering::Relaxed);
            }
            Ok(Err(_)) => break,
            Err(_) => {} // 读超时：落到下方判 deadline
        }
        if Instant::now() >= deadline {
            break;
        }
    }
}

use tokio::net::TcpListener;

/// 接受一条连接，期间轮询 abort；中止或出错返回 None。
async fn accept_with_abort(
    listener: &TcpListener,
    abort: &Arc<Mutex<bool>>,
) -> Option<TcpStream> {
    loop {
        if *abort.lock().unwrap() {
            return None;
        }
        match tokio::time::timeout(Duration::from_millis(500), listener.accept()).await {
            Ok(Ok((s, _))) => return Some(s),
            Ok(Err(_)) => return None,
            Err(_) => continue,
        }
    }
}

/// 服务端：监听端口，第一条连接是控制连接（读 spec、回 Ack），
/// 随后按 spec.proto 建数据通道并跑会话。
pub async fn run_server(port: u16, tx: mpsc::Sender<LanEvent>, abort: Arc<Mutex<bool>>) {
    let listener = match TcpListener::bind(("0.0.0.0", port)).await {
        Ok(l) => l,
        Err(_) => {
            let _ = tx.send(LanEvent::Error("diag_lan_err".into())).await;
            return;
        }
    };
    let _ = tx
        .send(LanEvent::Status("diag_lan_status_listening".into()))
        .await;

    let mut ctrl = match accept_with_abort(&listener, &abort).await {
        Some(s) => s,
        None => return,
    };
    let spec: TestSpec = match read_frame(&mut ctrl).await {
        Some(s) => s,
        None => {
            let _ = tx.send(LanEvent::Error("diag_lan_err".into())).await;
            return;
        }
    };
    let _ = write_frame(&mut ctrl, &Ack { ok: true }).await;

    match spec.proto {
        Proto::Tcp => {
            let role = role_for(true, spec.direction);
            let n = spec.streams.max(1) as usize;
            let mut conns = Vec::new();
            for _ in 0..n {
                match accept_with_abort(&listener, &abort).await {
                    Some(s) => conns.push(s),
                    None => return,
                }
            }
            run_tcp_session(conns, role, spec, tx, abort).await;
        }
        Proto::Udp => {
            // 后续任务实现；当前回报不支持以免静默卡住。
            let _ = tx
                .send(LanEvent::Error("diag_lan_udp_todo".into()))
                .await;
        }
    }
}

/// 客户端：连控制端口、发 spec、收 Ack，再开 N 条数据连接跑会话。
pub async fn run_client(
    peer: String,
    port: u16,
    spec: TestSpec,
    tx: mpsc::Sender<LanEvent>,
    abort: Arc<Mutex<bool>>,
) {
    if peer.is_empty() {
        let _ = tx.send(LanEvent::Error("diag_lan_err".into())).await;
        return;
    }
    let _ = tx
        .send(LanEvent::Status("diag_lan_status_connecting".into()))
        .await;

    let mut ctrl = match TcpStream::connect((peer.as_str(), port)).await {
        Ok(s) => s,
        Err(_) => {
            let _ = tx.send(LanEvent::Error("diag_lan_err".into())).await;
            return;
        }
    };
    if write_frame(&mut ctrl, &spec).await.is_err() {
        let _ = tx.send(LanEvent::Error("diag_lan_err".into())).await;
        return;
    }
    let _ack: Ack = match read_frame(&mut ctrl).await {
        Some(a) => a,
        None => {
            let _ = tx.send(LanEvent::Error("diag_lan_err".into())).await;
            return;
        }
    };

    match spec.proto {
        Proto::Tcp => {
            let role = role_for(false, spec.direction);
            let n = spec.streams.max(1) as usize;
            let mut conns = Vec::new();
            for _ in 0..n {
                match TcpStream::connect((peer.as_str(), port)).await {
                    Ok(s) => conns.push(s),
                    Err(_) => {
                        let _ = tx.send(LanEvent::Error("diag_lan_err".into())).await;
                        return;
                    }
                }
            }
            run_tcp_session(conns, role, spec, tx, abort).await;
        }
        Proto::Udp => {
            let _ = tx
                .send(LanEvent::Error("diag_lan_udp_todo".into()))
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::net::{TcpListener, TcpStream};
    use tokio::sync::mpsc;

    async fn drain_summary(rx: &mut mpsc::Receiver<LanEvent>) -> Option<TestSummary> {
        let mut s = None;
        while let Ok(ev) = rx.try_recv() {
            if let LanEvent::Summary(sm) = ev {
                s = Some(sm);
            }
        }
        s
    }

    #[tokio::test]
    async fn tcp_session_up_transfers_bytes() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let spec = TestSpec {
            proto: Proto::Tcp,
            direction: Direction::Up,
            duration_ms: 300,
            streams: 2,
            rate_mbps: 0,
            payload_size: 4096,
        };
        let sspec = spec.clone();
        let server = tokio::spawn(async move {
            let mut conns = Vec::new();
            for _ in 0..2 {
                conns.push(listener.accept().await.unwrap().0);
            }
            let (tx, mut rx) = mpsc::channel(256);
            let abort = Arc::new(Mutex::new(false));
            run_tcp_session(conns, role_for(true, Direction::Up), sspec, tx, abort).await;
            drain_summary(&mut rx).await
        });

        let mut conns = Vec::new();
        for _ in 0..2 {
            conns.push(TcpStream::connect(addr).await.unwrap());
        }
        let (ctx, _crx) = mpsc::channel(256);
        let cabort = Arc::new(Mutex::new(false));
        run_tcp_session(conns, role_for(false, Direction::Up), spec, ctx, cabort).await;

        let summary = server.await.unwrap().unwrap();
        assert!(summary.rx_bytes > 0, "server should have received bytes");
    }

    #[tokio::test]
    async fn tcp_session_bidir_transfers_both_ways() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let spec = TestSpec {
            proto: Proto::Tcp,
            direction: Direction::Bidir,
            duration_ms: 300,
            streams: 1,
            rate_mbps: 0,
            payload_size: 4096,
        };
        let sspec = spec.clone();
        let server = tokio::spawn(async move {
            let conn = listener.accept().await.unwrap().0;
            let (tx, mut rx) = mpsc::channel(256);
            let abort = Arc::new(Mutex::new(false));
            run_tcp_session(vec![conn], role_for(true, Direction::Bidir), sspec, tx, abort).await;
            drain_summary(&mut rx).await
        });
        let conn = TcpStream::connect(addr).await.unwrap();
        let (ctx, mut crx) = mpsc::channel(256);
        let cabort = Arc::new(Mutex::new(false));
        run_tcp_session(vec![conn], role_for(false, Direction::Bidir), spec, ctx, cabort).await;
        let client_sum = drain_summary(&mut crx).await.unwrap();
        let server_sum = server.await.unwrap().unwrap();
        assert!(client_sum.tx_bytes > 0 && client_sum.rx_bytes > 0);
        assert!(server_sum.tx_bytes > 0 && server_sum.rx_bytes > 0);
    }

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
