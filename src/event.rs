use crossterm::event::{Event as CrosstermEvent, KeyEvent, KeyEventKind, MouseEvent};
use std::time::Duration;
use tokio::sync::mpsc;
use futures::StreamExt;

#[derive(Debug, Clone, Copy)]
pub enum Event {
    Tick,
    Key(KeyEvent),
    Mouse(MouseEvent),
    // !!! 修复点：添加 allow(dead_code) 压制未使用字段的警告 !!!
    #[allow(dead_code)]
    Resize(u16, u16),
}

pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<Event>,
    _task: tokio::task::JoinHandle<()>,
}

impl EventHandler {
    pub fn new(tick_rate_ms: u64) -> Self {
        let tick_rate = Duration::from_millis(tick_rate_ms);
        let (tx, rx) = mpsc::unbounded_channel();
        let _tx = tx.clone();

        let task = tokio::spawn(async move {
            let mut reader = crossterm::event::EventStream::new();
            let mut interval = tokio::time::interval(tick_rate);

            loop {
                let tick_delay = interval.tick();
                let crossterm_event = reader.next();

                tokio::select! {
                    _ = tick_delay => {
                        if tx.send(Event::Tick).is_err() { break; }
                    }
                    Some(Ok(evt)) = crossterm_event => {
                        match evt {
                            CrosstermEvent::Key(key) => {
                                if key.kind == KeyEventKind::Press {
                                    if tx.send(Event::Key(key)).is_err() { break; }
                                }
                            }
                            CrosstermEvent::Mouse(mouse) => {
                                if tx.send(Event::Mouse(mouse)).is_err() { break; }
                            }
                            CrosstermEvent::Resize(w, h) => {
                                if tx.send(Event::Resize(w, h)).is_err() { break; }
                            }
                            _ => {}
                        }
                    }
                }
            }
        });

        Self { rx, _task: task }
    }

    pub async fn next(&mut self) -> anyhow::Result<Event> {
        self.rx.recv().await.ok_or_else(|| anyhow::anyhow!("Event stream closed"))
    }
}