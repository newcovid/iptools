//! 内网测速线路协议与测量逻辑。
//!
//! 控制头用 JSON（长度前缀），UDP 数据报头用紧凑二进制。收发 worker 用纯
//! tokio TCP/UDP，跨平台。纯逻辑（编解码 / 丢包统计 / 速率换算）带单元测试。

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const DEFAULT_PORT: u16 = 50505;
pub const UDP_HEADER_LEN: usize = 18; // u16 stream_id + u64 seq + u64 send_ts_nanos
/// 注册报文哨兵 seq（不计入统计，仅用于让服务端学习客户端地址）。
pub const REG_SEQ: u64 = u64::MAX;

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

pub fn write_udp_header(buf: &mut [u8], stream_id: u16, seq: u64, send_ts_nanos: u64) {
    buf[0..2].copy_from_slice(&stream_id.to_le_bytes());
    buf[2..10].copy_from_slice(&seq.to_le_bytes());
    buf[10..18].copy_from_slice(&send_ts_nanos.to_le_bytes());
}

pub fn read_udp_header(buf: &[u8]) -> Option<(u16, u64, u64)> {
    if buf.len() < UDP_HEADER_LEN {
        return None;
    }
    let sid = u16::from_le_bytes([buf[0], buf[1]]);
    let seq = u64::from_le_bytes(buf[2..10].try_into().ok()?);
    let ts = u64::from_le_bytes(buf[10..18].try_into().ok()?);
    Some((sid, seq, ts))
}

/// 单条 UDP 流的接收统计（丢包/乱序/抖动）。
#[derive(Debug, Default, Clone)]
pub struct StreamTracker {
    pub received: u64,
    pub max_seq: u64,
    pub last_seq: Option<u64>,
    pub out_of_order: u64,
    jitter_ns: f64,
    last_transit_ns: Option<i64>,
}

impl StreamTracker {
    pub fn on_packet(&mut self, seq: u64, send_ts_nanos: u64, arrival_nanos: u64) {
        self.received += 1;
        if let Some(last) = self.last_seq {
            if seq < last {
                self.out_of_order += 1;
            }
        }
        self.last_seq = Some(seq);
        if seq > self.max_seq {
            self.max_seq = seq;
        }
        // RFC3550 式抖动：相邻包传输时延差的滑动平均。
        let transit = arrival_nanos as i64 - send_ts_nanos as i64;
        if let Some(prev) = self.last_transit_ns {
            let d = (transit - prev).abs() as f64;
            self.jitter_ns += (d - self.jitter_ns) / 16.0;
        }
        self.last_transit_ns = Some(transit);
    }

    pub fn lost(&self) -> u64 {
        if self.received == 0 {
            return 0;
        }
        (self.max_seq + 1).saturating_sub(self.received)
    }

    pub fn jitter_ms(&self) -> f64 {
        self.jitter_ns / 1_000_000.0
    }
}

/// 多条流聚合为一个 UdpSummary。
pub fn aggregate_udp(trackers: &[StreamTracker]) -> UdpSummary {
    let mut s = UdpSummary::default();
    let (mut jsum, mut jn) = (0.0f64, 0u64);
    for t in trackers {
        s.received += t.received;
        s.lost += t.lost();
        s.out_of_order += t.out_of_order;
        if t.received > 1 {
            jsum += t.jitter_ms();
            jn += 1;
        }
    }
    s.jitter_ms = if jn > 0 { jsum / jn as f64 } else { 0.0 };
    s
}

/// 每包发送间隔（纳秒）；rate_mbps=0 或 payload=0 → 0（不限速）。
pub fn packet_interval_ns(rate_mbps: u32, payload_size: u32) -> u64 {
    if rate_mbps == 0 || payload_size == 0 {
        return 0;
    }
    let bits = payload_size as u64 * 8;
    let rate_bps = rate_mbps as u64 * 1_000_000;
    bits.saturating_mul(1_000_000_000) / rate_bps
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
            let sock = match UdpSocket::bind(("0.0.0.0", port)).await {
                Ok(s) => s,
                Err(_) => {
                    let _ = tx.send(LanEvent::Error("diag_lan_err".into())).await;
                    return;
                }
            };
            run_udp_server_socket(sock, spec, tx, abort).await;
            drop(ctrl);
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
            run_udp_client(peer, port, spec, tx, abort).await;
            drop(ctrl);
        }
    }
}

use std::collections::HashMap;
use std::net::SocketAddr;
use tokio::net::UdpSocket;

fn now_nanos() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// 服务端 UDP：用已绑定的 socket。收数据计入 per-stream tracker；
/// 若方向需要本端发送（Down/Bidir），用注册报文学到的客户端地址回发。
pub(crate) async fn run_udp_server_socket(
    sock: UdpSocket,
    spec: TestSpec,
    tx: mpsc::Sender<LanEvent>,
    abort: Arc<Mutex<bool>>,
) {
    let role = role_for(true, spec.direction);
    run_udp_session(sock, role, spec, None, tx, abort).await;
}

