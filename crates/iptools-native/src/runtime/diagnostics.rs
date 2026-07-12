use iptools_core::{
    JobId, LanSpeedRequest, LinkQualityRequest, PingRequest, PublicSpeedRequest, TraceRequest,
};

use super::{NativeRuntime, RuntimeTaskError};

impl NativeRuntime {
    pub(super) fn spawn_ping(&mut self, job: JobId, request: PingRequest) {
        self.spawn(job, move |cancellation, events| async move {
            crate::modules::diagnostics::ping::run_shared(job, request, cancellation, events)
                .await
                .map_err(RuntimeTaskError::Operation)
        });
    }

    pub(super) fn spawn_trace(&mut self, job: JobId, request: TraceRequest) {
        self.spawn(job, move |cancellation, events| async move {
            crate::modules::diagnostics::trace::run_shared(job, request, cancellation, events)
                .await
                .map_err(RuntimeTaskError::Operation)
        });
    }

    pub(super) fn spawn_public_speed(&mut self, job: JobId, request: PublicSpeedRequest) {
        self.spawn(job, move |cancellation, events| async move {
            crate::modules::diagnostics::public_speed::run_shared(
                job,
                request,
                cancellation,
                events,
            )
            .await
            .map_err(RuntimeTaskError::Operation)
        });
    }

    pub(super) fn spawn_link_quality(&mut self, job: JobId, request: LinkQualityRequest) {
        self.spawn(job, move |cancellation, events| async move {
            crate::modules::diagnostics::link_quality::run_shared(
                job,
                request,
                cancellation,
                events,
            )
            .await
            .map_err(RuntimeTaskError::Operation)
        });
    }

    pub(super) fn spawn_lan_speed(&mut self, job: JobId, request: LanSpeedRequest) {
        self.spawn(job, move |cancellation, events| async move {
            crate::modules::diagnostics::lan_speed::run_shared(job, request, cancellation, events)
                .await
                .map_err(RuntimeTaskError::Operation)
        });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use iptools_core::{
        Effect, LanSpeedMode, LinkQualityRequest, RuntimeErrorCode, RuntimeEvent, ToolKind,
    };

    use super::*;

    #[tokio::test]
    async fn invalid_ping_and_trace_requests_fail_without_network_io() {
        let mut runtime = NativeRuntime::new();
        let ping = JobId {
            tool: ToolKind::Ping,
            generation: 1,
        };
        runtime
            .dispatch(Effect::StartPing {
                job: ping,
                request: PingRequest {
                    target: String::new(),
                    ..PingRequest::default()
                },
            })
            .unwrap();
        let trace = JobId {
            tool: ToolKind::Trace,
            generation: 2,
        };
        runtime
            .dispatch(Effect::StartTrace {
                job: trace,
                request: TraceRequest {
                    target: String::new(),
                    ..TraceRequest::default()
                },
            })
            .unwrap();
        let link = JobId {
            tool: ToolKind::LinkQuality,
            generation: 3,
        };
        runtime
            .dispatch(Effect::StartLinkQuality {
                job: link,
                request: LinkQualityRequest::default(),
            })
            .unwrap();
        let lan = JobId {
            tool: ToolKind::LanSpeed,
            generation: 4,
        };
        runtime
            .dispatch(Effect::StartLanSpeed {
                job: lan,
                request: LanSpeedRequest {
                    mode: LanSpeedMode::Client,
                    peer: String::new(),
                    ..LanSpeedRequest::default()
                },
            })
            .unwrap();

        let mut events = Vec::new();
        for _ in 0..20 {
            while let Some(event) = runtime.try_recv() {
                events.push(event);
            }
            if events.len() >= 4 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        assert!(events.iter().any(|event| matches!(event, RuntimeEvent::PingFailed { job, error } if *job == ping && error.code == RuntimeErrorCode::InvalidRequest)));
        assert!(events.iter().any(|event| matches!(event, RuntimeEvent::TraceFailed { job, error } if *job == trace && error.code == RuntimeErrorCode::InvalidRequest)));
        assert!(events.iter().any(|event| matches!(event, RuntimeEvent::LinkQualityFailed { job, error } if *job == link && error.code == RuntimeErrorCode::InvalidRequest)));
        assert!(events.iter().any(|event| matches!(event, RuntimeEvent::LanSpeedFailed { job, error } if *job == lan && error.code == RuntimeErrorCode::InvalidRequest)));
        runtime.shutdown().await;
    }
}
