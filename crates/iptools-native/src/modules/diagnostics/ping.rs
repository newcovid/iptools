//! Native Ping algorithm for the structured runtime.

use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
struct PingConfig {
    interval_ms: u64,
    timeout_ms: u64,
    packet_size: u64,
}

#[derive(Debug)]
enum PingEvent {
    Result {
        seq: u64,
        latency: u64,
        ttl: u8,
        size: usize,
    },
    Timeout {
        seq: u64,
    },
    Error {
        key: String,
        detail: String,
    },
}

/// Bridge the existing platform ping algorithm into the shared runtime protocol.
pub(crate) async fn run_shared(
    job: iptools_core::JobId,
    request: iptools_core::PingRequest,
    cancellation: CancellationToken,
    events: mpsc::Sender<iptools_core::RuntimeEvent>,
) -> Result<(), String> {
    use iptools_core::{RuntimeError, RuntimeErrorCode, RuntimeEvent};
    use std::net::IpAddr;

    if request.target.trim().is_empty() {
        events
            .send(RuntimeEvent::PingFailed {
                job,
                error: RuntimeError::new(
                    RuntimeErrorCode::InvalidRequest,
                    "target cannot be empty",
                ),
            })
            .await
            .map_err(|error| error.to_string())?;
        return Ok(());
    }
    let target = request.target.trim().to_string();
    let target_ip = match target.parse::<IpAddr>() {
        Ok(ip) => ip,
        Err(_) => match tokio::net::lookup_host((target.as_str(), 0)).await {
            Ok(mut values) => match values.next() {
                Some(value) => value.ip(),
                None => {
                    events
                        .send(RuntimeEvent::PingFailed {
                            job,
                            error: RuntimeError::new(
                                RuntimeErrorCode::ResolveTarget,
                                "target resolved to no addresses",
                            ),
                        })
                        .await
                        .map_err(|error| error.to_string())?;
                    return Ok(());
                }
            },
            Err(error) => {
                events
                    .send(RuntimeEvent::PingFailed {
                        job,
                        error: RuntimeError::new(
                            RuntimeErrorCode::ResolveTarget,
                            error.to_string(),
                        ),
                    })
                    .await
                    .map_err(|error| error.to_string())?;
                return Ok(());
            }
        },
    };

    events
        .send(RuntimeEvent::PingStarted { job })
        .await
        .map_err(|error| error.to_string())?;
    let config = PingConfig {
        interval_ms: request.interval_ms.clamp(100, 10_000),
        timeout_ms: request.timeout_ms.clamp(100, 10_000),
        packet_size: request.packet_size.min(65_500),
    };
    let (tx, mut rx) = mpsc::channel(32);
    let worker_cancellation = cancellation.child_token();
    let worker = run_shared_platform(target_ip, config, tx, worker_cancellation.clone());
    tokio::pin!(worker);
    let mut stats = SharedPingStats::default();
    let mut last_emit = None;

    loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                worker_cancellation.cancel();
                worker.await;
                return Ok(());
            }
            event = rx.recv() => {
                let Some(event) = event else { return Ok(()); };
                match event {
                    PingEvent::Result { seq, latency, ttl, size } => {
                        stats.observe(Some(latency));
                        let now = tokio::time::Instant::now();
                        if last_emit.is_none_or(|previous| now.duration_since(previous) >= Duration::from_millis(250)) {
                            last_emit = Some(now);
                            events.send(RuntimeEvent::PingSample { job, sample: stats.sample(seq, Some(latency), Some(ttl), size) }).await.map_err(|error| error.to_string())?;
                        }
                    }
                    PingEvent::Timeout { seq } => {
                        stats.observe(None);
                        let now = tokio::time::Instant::now();
                        if last_emit.is_none_or(|previous| now.duration_since(previous) >= Duration::from_millis(250)) {
                            last_emit = Some(now);
                            events.send(RuntimeEvent::PingSample { job, sample: stats.sample(seq, None, None, request.packet_size as usize) }).await.map_err(|error| error.to_string())?;
                        }
                    }
                    PingEvent::Error { key, detail } => {
                        worker_cancellation.cancel();
                        let permission_key = key == "diag_ping_err_perm";
                        let summary = match key.as_str() {
                            "diag_ping_err_dns_empty" => "target resolved to no addresses",
                            "diag_ping_err_dns" => "target resolution failed",
                            "diag_ping_err_ipv6" => "IPv6 is not supported by this ping backend",
                            "diag_ping_err_perm" => "raw socket permission denied",
                            _ => "ping request failed",
                        };
                        let message = if detail.is_empty() {
                            summary.into()
                        } else {
                            format!("{summary}: {detail}")
                        };
                        let code = if permission_key
                            || message.to_lowercase().contains("permission")
                            || message.contains("权限")
                        {
                            RuntimeErrorCode::PermissionDenied
                        } else {
                            RuntimeErrorCode::Network
                        };
                        events.send(RuntimeEvent::PingFailed { job, error: RuntimeError::new(code, message) }).await.map_err(|error| error.to_string())?;
                        worker.await;
                        return Ok(());
                    }
                }
            }
            _ = &mut worker => {
                return Ok(());
            }
        }
    }
}

