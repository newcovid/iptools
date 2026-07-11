//! Structured lifecycle management for native background jobs.

use std::{collections::HashMap, future::Future};

use iptools_core::{JobId, RuntimeEvent};
use tokio::{sync::mpsc, task::JoinSet};
use tokio_util::sync::CancellationToken;

const EVENT_CAPACITY: usize = 512;

/// Owns every task started through the modern native runtime.
pub struct RuntimeSupervisor {
    tasks: JoinSet<JobId>,
    cancellations: HashMap<JobId, CancellationToken>,
    event_tx: mpsc::Sender<RuntimeEvent>,
    event_rx: mpsc::Receiver<RuntimeEvent>,
}

impl Default for RuntimeSupervisor {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeSupervisor {
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(EVENT_CAPACITY);
        Self {
            tasks: JoinSet::new(),
            cancellations: HashMap::new(),
            event_tx,
            event_rx,
        }
    }

    /// Start one owned job, cancelling any older generation for the same tool.
    pub fn spawn<F, Fut>(&mut self, job: JobId, task: F)
    where
        F: FnOnce(CancellationToken, mpsc::Sender<RuntimeEvent>) -> Fut + Send + 'static,
        Fut: Future<Output = ()> + Send + 'static,
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
        self.tasks.spawn(async move {
            let span = tracing::info_span!(
                "runtime_job",
                tool = ?job.tool,
                generation = job.generation
            );
            let _guard = span.enter();
            task(token, tx).await;
            job
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
                Ok(job) => {
                    self.cancellations.remove(&job);
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
            if let Err(error) = result {
                tracing::warn!(%error, "runtime job failed during shutdown");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iptools_core::ToolKind;

    #[tokio::test]
    async fn replacement_cancels_previous_generation() {
        let mut supervisor = RuntimeSupervisor::new();
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
        });
        supervisor.spawn(second, |token, _| async move {
            token.cancelled().await;
        });
        assert!(!supervisor.cancellations.contains_key(&first));
        assert!(supervisor.cancellations.contains_key(&second));
        supervisor.shutdown().await;
        assert!(supervisor.cancellations.is_empty());
    }
}
