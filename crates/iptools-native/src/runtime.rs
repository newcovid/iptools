//! Structured lifecycle management for native background jobs.

mod adapter_edit;
mod dashboard;
mod network_read;
mod port_scan;
mod scanner;

use std::{collections::HashMap, future::Future};

use iptools_core::{Effect, JobId, RuntimeEvent};
use sysinfo::Networks;
use tokio::{
    sync::{Semaphore, mpsc},
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;

const EVENT_CAPACITY: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPhase {
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeTaskError {
    #[error("{0}")]
    Operation(String),
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeDispatchError {
    #[error("native effect handler is not migrated yet: {0}")]
    UnsupportedEffect(&'static str),
}

#[derive(Debug)]
pub struct TaskResult {
    pub job: JobId,
    pub phase: TaskPhase,
    pub result: Result<(), RuntimeTaskError>,
}

/// Owns every task started through the modern native runtime.
pub struct NativeRuntime {
    tasks: JoinSet<TaskResult>,
    cancellations: HashMap<JobId, CancellationToken>,
    event_tx: mpsc::Sender<RuntimeEvent>,
    event_rx: mpsc::Receiver<RuntimeEvent>,
    dashboard_networks: Networks,
    dashboard_sample: Option<dashboard::TrafficSample>,
    network_sampler: network_read::NetworkSampler,
    adapter_gate: std::sync::Arc<Semaphore>,
}

impl Default for NativeRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeRuntime {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(EVENT_CAPACITY);
        Self {
            tasks: JoinSet::new(),
            cancellations: HashMap::new(),
            event_tx,
            event_rx,
            dashboard_networks: Networks::new_with_refreshed_list(),
            dashboard_sample: None,
            network_sampler: network_read::NetworkSampler::new(),
            adapter_gate: std::sync::Arc::new(Semaphore::new(1)),
        }
    }

    /// Execute a shared effect through the native runtime boundary.
    ///
    /// During vertical migration this deliberately accepts only effects whose
    /// native handlers have moved out of legacy page objects.
    pub fn dispatch(&mut self, effect: Effect) -> Result<(), RuntimeDispatchError> {
        match effect {
            Effect::RefreshDashboard { job, request } => {
                self.spawn_dashboard_refresh(job, request);
                Ok(())
            }
            Effect::RefreshAdapters { job } => {
                self.spawn_adapters_refresh(job);
                Ok(())
            }
            Effect::RefreshTraffic { job } => {
                self.spawn_traffic_refresh(job);
                Ok(())
            }
            Effect::ApplyAdapterConfig { job, request } => {
                self.spawn_adapter_config(job, request);
                Ok(())
            }
            Effect::StartScan { job, request } => {
                self.spawn_scan(job, request);
                Ok(())
            }
            Effect::CancelScan(job) => {
                self.cancel(job);
                Ok(())
            }
            Effect::StartPortScan { job, request } => {
                self.spawn_port_scan(job, request);
                Ok(())
            }
            Effect::StopPortScan(job) => {
                self.cancel(job);
                Ok(())
            }
            other => Err(RuntimeDispatchError::UnsupportedEffect(effect_name(&other))),
        }
    }

    /// Start one owned job, cancelling any older generation for the same tool.
    pub fn spawn<F, Fut>(&mut self, job: JobId, task: F)
    where
        F: FnOnce(CancellationToken, mpsc::Sender<RuntimeEvent>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), RuntimeTaskError>> + Send + 'static,
    {
        let previous: Vec<JobId> = self
            .cancellations
            .keys()
            .filter(|current| current.tool == job.tool)
            .copied()
            .collect();
        for current in previous {
            self.cancel(current);
        }

        let token = CancellationToken::new();
        self.cancellations.insert(job, token.clone());
        let tx = self.event_tx.clone();
        let task_token = token.clone();
        self.tasks.spawn(async move {
            let span = tracing::info_span!(
                "runtime_job",
                tool = ?job.tool,
                generation = job.generation
            );
            let _guard = span.enter();
            let result = task(token, tx).await;
            let phase = if result.is_err() {
                TaskPhase::Failed
            } else if task_token.is_cancelled() {
                TaskPhase::Cancelled
            } else {
                TaskPhase::Completed
            };
            TaskResult { job, phase, result }
        });
    }

    pub fn cancel(&mut self, job: JobId) {
        if let Some(token) = self.cancellations.remove(&job) {
            tracing::debug!(tool = ?job.tool, generation = job.generation, "cancelling runtime job");
            token.cancel();
        }
    }

    pub fn try_recv(&mut self) -> Option<RuntimeEvent> {
        self.event_rx.try_recv().ok()
    }

    pub fn reap_finished(&mut self) {
        while let Some(result) = self.tasks.try_join_next() {
            match result {
                Ok(task) => {
                    self.cancellations.remove(&task.job);
                    match task.result {
                        Ok(()) => tracing::debug!(
                            tool = ?task.job.tool,
                            generation = task.job.generation,
                            phase = ?task.phase,
                            "runtime job joined"
                        ),
                        Err(error) => tracing::warn!(
                            tool = ?task.job.tool,
                            generation = task.job.generation,
                            phase = ?task.phase,
                            %error,
                            "runtime job failed"
                        ),
                    }
                }
                Err(error) => tracing::warn!(%error, "runtime job failed to join"),
            }
        }
    }

    pub async fn shutdown(&mut self) {
        for token in self.cancellations.values() {
            token.cancel();
        }
        self.cancellations.clear();
        while let Some(result) = self.tasks.join_next().await {
            match result {
                Ok(task) => {
                    if let Err(error) = task.result {
                        tracing::warn!(tool = ?task.job.tool, generation = task.job.generation, %error, "runtime job failed during shutdown");
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "runtime job failed to join during shutdown");
                }
            }
        }
    }
}

fn effect_name(effect: &Effect) -> &'static str {
    match effect {
        Effect::PersistPreferences(_) => "persist-preferences",
        Effect::PersistAdapterEdit { .. } => "persist-adapter-edit",
        Effect::RefreshDashboard { .. } => "refresh-dashboard",
        Effect::RefreshAdapters { .. } => "refresh-adapters",
        Effect::RefreshTraffic { .. } => "refresh-traffic",
        Effect::ApplyAdapterConfig { .. } => "apply-adapter-config",
        Effect::StartScan { .. } => "start-scan",
        Effect::CancelScan(_) => "cancel-scan",
        Effect::StartPing { .. } => "start-ping",
        Effect::StopPing(_) => "stop-ping",
        Effect::StartTrace { .. } => "start-trace",
        Effect::StopTrace(_) => "stop-trace",
        Effect::StartPortScan { .. } => "start-port-scan",
        Effect::StopPortScan(_) => "stop-port-scan",
        Effect::StartPublicSpeed { .. } => "start-public-speed",
        Effect::StopPublicSpeed(_) => "stop-public-speed",
        Effect::StartLinkQuality { .. } => "start-link-quality",
        Effect::StopLinkQuality(_) => "stop-link-quality",
        Effect::StartLanSpeed { .. } => "start-lan-speed",
        Effect::StopLanSpeed(_) => "stop-lan-speed",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iptools_core::ToolKind;

    #[tokio::test]
    async fn replacement_cancels_previous_generation() {
        let mut supervisor = NativeRuntime::new();
        let first = JobId {
            tool: ToolKind::Ping,
            generation: 1,
        };
        let second = JobId {
            generation: 2,
            ..first
        };
        supervisor.spawn(first, |token, _| async move {
            token.cancelled().await;
            Ok(())
        });
        supervisor.spawn(second, |token, _| async move {
            token.cancelled().await;
            Ok(())
        });
        assert!(!supervisor.cancellations.contains_key(&first));
        assert!(supervisor.cancellations.contains_key(&second));
        supervisor.shutdown().await;
        assert!(supervisor.cancellations.is_empty());
    }

    #[tokio::test]
    async fn bounded_event_queue_backpressures_producers() {
        let mut runtime = NativeRuntime::new();
        let job = JobId {
            tool: ToolKind::Scanner,
            generation: 1,
        };
        runtime.spawn(job, move |_, events| async move {
            for current in 0..=EVENT_CAPACITY as u64 {
                events
                    .send(RuntimeEvent::ScanProgress {
                        job,
                        current,
                        total: EVENT_CAPACITY as u64 + 1,
                    })
                    .await
                    .map_err(|error| RuntimeTaskError::Operation(error.to_string()))?;
            }
            Ok(())
        });

        for _ in 0..32 {
            tokio::task::yield_now().await;
        }
        assert_eq!(runtime.event_rx.len(), EVENT_CAPACITY);
        assert_eq!(runtime.tasks.len(), 1, "producer should be backpressured");

        assert!(runtime.try_recv().is_some());
        for _ in 0..32 {
            tokio::task::yield_now().await;
            runtime.reap_finished();
            if runtime.tasks.is_empty() {
                break;
            }
        }
        assert!(runtime.tasks.is_empty());
        assert!(!runtime.cancellations.contains_key(&job));
    }
}