/// 客户端 UDP：自绑定 socket，连到服务端。
pub async fn run_udp_client(
    peer_ip: String,
    port: u16,
    spec: TestSpec,
    tx: mpsc::Sender<LanEvent>,
    abort: Arc<Mutex<bool>>,
) {
    let sock = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => s,
        Err(_) => {
            let _ = tx.send(LanEvent::Error("diag_lan_err".into())).await;
            return;
        }
    };
    let dest: SocketAddr = match format!("{}:{}", peer_ip, port).parse() {
        Ok(a) => a,
        Err(_) => match tokio::net::lookup_host((peer_ip.as_str(), port)).await {
            Ok(mut it) => match it.next() {
                Some(a) => a,
                None => {
                    let _ = tx.send(LanEvent::Error("diag_lan_err".into())).await;
                    return;
                }
            },
            Err(_) => {
                let _ = tx.send(LanEvent::Error("diag_lan_err".into())).await;
                return;
            }
        },
    };
    let role = role_for(false, spec.direction);
    run_udp_session(sock, role, spec, Some(dest), tx, abort).await;
}

/// UDP 会话核心：单 socket，按角色并发收/发。
/// `dest` 为客户端已知的服务端地址；服务端为 None（从注册报文学客户端地址）。
async fn run_udp_session(
    sock: UdpSocket,
    role: Role,
    spec: TestSpec,
    dest: Option<SocketAddr>,
    tx: mpsc::Sender<LanEvent>,
    abort: Arc<Mutex<bool>>,
) {
    let sock = Arc::new(sock);
    let tx_bytes = Arc::new(AtomicU64::new(0));
    let rx_bytes = Arc::new(AtomicU64::new(0));
    let _ = tx
        .send(LanEvent::Status("diag_lan_status_connected".into()))
        .await;

    let start = Instant::now();
    let deadline = start + Duration::from_millis(spec.duration_ms);
    let has_tx = matches!(role, Role::Send | Role::Both);
    let has_rx = matches!(role, Role::Recv | Role::Both);
    let payload = spec.payload_size.max(UDP_HEADER_LEN as u32) as usize;
    let n_streams = spec.streams.max(1);

    // 客户端：先发注册报文（让服务端学到地址）。
    if let Some(d) = dest {
        let mut hdr = vec![0u8; payload];
        for sid in 0..n_streams {
            write_udp_header(&mut hdr, sid, REG_SEQ, 0);
            let _ = sock.send_to(&hdr, d).await;
        }
    }

    let peers: Arc<Mutex<HashMap<u16, SocketAddr>>> = Arc::new(Mutex::new(HashMap::new()));
    let trackers: Arc<Mutex<HashMap<u16, StreamTracker>>> = Arc::new(Mutex::new(HashMap::new()));

    // 接收任务（总是开：服务端纯发送也要收注册报文；统计仅在 has_rx 时累加）。
    let recv_task = {
        let sock = sock.clone();
        let rxb = rx_bytes.clone();
        let peers = peers.clone();
        let trackers = trackers.clone();
        let abort = abort.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 1 << 16];
            loop {
                if *abort.lock().unwrap() {
                    break;
                }
                match tokio::time::timeout(Duration::from_millis(500), sock.recv_from(&mut buf)).await
                {
                    Ok(Ok((n, from))) => {
                        if let Some((sid, seq, ts)) = read_udp_header(&buf[..n]) {
                            if seq == REG_SEQ {
                                peers.lock().unwrap().insert(sid, from);
                            } else if has_rx {
                                rxb.fetch_add(n as u64, Ordering::Relaxed);
                                let arr = now_nanos();
                                let mut t = trackers.lock().unwrap();
                                t.entry(sid).or_default().on_packet(seq, ts, arr);
                            }
                        }
                    }
                    Ok(Err(_)) => break,
                    Err(_) => {} // 读超时
                }
                if Instant::now() >= deadline {
                    break;
                }
            }
        })
    };

    // 发送任务（has_tx 时）：每流一个，定速。
    let mut send_handles = Vec::new();
    if has_tx {
        // 服务端需等注册报文到达以学到回发地址。
        if dest.is_none() {
            let pdead = Instant::now() + Duration::from_millis(1000);
            loop {
                if peers.lock().unwrap().len() as u16 >= n_streams || Instant::now() >= pdead {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
        let interval = packet_interval_ns(spec.rate_mbps, payload as u32);
        for sid in 0..n_streams {
            let target = match dest {
                Some(d) => Some(d),
                None => peers.lock().unwrap().get(&sid).copied(),
            };
            let target = match target {
                Some(t) => t,
                None => continue,
            };
            let sock = sock.clone();
            let txb = tx_bytes.clone();
            let abort = abort.clone();
            send_handles.push(tokio::spawn(async move {
                let mut buf = vec![0u8; payload];
                let mut seq = 0u64;
                loop {
                    if *abort.lock().unwrap() {
                        break;
                    }
                    write_udp_header(&mut buf, sid, seq, now_nanos());
                    match sock.send_to(&buf, target).await {
                        Ok(_) => {
                            txb.fetch_add(payload as u64, Ordering::Relaxed);
                            seq += 1;
                        }
                        Err(_) => break,
                    }
                    if Instant::now() >= deadline {
                        break;
                    }
                    if interval > 0 {
                        tokio::time::sleep(Duration::from_nanos(interval)).await;
                    }
                }
            }));
        }
    }

    // 采样上报
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
                    let _ = rtx
                        .send(LanEvent::Progress {
                            flow: Flow::Tx,
                            total_bytes: cur_tx,
                            elapsed_ms: elapsed,
                            inst_bps: ((cur_tx - last_tx) as f64 / since) as u64,
                        })
                        .await;
                }
                if has_rx {
                    let _ = rtx
                        .send(LanEvent::Progress {
                            flow: Flow::Rx,
                            total_bytes: cur_rx,
                            elapsed_ms: elapsed,
                            inst_bps: ((cur_rx - last_rx) as f64 / since) as u64,
                        })
                        .await;
                }
                last = now;
                last_tx = cur_tx;
                last_rx = cur_rx;
            }
        })
    };

    for h in send_handles {
        let _ = h.await;
    }
    let _ = recv_task.await;
    let _ = reporter.await;

    let elapsed = start.elapsed().as_millis() as u64;
    let udp = if has_rx {
        let t = trackers.lock().unwrap();
        let vec: Vec<StreamTracker> = t.values().cloned().collect();
        Some(aggregate_udp(&vec))
    } else {
        None
    };
    let _ = tx
        .send(LanEvent::Summary(TestSummary {
            tx_bytes: tx_bytes.load(Ordering::Relaxed),
            rx_bytes: rx_bytes.load(Ordering::Relaxed),
            elapsed_ms: elapsed,
            udp,
        }))
        .await;
    let _ = tx.send(LanEvent::Status("diag_lan_done".into())).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::net::{TcpListener, TcpStream, UdpSocket};
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

    #[test]
    fn udp_header_roundtrip() {
        let mut buf = [0u8; 64];
        write_udp_header(&mut buf, 3, 42, 1_234_567_890);
        let (sid, seq, ts) = read_udp_header(&buf).unwrap();
        assert_eq!((sid, seq, ts), (3, 42, 1_234_567_890));
        assert!(read_udp_header(&[0u8; 4]).is_none());
    }

    #[test]
    fn tracker_counts_loss_and_reorder() {
        let mut t = StreamTracker::default();
        // 收到 seq 0,1,3,2 → max=3,收4 → 期望4 收4 丢0；2 在 3 之后到达=乱序1
        for (seq, ts, arr) in [(0u64, 0u64, 10u64), (1, 1, 11), (3, 3, 13), (2, 2, 14)] {
            t.on_packet(seq, ts, arr);
        }
        assert_eq!(t.received, 4);
        assert_eq!(t.out_of_order, 1);
        assert_eq!(t.lost(), 0);
    }

    #[test]
    fn tracker_detects_gap() {
        let mut t = StreamTracker::default();
        for seq in [0u64, 1, 2, 5] {
            t.on_packet(seq, seq, seq + 10);
        }
        assert_eq!(t.received, 4);
        assert_eq!(t.lost(), 2); // 期望 6，收 4
    }

    #[test]
    fn packet_interval_math() {
        // 8 Mbps, 1000 字节=8000 bit → 1ms = 1_000_000 ns
        assert_eq!(packet_interval_ns(8, 1000), 1_000_000);
        assert_eq!(packet_interval_ns(0, 1000), 0); // 无限速
        assert_eq!(packet_interval_ns(100, 0), 0);
    }

    #[tokio::test]
    async fn udp_session_up_counts_packets() {
        let srv = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let srv_addr = srv.local_addr().unwrap();
        let spec = TestSpec {
            proto: Proto::Udp,
            direction: Direction::Up,
            duration_ms: 300,
            streams: 1,
            rate_mbps: 0,
            payload_size: 1200,
        };
        let sspec = spec.clone();
        let server = tokio::spawn(async move {
            let (tx, mut rx) = mpsc::channel(512);
            let abort = Arc::new(Mutex::new(false));
            run_udp_server_socket(srv, sspec, tx, abort).await;
            drain_summary(&mut rx).await
        });

        let (ctx, _crx) = mpsc::channel(512);
        let cabort = Arc::new(Mutex::new(false));
        run_udp_client(srv_addr.ip().to_string(), srv_addr.port(), spec, ctx, cabort).await;

        let s = server.await.unwrap().unwrap();
        let u = s.udp.unwrap();
        assert!(u.received > 0, "server should receive udp packets");
    }
}