#[derive(Default)]
struct SharedPingStats {
    sent: u64,
    received: u64,
    min_ms: Option<u64>,
    max_ms: Option<u64>,
    total_ms: u64,
}

impl SharedPingStats {
    fn observe(&mut self, latency: Option<u64>) {
        self.sent += 1;
        if let Some(latency) = latency {
            self.received += 1;
            self.min_ms = Some(self.min_ms.map_or(latency, |value| value.min(latency)));
            self.max_ms = Some(self.max_ms.map_or(latency, |value| value.max(latency)));
            self.total_ms = self.total_ms.saturating_add(latency);
        }
    }

    fn sample(
        &self,
        sequence: u64,
        latency_ms: Option<u64>,
        ttl: Option<u8>,
        size: usize,
    ) -> iptools_core::PingSample {
        iptools_core::PingSample {
            sequence,
            latency_ms,
            ttl,
            size,
            sent: self.sent,
            received: self.received,
            min_ms: self.min_ms,
            average_ms: (self.received > 0).then(|| self.total_ms as f64 / self.received as f64),
            max_ms: self.max_ms,
            loss_percent: if self.sent == 0 {
                0.0
            } else {
                (self.sent - self.received) as f64 * 100.0 / self.sent as f64
            },
        }
    }
}

#[cfg(target_os = "windows")]
async fn run_shared_platform(
    target: std::net::IpAddr,
    config: PingConfig,
    tx: mpsc::Sender<PingEvent>,
    cancellation: CancellationToken,
) {
    if let std::net::IpAddr::V4(target) = target {
        run_ping_windows(target, config, tx, cancellation).await;
    } else {
        let _ = tx
            .send(PingEvent::Error {
                key: "diag_ping_err_ipv6".into(),
                detail: String::new(),
            })
            .await;
    }
}

#[cfg(not(target_os = "windows"))]
async fn run_shared_platform(
    target: std::net::IpAddr,
    config: PingConfig,
    tx: mpsc::Sender<PingEvent>,
    cancellation: CancellationToken,
) {
    run_ping_unix(target, config, tx, cancellation).await;
}

#[cfg(target_os = "windows")]
async fn run_ping_windows(
    target_ip: std::net::Ipv4Addr,
    config: PingConfig,
    tx: mpsc::Sender<PingEvent>,
    abort: CancellationToken,
) {
    use std::ffi::c_void;
    use windows::Win32::NetworkManagement::IpHelper::{
        ICMP_ECHO_REPLY, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho,
    };

    let ip_u32 = u32::from_le_bytes(target_ip.octets());
    const REPLY_SIZE: usize = 2048 + 65535; // 足够大的缓冲区
    let mut seq = 0;

    loop {
        if abort.is_cancelled() {
            break;
        }

        let payload = vec![0u8; config.packet_size as usize];
        let timeout = config.timeout_ms as u32;

        let ping_task_result = tokio::task::spawn_blocking(move || {
            let handle = unsafe { IcmpCreateFile() }.map_err(|e| e.to_string())?;

            let mut reply_buffer = vec![0u8; REPLY_SIZE];

            let ret_count = unsafe {
                IcmpSendEcho(
                    handle,
                    ip_u32,
                    payload.as_ptr() as *const c_void,
                    payload.len() as u16,
                    None,
                    reply_buffer.as_mut_ptr() as *mut c_void,
                    REPLY_SIZE as u32,
                    timeout,
                )
            };

            // 修复：处理 CloseHandle 的返回值，使用 let _ = ... 忽略
            unsafe {
                let _ = IcmpCloseHandle(handle);
            };

            Ok::<(u32, Vec<u8>, usize), String>((ret_count, reply_buffer, payload.len()))
        })
        .await;

        match ping_task_result {
            Ok(Ok((count, reply_buf, sent_size))) => {
                if count > 0 {
                    let reply = unsafe { &*(reply_buf.as_ptr() as *const ICMP_ECHO_REPLY) };
                    if reply.Status == 0 {
                        let _ = tx
                            .send(PingEvent::Result {
                                seq,
                                latency: reply.RoundTripTime as u64,
                                ttl: reply.Options.Ttl,
                                size: sent_size,
                            })
                            .await;
                    } else {
                        let _ = tx.send(PingEvent::Timeout { seq }).await;
                    }
                } else {
                    let _ = tx.send(PingEvent::Timeout { seq }).await;
                }
            }
            Ok(Err(e)) => {
                let _ = tx
                    .send(PingEvent::Error {
                        key: "diag_ping_err_generic".into(),
                        detail: e,
                    })
                    .await;
                break;
            }
            Err(_) => {
                break;
            }
        }

        tokio::select! {
            _ = abort.cancelled() => break,
            _ = tokio::time::sleep(Duration::from_millis(config.interval_ms)) => {}
        }
        seq += 1;
    }
}

