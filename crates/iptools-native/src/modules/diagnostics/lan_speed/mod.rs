//! Native LAN throughput runtime wrapper.

mod proto;

use tokio::{sync::mpsc, task::JoinSet};
use tokio_util::sync::CancellationToken;

use crate::utils::net;
use proto::{Direction, Flow, LanEvent, Proto, TestSpec, run_client, run_server};

/// Bridge the v0.3.1 LAN protocol into the shared runtime. All protocol
/// workers remain owned by this task and are joined before it returns.
pub(crate) async fn run_shared(
    job: iptools_core::JobId,
    request: iptools_core::LanSpeedRequest,
    cancellation: CancellationToken,
    events: mpsc::Sender<iptools_core::RuntimeEvent>,
) -> Result<(), String> {
    use iptools_core::{
        LanDirection, LanProtocol, LanSpeedMode, LanSpeedPhase, LanSpeedSample, LanSpeedSummary,
        RuntimeError, RuntimeErrorCode, RuntimeEvent,
    };

    if request.port == 0 || (request.mode == LanSpeedMode::Client && request.peer.trim().is_empty())
    {
        events
            .send(RuntimeEvent::LanSpeedFailed {
                job,
                error: RuntimeError::new(
                    RuntimeErrorCode::InvalidRequest,
                    "invalid LAN speed request",
                ),
            })
            .await
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    let endpoint = if request.mode == LanSpeedMode::Server {
        format!(
            "{}:{}",
            local_ipv4().unwrap_or_else(|| "0.0.0.0".into()),
            request.port
        )
    } else {
        format!("{}:{}", request.peer, request.port)
    };
    events
        .send(RuntimeEvent::LanSpeedStarted { job, endpoint })
        .await
        .map_err(|error| error.to_string())?;

    let spec = TestSpec {
        proto: match request.protocol {
            LanProtocol::Tcp => Proto::Tcp,
            LanProtocol::Udp => Proto::Udp,
        },
        direction: match request.direction {
            LanDirection::Upload => Direction::Up,
            LanDirection::Download => Direction::Down,
            LanDirection::Bidirectional => Direction::Bidir,
        },
        duration_ms: request.duration_secs.saturating_mul(1_000),
        streams: request.streams,
        rate_mbps: request.rate_mbps,
        payload_size: request.payload_size,
    };
    let (legacy_tx, mut legacy_rx) = mpsc::channel(64);
    let abort = CancellationToken::new();
    let mut runners = JoinSet::new();
    match request.mode {
        LanSpeedMode::Server => {
            let abort = abort.clone();
            runners.spawn(async move { run_server(request.port, legacy_tx, abort).await });
        }
        LanSpeedMode::Client => {
            let abort = abort.clone();
            runners.spawn(async move {
                run_client(request.peer, request.port, spec, legacy_tx, abort).await
            });
        }
    }

    let mut ticker = tokio::time::interval(std::time::Duration::from_millis(250));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut sample = LanSpeedSample {
        elapsed_ms: 0,
        tx_bps: 0,
        rx_bps: 0,
        tx_bytes: 0,
        rx_bytes: 0,
        loss_percent: None,
        jitter_ms: None,
    };
    let mut dirty = false;

    loop {
        tokio::select! {
            _ = cancellation.cancelled() => {
                abort.cancel();
                while let Some(result) = runners.join_next().await {
                    result.map_err(|error| error.to_string())?;
                }
                return Ok(());
            }
            _ = ticker.tick() => {
                if dirty {
                    events.send(RuntimeEvent::LanSpeedSample { job, sample: sample.clone() })
                        .await.map_err(|error| error.to_string())?;
                    dirty = false;
                }
            }
            event = legacy_rx.recv() => {
                match event {
                    Some(LanEvent::Status(key)) => {
                        let phase = match key.as_str() {
                            "diag_lan_status_listening" => Some(LanSpeedPhase::Listening),
                            "diag_lan_status_connecting" => Some(LanSpeedPhase::Connecting),
                            "diag_lan_status_connected" => Some(LanSpeedPhase::Connected),
                            _ => None,
                        };
                        if let Some(phase) = phase {
                            events.send(RuntimeEvent::LanSpeedStatus { job, phase })
                                .await.map_err(|error| error.to_string())?;
                        }
                    }
                    Some(LanEvent::Progress { flow, total_bytes, elapsed_ms, inst_bps }) => {
                        sample.elapsed_ms = elapsed_ms;
                        match flow {
                            Flow::Tx => {
                                sample.tx_bps = inst_bps;
                                sample.tx_bytes = total_bytes;
                            }
                            Flow::Rx => {
                                sample.rx_bps = inst_bps;
                                sample.rx_bytes = total_bytes;
                            }
                        }
                        dirty = true;
                    }
                    Some(LanEvent::Summary(summary)) => {
                        if dirty {
                            events.send(RuntimeEvent::LanSpeedSample { job, sample: sample.clone() })
                                .await.map_err(|error| error.to_string())?;
                        }
                        abort.cancel();
                        while let Some(result) = runners.join_next().await {
                            result.map_err(|error| error.to_string())?;
                        }
                        let udp = summary.udp.as_ref();
                        events.send(RuntimeEvent::LanSpeedFinished {
                            job,
                            summary: LanSpeedSummary {
                                tx_bytes: summary.tx_bytes,
                                rx_bytes: summary.rx_bytes,
                                elapsed_ms: summary.elapsed_ms,
                                loss_percent: udp.map(|value| value.loss_pct()),
                                jitter_ms: udp.map(|value| value.jitter_ms),
                                out_of_order: udp.map(|value| value.out_of_order),
                            },
                        }).await.map_err(|error| error.to_string())?;
                        return Ok(());
                    }
                    Some(LanEvent::Error(message)) => {
                        tracing::warn!(%message, "LAN speed session failed");
                        abort.cancel();
                        while let Some(result) = runners.join_next().await {
                            result.map_err(|error| error.to_string())?;
                        }
                        events.send(RuntimeEvent::LanSpeedFailed {
                            job,
                            error: RuntimeError::new(RuntimeErrorCode::Network, "LAN speed session failed"),
                        }).await.map_err(|error| error.to_string())?;
                        return Ok(());
                    }
                    None => {
                        while let Some(result) = runners.join_next().await {
                            result.map_err(|error| error.to_string())?;
                        }
                        events.send(RuntimeEvent::LanSpeedFailed {
                            job,
                            error: RuntimeError::new(RuntimeErrorCode::Internal, "LAN speed session ended without a summary"),
                        }).await.map_err(|error| error.to_string())?;
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// 取一个活跃物理接口的 IPv4，用于服务端显示监听地址。
fn local_ipv4() -> Option<String> {
    let interfaces = net::get_interfaces();
    interfaces
        .iter()
        .find(|i| i.is_up && i.is_physical && !i.ipv4.is_empty())
        .and_then(|i| i.ipv4.first().cloned())
}

#[cfg(test)]
mod shared_tests {
    use super::*;
    use iptools_core::{
        JobId, LanDirection, LanProtocol, LanSpeedMode, LanSpeedRequest, RuntimeEvent, ToolKind,
    };
    use tokio::time::{Duration, timeout};

    fn free_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    }

    #[tokio::test]
    async fn shared_tcp_client_and_server_emit_typed_samples_and_summaries() {
        let port = free_port();
        let server_job = JobId {
            tool: ToolKind::LanSpeed,
            generation: 1,
        };
        let client_job = JobId {
            tool: ToolKind::LanSpeed,
            generation: 2,
        };
        let (server_tx, mut server_rx) = mpsc::channel(512);
        let (client_tx, mut client_rx) = mpsc::channel(512);
        let server_cancel = CancellationToken::new();
        let client_cancel = CancellationToken::new();
        let server = tokio::spawn(run_shared(
            server_job,
            LanSpeedRequest {
                port,
                duration_secs: 1,
                payload_size: 4_096,
                ..LanSpeedRequest::default()
            },
            server_cancel,
            server_tx,
        ));
        tokio::time::sleep(Duration::from_millis(100)).await;
        let client = tokio::spawn(run_shared(
            client_job,
            LanSpeedRequest {
                mode: LanSpeedMode::Client,
                peer: "127.0.0.1".into(),
                port,
                protocol: LanProtocol::Tcp,
                direction: LanDirection::Upload,
                duration_secs: 1,
                streams: 1,
                payload_size: 4_096,
                rate_mbps: 0,
            },
            client_cancel,
            client_tx,
        ));

        timeout(Duration::from_secs(6), async {
            server.await.unwrap().unwrap();
            client.await.unwrap().unwrap();
        })
        .await
        .expect("local LAN speed pair should join");
        let mut server_events = Vec::new();
        while let Ok(event) = server_rx.try_recv() {
            server_events.push(event);
        }
        let mut client_events = Vec::new();
        while let Ok(event) = client_rx.try_recv() {
            client_events.push(event);
        }
        assert!(server_events.iter().any(|event| matches!(
            event,
            RuntimeEvent::LanSpeedSample { sample, .. } if sample.rx_bytes > 0
        )));
        assert!(client_events.iter().any(|event| matches!(
            event,
            RuntimeEvent::LanSpeedSample { sample, .. } if sample.tx_bytes > 0
        )));
        assert!(server_events.iter().any(|event| matches!(
            event,
            RuntimeEvent::LanSpeedFinished { summary, .. } if summary.rx_bytes > 0
        )));
        assert!(client_events.iter().any(|event| matches!(
            event,
            RuntimeEvent::LanSpeedFinished { summary, .. } if summary.tx_bytes > 0
        )));
        assert!(
            client_events
                .iter()
                .filter(|event| matches!(event, RuntimeEvent::LanSpeedSample { .. }))
                .count()
                <= 5
        );
    }

    #[tokio::test]
    async fn shared_server_cancellation_joins_while_waiting_for_a_client() {
        let job = JobId {
            tool: ToolKind::LanSpeed,
            generation: 7,
        };
        let (tx, mut rx) = mpsc::channel(16);
        let cancellation = CancellationToken::new();
        let task = tokio::spawn(run_shared(
            job,
            LanSpeedRequest {
                port: free_port(),
                ..LanSpeedRequest::default()
            },
            cancellation.clone(),
            tx,
        ));
        assert!(matches!(
            rx.recv().await,
            Some(RuntimeEvent::LanSpeedStarted { .. })
        ));
        cancellation.cancel();
        timeout(Duration::from_secs(2), task)
            .await
            .expect("cancelled listener should join")
            .unwrap()
            .unwrap();
    }
}
