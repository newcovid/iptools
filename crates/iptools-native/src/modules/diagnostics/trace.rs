//! Native IPv4 trace-route algorithm for the structured runtime.

use std::net::Ipv4Addr;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
struct Hop {
    ttl: u8,
    addr: Option<Ipv4Addr>,
    rtt_ms: Option<u64>,
    host: Option<String>,
}

#[derive(Debug)]
enum TraceEvent {
    Hop(Hop),
    Done,
    Error(String),
}

/// Bridge the existing cross-platform trace algorithm into the shared runtime protocol.
pub(crate) async fn run_shared(
    job: iptools_core::JobId,
    request: iptools_core::TraceRequest,
    cancellation: CancellationToken,
    events: mpsc::Sender<iptools_core::RuntimeEvent>,
) -> Result<(), String> {
    use iptools_core::{RuntimeError, RuntimeErrorCode, RuntimeEvent, TraceHop};

    if request.target.trim().is_empty() {
        events
            .send(RuntimeEvent::TraceFailed {
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
    events
        .send(RuntimeEvent::TraceStarted { job })
        .await
        .map_err(|error| error.to_string())?;
    let (tx, mut rx) = mpsc::channel(16);
    let worker_cancellation = cancellation.child_token();
    let worker = run_trace(
        request.target.trim().to_string(),
        u32::from(request.max_hops.clamp(1, 64)),
        request.timeout_ms.clamp(100, 10_000) as u32,
        tx,
        worker_cancellation.clone(),
    );
    tokio::pin!(worker);
    let mut hops = 0u8;
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
                    TraceEvent::Hop(hop) => {
                        hops = hops.max(hop.ttl);
                        events.send(RuntimeEvent::TraceHop {
                            job,
                            hop: TraceHop {
                                ttl: hop.ttl,
                                address: hop.addr.map(|value| value.to_string()),
                                hostname: hop.host,
                                latency_ms: hop.rtt_ms,
                            },
                        }).await.map_err(|error| error.to_string())?;
                    }
                    TraceEvent::Done => {
                        events.send(RuntimeEvent::TraceFinished { job, hops }).await.map_err(|error| error.to_string())?;
                        worker_cancellation.cancel();
                        worker.await;
                        return Ok(());
                    }
                    TraceEvent::Error(message) => {
                        let message = if message.starts_with("diag_") {
                            "target resolution or trace probe failed".into()
                        } else {
                            message
                        };
                        events.send(RuntimeEvent::TraceFailed {
                            job,
                            error: RuntimeError::new(RuntimeErrorCode::ResolveTarget, message),
                        }).await.map_err(|error| error.to_string())?;
                        worker_cancellation.cancel();
                        worker.await;
                        return Ok(());
                    }
                }
            }
            _ = &mut worker => return Ok(()),
        }
    }
}

// Windows 与 unix（Linux）共用同一逐跳逻辑：仅依赖跨平台的 icmp::echo_once、tokio、
// mpsc 与 dns_lookup，无平台专属符号。unix 的 echo_once 由 socket2 raw 套接字实现。
#[cfg(any(target_os = "windows", unix))]
async fn run_trace(
    target: String,
    max_hops: u32,
    timeout_ms: u32,
    tx: mpsc::Sender<TraceEvent>,
    abort: CancellationToken,
) {
    use std::net::IpAddr;

    // 解析目标为 IPv4
    let dest_v4: Ipv4Addr = match target.parse::<IpAddr>() {
        Ok(IpAddr::V4(v4)) => v4,
        Ok(IpAddr::V6(_)) => {
            let _ = tx.send(TraceEvent::Error("diag_trace_err".into())).await;
            return;
        }
        Err(_) => match tokio::net::lookup_host((target.as_str(), 0u16)).await {
            Ok(mut it) => loop {
                match it.next() {
                    Some(sa) => {
                        if let IpAddr::V4(v4) = sa.ip() {
                            break v4;
                        }
                    }
                    None => {
                        let _ = tx.send(TraceEvent::Error("diag_trace_err".into())).await;
                        return;
                    }
                }
            },
            Err(_) => {
                let _ = tx.send(TraceEvent::Error("diag_trace_err".into())).await;
                return;
            }
        },
    };

    for ttl in 1..=max_hops as u8 {
        if abort.is_cancelled() {
            return;
        }

        let probe =
            tokio::task::spawn_blocking(move || super::icmp::echo_once(dest_v4, ttl, timeout_ms))
                .await;
        let result = match probe {
            Ok(v) => v,
            Err(_) => return,
        };

        let addr = result.addr;

        // 反向 DNS（best-effort，不阻塞 UI）
        let host = if let Some(a) = addr {
            tokio::task::spawn_blocking(move || {
                dns_lookup::lookup_addr(&std::net::IpAddr::V4(a)).ok()
            })
            .await
            .ok()
            .flatten()
        } else {
            None
        };

        let reached = addr == Some(dest_v4) || result.reached();

        let _ = tx
            .send(TraceEvent::Hop(Hop {
                ttl,
                addr,
                rtt_ms: result.rtt_ms,
                host,
            }))
            .await;

        if reached {
            let _ = tx.send(TraceEvent::Done).await;
            return;
        }
    }

    let _ = tx.send(TraceEvent::Done).await;
}
