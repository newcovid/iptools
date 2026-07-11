use crossterm::event::{Event as CrosstermEvent, KeyEvent, KeyEventKind, MouseEvent};
use futures::StreamExt;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

const EVENT_CAPACITY: usize = 128;

#[derive(Debug, Clone, Copy)]
pub enum Event {
    Tick,
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize,
}

pub struct EventHandler {
    rx: mpsc::Receiver<Event>,
    shutdown: CancellationToken,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl EventHandler {
    pub fn new(tick_rate_ms: u64) -> Self {
        let tick_rate = Duration::from_millis(tick_rate_ms);
        let (tx, rx) = mpsc::channel(EVENT_CAPACITY);
        let shutdown = CancellationToken::new();
        let task_shutdown = shutdown.clone();

        let task = tokio::spawn(async move {
            let mut reader = crossterm::event::EventStream::new();
            let mut interval = tokio::time::interval(tick_rate);

            loop {
                let tick_delay = interval.tick();
                let crossterm_event = reader.next();

                tokio::select! {
                    _ = task_shutdown.cancelled() => break,
                    _ = tick_delay => {
                        if tx.send(Event::Tick).await.is_err() { break; }
                    }
                    Some(Ok(evt)) = crossterm_event => {
                        match evt {
                            CrosstermEvent::Key(key) => {
                                if key.kind == KeyEventKind::Press
                                    && tx.send(Event::Key(key)).await.is_err()
                                {
                                    break;
                                }
                            }
                            CrosstermEvent::Mouse(mouse) => {
                                if tx.send(Event::Mouse(mouse)).await.is_err() { break; }
                            }
                            CrosstermEvent::Resize(_, _)
                                if tx.send(Event::Resize).await.is_err() =>
                            {
                                break;
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        Self {
            rx,
            shutdown,
            task: Some(task),
        }
    }

    pub async fn next(&mut self) -> anyhow::Result<Event> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("Event stream closed"))
    }

    pub async fn shutdown(&mut self) {
        self.shutdown.cancel();
        if let Some(task) = self.task.take()
            && let Err(error) = task.await
        {
            tracing::warn!(%error, "terminal event task failed during shutdown");
        }
    }
}

impl Drop for EventHandler {
    fn drop(&mut self) {
        self.shutdown.cancel();
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