#[cfg(not(target_os = "windows"))]
async fn run_ping_unix(
    target_ip: std::net::IpAddr,
    config: PingConfig,
    tx: mpsc::Sender<PingEvent>,
    abort: CancellationToken,
) {
    let payload = vec![0u8; config.packet_size as usize];
    // surge-ping 0.8：先建 Client（ICMP 套接字，需 root/CAP_NET_RAW），再按目标地址族建 Pinger。
    let cfg = if target_ip.is_ipv6() {
        surge_ping::Config::builder()
            .kind(surge_ping::ICMP::V6)
            .build()
    } else {
        surge_ping::Config::default()
    };
    let client = match surge_ping::Client::new(&cfg) {
        Ok(c) => c,
        Err(e) => {
            let evt = if e.to_string().contains("Permission") {
                PingEvent::Error {
                    key: "diag_ping_err_perm".into(),
                    detail: String::new(),
                }
            } else {
                PingEvent::Error {
                    key: "diag_ping_err_generic".into(),
                    detail: e.to_string(),
                }
            };
            let _ = tx.send(evt).await;
            return;
        }
    };
    // 标识符固定（取进程低 16 位），序列号每次自增，匹配回包。
    let ident = surge_ping::PingIdentifier((std::process::id() & 0xFFFF) as u16);
    let mut pinger = client.pinger(target_ip, ident).await;
    pinger.timeout(Duration::from_millis(config.timeout_ms));

    let mut seq = 0;
    let mut interval = tokio::time::interval(Duration::from_millis(config.interval_ms));

    loop {
        if abort.is_cancelled() {
            break;
        }
        tokio::select! {
            _ = abort.cancelled() => break,
            _ = interval.tick() => {}
        }

        let result = tokio::select! {
            _ = abort.cancelled() => break,
            result = pinger.ping(surge_ping::PingSequence(seq as u16), &payload) => result,
        };
        match result {
            Ok((_packet, duration)) => {
                let ms = duration.as_millis() as u64;
                let _ = tx
                    .send(PingEvent::Result {
                        seq,
                        latency: ms,
                        ttl: 64,
                        size: payload.len(),
                    })
                    .await;
            }
            Err(_) => {
                let _ = tx.send(PingEvent::Timeout { seq }).await;
            }
        }
        seq += 1;
    }
}

#[cfg(test)]
mod shared_tests {
    use super::*;

    #[test]
    fn coalesced_samples_keep_cumulative_loss_statistics() {
        let mut stats = SharedPingStats::default();
        stats.observe(Some(10));
        stats.observe(None);
        stats.observe(Some(20));
        let sample = stats.sample(2, Some(20), Some(64), 32);
        assert_eq!(sample.sent, 3);
        assert_eq!(sample.received, 2);
        assert_eq!(sample.min_ms, Some(10));
        assert_eq!(sample.max_ms, Some(20));
        assert_eq!(sample.average_ms, Some(15.0));
        assert!((sample.loss_percent - 33.333).abs() < 0.01);
    }
}
