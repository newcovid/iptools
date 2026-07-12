//! Native public download-speed algorithm for the structured runtime.

use std::time::{Duration, Instant};
use tokio::sync::mpsc;

const TEST_ENDPOINTS: &[(&str, &str)] = &[
    ("https://speedtest.zju.edu.cn/1000M", "speedtest.zju.edu.cn"),
    (
        "https://wirelesscdn-download.xuexi.cn/publish/xuexi_android/latest/xuexi_android_10002068.apk",
        "wirelesscdn-download.xuexi.cn",
    ),
    (
        "https://speed.cloudflare.com/__down?bytes=104857600",
        "speed.cloudflare.com",
    ),
];
const MAX_DURATION_MS: u64 = 15_000;
const CONNECT_TIMEOUT_SECS: u64 = 6;

/// Bridge the established endpoint selection and streaming algorithm into the
/// shared runtime protocol without exposing real requests to the Web runtime.
pub(crate) async fn run_shared(
    job: iptools_core::JobId,
    request: iptools_core::PublicSpeedRequest,
    cancellation: tokio_util::sync::CancellationToken,
    events: mpsc::Sender<iptools_core::RuntimeEvent>,
) -> Result<(), String> {
    run_shared_with_endpoints(job, request, cancellation, events, TEST_ENDPOINTS).await
}

