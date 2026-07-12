//! Native IPv4 trace-route algorithm for the structured runtime.

use std::{future::Future, net::Ipv4Addr};
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
    use iptools_core::{RuntimeError, RuntimeErrorCode, RuntimeEvent};

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
    let (tx, rx) = mpsc::channel(16);
    let worker_cancellation = cancellation.child_token();
    let worker = run_trace(
        request.target.trim().to_string(),
        u32::from(request.max_hops.clamp(1, 64)),
        request.timeout_ms.clamp(100, 10_000) as u32,
        tx,
        worker_cancellation.clone(),
    );
    forward_trace_events(job, cancellation, worker_cancellation, events, rx, worker).await
}

async fn forward_trace_events<F>(
    job: iptools_core::JobId,
    cancellation: CancellationToken,
    worker_cancellation: CancellationToken,
    events: mpsc::Sender<iptools_core::RuntimeEvent>,
    mut rx: mpsc::Receiver<TraceEvent>,
    worker: F,
) -> Result<(), String>
where
    F: Future<Output = ()>,
{
    use iptools_core::{RuntimeError, RuntimeErrorCode, RuntimeEvent, TraceHop};

    tokio::pin!(worker);
    let mut worker_done = false;
    let mut hops = 0u8;
    loop {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => {
                worker_cancellation.cancel();
                if !worker_done {
                    worker.await;
                }
                return Ok(());
            }
            event = rx.recv() => {
                let Some(event) = event else {
                    events.send(RuntimeEvent::TraceFailed {
                        job,
                        error: RuntimeError::new(
                            RuntimeErrorCode::Internal,
                            "trace worker ended without a terminal event",
                        ),
                    }).await.map_err(|error| error.to_string())?;
                    return Ok(());
                };
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
                        if !worker_done {
                            worker.await;
                        }
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
                        if !worker_done {
                            worker.await;
                        }
                        return Ok(());
                    }
                }
            }
            _ = &mut worker, if !worker_done => {
                // The worker can enqueue Hop + Done and complete in a single
                // poll (especially for a one-hop LAN target). Keep draining
                // the channel instead of dropping those buffered events.
                worker_done = true;
            },
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

#[cfg(test)]
mod bridge_tests {
    use super::*;
    use iptools_core::{JobId, RuntimeEvent, ToolKind};

    #[tokio::test]
    async fn worker_completion_does_not_drop_buffered_hop_and_done_events() {
        let job = JobId {
            tool: ToolKind::Trace,
            generation: 7,
        };
        let (inner_tx, inner_rx) = mpsc::channel(4);
        let worker = async move {
            inner_tx
                .send(TraceEvent::Hop(Hop {
                    ttl: 1,
                    addr: Some(Ipv4Addr::new(192, 168, 1, 1)),
                    rtt_ms: Some(2),
                    host: Some("router.local".into()),
                }))
                .await
                .unwrap();
            inner_tx.send(TraceEvent::Done).await.unwrap();
        };
        let (events_tx, mut events_rx) = mpsc::channel(4);
        forward_trace_events(
            job,
            CancellationToken::new(),
            CancellationToken::new(),
            events_tx,
            inner_rx,
            worker,
        )
        .await
        .unwrap();

        assert!(matches!(
            events_rx.recv().await,
            Some(RuntimeEvent::TraceHop { job: current, hop })
                if current == job && hop.ttl == 1
        ));
        assert!(matches!(
            events_rx.recv().await,
            Some(RuntimeEvent::TraceFinished { job: current, hops: 1 }) if current == job
        ));
    }
}