async fn run_shared_with_endpoints(
    job: iptools_core::JobId,
    request: iptools_core::PublicSpeedRequest,
    cancellation: tokio_util::sync::CancellationToken,
    events: mpsc::Sender<iptools_core::RuntimeEvent>,
    endpoints: &[(&str, &str)],
) -> Result<(), String> {
    use iptools_core::{RuntimeError, RuntimeErrorCode, RuntimeEvent, SpeedSample, SpeedSummary};

    events
        .send(RuntimeEvent::PublicSpeedStarted { job, server: None })
        .await
        .map_err(|error| error.to_string())?;
    let client = match reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(CONNECT_TIMEOUT_SECS))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            events
                .send(RuntimeEvent::PublicSpeedFailed {
                    job,
                    error: RuntimeError::new(RuntimeErrorCode::Internal, error.to_string()),
                })
                .await
                .map_err(|error| error.to_string())?;
            return Ok(());
        }
    };

    let mut response = None;
    for (url, host) in endpoints {
        let attempt = tokio::select! {
            _ = cancellation.cancelled() => return Ok(()),
            response = client.get(*url).send() => response,
        };
        if let Ok(candidate) = attempt
            && candidate.status().is_success()
        {
            events
                .send(RuntimeEvent::PublicSpeedStarted {
                    job,
                    server: Some((*host).into()),
                })
                .await
                .map_err(|error| error.to_string())?;
            response = Some(candidate);
            break;
        }
    }
    let Some(mut response) = response else {
        events
            .send(RuntimeEvent::PublicSpeedFailed {
                job,
                error: RuntimeError::new(
                    RuntimeErrorCode::Network,
                    "all public speed endpoints were unavailable",
                ),
            })
            .await
            .map_err(|error| error.to_string())?;
        return Ok(());
    };

    let max_duration = Duration::from_millis(request.max_duration_ms.clamp(1_000, MAX_DURATION_MS));
    let start = Instant::now();
    let mut last = start;
    let mut last_bytes = 0u64;
    let mut total = 0u64;
    let mut peak = 0u64;
    loop {
        let chunk = tokio::select! {
            _ = cancellation.cancelled() => return Ok(()),
            chunk = response.chunk() => chunk,
        };
        match chunk {
            Ok(Some(chunk)) => {
                total = total.saturating_add(chunk.len() as u64);
                let now = Instant::now();
                let sample_duration = now.duration_since(last);
                if sample_duration >= Duration::from_millis(250) {
                    let bytes_per_second = ((total.saturating_sub(last_bytes)) as f64
                        / sample_duration.as_secs_f64())
                        as u64;
                    peak = peak.max(bytes_per_second);
                    events
                        .send(RuntimeEvent::PublicSpeedSample {
                            job,
                            sample: SpeedSample {
                                elapsed_ms: now.duration_since(start).as_millis() as u64,
                                bytes: total,
                                bytes_per_second,
                            },
                        })
                        .await
                        .map_err(|error| error.to_string())?;
                    last = now;
                    last_bytes = total;
                    if now.duration_since(start) >= max_duration {
                        break;
                    }
                }
            }
            Ok(None) => break,
            Err(error) => {
                events
                    .send(RuntimeEvent::PublicSpeedFailed {
                        job,
                        error: RuntimeError::new(RuntimeErrorCode::Network, error.to_string()),
                    })
                    .await
                    .map_err(|error| error.to_string())?;
                return Ok(());
            }
        }
    }
    let elapsed = start.elapsed();
    let average = if elapsed.is_zero() {
        0
    } else {
        (total as f64 / elapsed.as_secs_f64()) as u64
    };
    events
        .send(RuntimeEvent::PublicSpeedFinished {
            job,
            summary: SpeedSummary {
                average_bytes_per_second: average,
                peak_bytes_per_second: peak,
                total_bytes: total,
            },
        })
        .await
        .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod shared_tests {
    use super::*;
    use iptools_core::{JobId, PublicSpeedRequest, RuntimeEvent, ToolKind};
    use tokio::io::AsyncWriteExt;
    use tokio_util::sync::CancellationToken;

    #[tokio::test]
    async fn local_stream_drives_typed_samples_and_summary_without_external_network() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 262144\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            for _ in 0..4 {
                socket.write_all(&vec![7u8; 65_536]).await.unwrap();
                socket.flush().await.unwrap();
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });
        let endpoint = format!("http://{address}/speed.bin");
        let endpoints = [(endpoint.as_str(), "local.test")];
        let job = JobId {
            tool: ToolKind::PublicSpeed,
            generation: 1,
        };
        let (events, mut receiver) = mpsc::channel(32);
        run_shared_with_endpoints(
            job,
            PublicSpeedRequest {
                max_duration_ms: 1_000,
            },
            CancellationToken::new(),
            events,
            &endpoints,
        )
        .await
        .unwrap();
        server.await.unwrap();

        let mut received = Vec::new();
        while let Some(event) = receiver.recv().await {
            received.push(event);
        }
        assert!(received.iter().any(|event| matches!(
            event,
            RuntimeEvent::PublicSpeedStarted {
                job: current,
                server: Some(server)
            } if *current == job && server == "local.test"
        )));
        assert!(received.iter().any(|event| matches!(
            event,
            RuntimeEvent::PublicSpeedSample { job: current, sample }
                if *current == job && sample.bytes > 0 && sample.bytes_per_second > 0
        )));
        assert!(received.iter().any(|event| matches!(
            event,
            RuntimeEvent::PublicSpeedFinished { job: current, summary }
                if *current == job
                    && summary.total_bytes == 262_144
                    && summary.average_bytes_per_second > 0
        )));
    }

    #[tokio::test]
    async fn cancellation_interrupts_endpoint_connection_and_joins_cleanly() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let cancellation = CancellationToken::new();
        let server_cancellation = cancellation.clone();
        let server = tokio::spawn(async move {
            tokio::select! {
                _ = server_cancellation.cancelled() => {}
                connection = listener.accept() => {
                    if let Ok((mut socket, _)) = connection {
                        let _ = socket
                            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n")
                            .await;
                        server_cancellation.cancelled().await;
                    }
                }
            }
        });
        let endpoint = format!("http://{address}/slow.bin");
        let endpoints = [(endpoint.as_str(), "slow.test")];
        let job = JobId {
            tool: ToolKind::PublicSpeed,
            generation: 2,
        };
        let (events, mut receiver) = mpsc::channel(8);
        let controller = async {
            assert!(matches!(
                receiver.recv().await,
                Some(RuntimeEvent::PublicSpeedStarted {
                    job: current,
                    server: None
                }) if current == job
            ));
            tokio::task::yield_now().await;
            cancellation.cancel();
        };
        tokio::time::timeout(Duration::from_secs(1), async {
            let (result, ()) = tokio::join!(
                run_shared_with_endpoints(
                    job,
                    PublicSpeedRequest::default(),
                    cancellation.clone(),
                    events,
                    &endpoints,
                ),
                controller
            );
            result.unwrap();
            server.await.unwrap();
        })
        .await
        .expect("cancelled public speed task must join");
    }
}
