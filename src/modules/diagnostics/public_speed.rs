//! 公网下载测速工具。
//!
//! 通过流式下载 Cloudflare 测速端点并测量吞吐量，最长 15 秒或下载满 100MB 即止。
//! 仅用 reqwest（已在依赖中），跨平台。计时与瞬时速率在异步任务内计算后回传，
//! UI 只负责展示与维护速率曲线。

use super::FocusArea;
use crate::keymap::Action;
use crate::ui::theme;
use crate::utils::format::format_bytes;
use crate::utils::i18n::I18n;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Paragraph, Sparkline},
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::mpsc;

const TEST_URL: &str = "https://speed.cloudflare.com/__down?bytes=104857600"; // 100 MB
const TEST_SERVER: &str = "speed.cloudflare.com";
const MAX_DURATION_MS: u64 = 15_000;

#[derive(Debug)]
enum SpeedEvent {
    Progress {
        total_bytes: u64,
        elapsed_ms: u64,
        inst_bps: u64,
    },
    Done,
    /// i18n 键
    Error(String),
}

pub struct PublicSpeedTool {
    running: bool,
    error_key: Option<String>,

    total_bytes: u64,
    elapsed_ms: u64,
    current_bps: u64,
    peak_bps: u64,
    history: VecDeque<u64>,

    tx: mpsc::Sender<SpeedEvent>,
    rx: mpsc::Receiver<SpeedEvent>,
    abort_flag: Arc<Mutex<bool>>,
}

impl PublicSpeedTool {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel(64);
        Self {
            running: false,
            error_key: None,
            total_bytes: 0,
            elapsed_ms: 0,
            current_bps: 0,
            peak_bps: 0,
            history: VecDeque::with_capacity(100),
            tx,
            rx,
            abort_flag: Arc::new(Mutex::new(false)),
        }
    }

    pub fn update(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            match event {
                SpeedEvent::Progress {
                    total_bytes,
                    elapsed_ms,
                    inst_bps,
                } => {
                    self.total_bytes = total_bytes;
                    self.elapsed_ms = elapsed_ms;
                    self.current_bps = inst_bps;
                    self.peak_bps = self.peak_bps.max(inst_bps);
                    if self.history.len() >= 100 {
                        self.history.pop_front();
                    }
                    self.history.push_back(inst_bps);
                }
                SpeedEvent::Done => {
                    self.running = false;
                    self.current_bps = 0;
                }
                SpeedEvent::Error(key) => {
                    self.error_key = Some(key);
                    self.running = false;
                    self.current_bps = 0;
                }
            }
        }
    }

    pub fn on_key(&mut self, action: Option<Action>) {
        if action == Some(Action::Toggle) {
            if self.running {
                self.stop();
            } else {
                self.start();
            }
        }
    }

    /// 平均速率（字节/秒）。
    fn avg_bps(&self) -> u64 {
        if self.elapsed_ms == 0 {
            0
        } else {
            (self.total_bytes as f64 / (self.elapsed_ms as f64 / 1000.0)) as u64
        }
    }

    fn start(&mut self) {
        self.running = true;
        self.error_key = None;
        self.total_bytes = 0;
        self.elapsed_ms = 0;
        self.current_bps = 0;
        self.peak_bps = 0;
        self.history.clear();
        *self.abort_flag.lock().unwrap() = false;

        let tx = self.tx.clone();
        let abort = self.abort_flag.clone();

        tokio::spawn(async move {
            let client = match reqwest::Client::builder().no_proxy().build() {
                Ok(c) => c,
                Err(_) => {
                    let _ = tx.send(SpeedEvent::Error("diag_speed_err".into())).await;
                    return;
                }
            };

            let mut resp = match client.get(TEST_URL).send().await {
                Ok(r) => r,
                Err(_) => {
                    let _ = tx.send(SpeedEvent::Error("diag_speed_err".into())).await;
                    return;
                }
            };

            let start = Instant::now();
            let mut last = start;
            let mut last_bytes: u64 = 0;
            let mut total: u64 = 0;

            loop {
                if *abort.lock().unwrap() {
                    break;
                }
                match resp.chunk().await {
                    Ok(Some(chunk)) => {
                        total += chunk.len() as u64;
                        let now = Instant::now();
                        let since = now.duration_since(last).as_secs_f64();
                        if since >= 0.25 {
                            let inst = ((total - last_bytes) as f64 / since) as u64;
                            let elapsed = now.duration_since(start).as_millis() as u64;
                            let _ = tx
                                .send(SpeedEvent::Progress {
                                    total_bytes: total,
                                    elapsed_ms: elapsed,
                                    inst_bps: inst,
                                })
                                .await;
                            last = now;
                            last_bytes = total;
                            if elapsed >= MAX_DURATION_MS {
                                break;
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(_) => {
                        let _ = tx.send(SpeedEvent::Error("diag_speed_err".into())).await;
                        return;
                    }
                }
            }

            let _ = tx.send(SpeedEvent::Done).await;
        });
    }

    fn stop(&mut self) {
        self.running = false;
        *self.abort_flag.lock().unwrap() = true;
    }

    // -------------------------------------------------------------------------
    // 绘图
    // -------------------------------------------------------------------------

    pub fn draw(
        &mut self,
        f: &mut Frame,
        main_area: Rect,
        config_area: Rect,
        i18n: &I18n,
        is_focused: bool,
        active_focus: FocusArea,
    ) {
        self.draw_main(f, main_area, i18n, is_focused, active_focus);
        self.draw_config(f, config_area, i18n, is_focused, active_focus);
    }

    fn draw_main(
        &self,
        f: &mut Frame,
        area: Rect,
        i18n: &I18n,
        is_focused: bool,
        active_focus: FocusArea,
    ) {
        let color = if is_focused && active_focus == FocusArea::Main {
            Color::Yellow
        } else {
            Color::Gray
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(i18n.t("diag_main_title"))
            .border_style(Style::default().fg(color));
        let inner = block.inner(area);
        f.render_widget(block, area);

        if !is_focused {
            let p = Paragraph::new(i18n.t("diag_msg_focus_hint"))
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(p, inner);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // current speed (big)
                Constraint::Length(2), // avg / peak
                Constraint::Length(2), // downloaded / elapsed
                Constraint::Min(3),    // sparkline
                Constraint::Length(1), // status
            ])
            .split(inner);

        // 1. 当前速率（突出显示，含 Mbps）
        let current = Line::from(vec![
            Span::styled(
                format!("{}  ", i18n.t("diag_speed_current")),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                speed_dual(self.current_bps),
                Style::default()
                    .fg(theme::COLOR_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        f.render_widget(Paragraph::new(current), chunks[0]);

        // 2. 平均 / 峰值
        let avg_peak = Line::from(vec![
            Span::styled(
                format!("{}: ", i18n.t("diag_speed_avg")),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!("{:<22}", speed_dual(self.avg_bps())),
                Style::default().fg(theme::COLOR_SECONDARY),
            ),
            Span::styled(
                format!("{}: ", i18n.t("diag_speed_peak")),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(speed_dual(self.peak_bps), Style::default().fg(Color::Yellow)),
        ]);
        f.render_widget(Paragraph::new(avg_peak), chunks[1]);

        // 3. 已下载 / 用时
        let dl_elapsed = Line::from(vec![
            Span::styled(
                format!("{}: ", i18n.t("diag_speed_downloaded")),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!("{:<18}", format_bytes(self.total_bytes)),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!("{}: ", i18n.t("diag_speed_elapsed")),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!("{:.1}s", self.elapsed_ms as f64 / 1000.0),
                Style::default().fg(Color::White),
            ),
        ]);
        f.render_widget(Paragraph::new(dl_elapsed), chunks[2]);

        // 4. 速率曲线
        let data: Vec<u64> = self.history.iter().cloned().collect();
        let sparkline = Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .title(i18n.t("diag_speed_history")),
            )
            .data(&data)
            .style(Style::default().fg(theme::COLOR_PRIMARY));
        f.render_widget(sparkline, chunks[3]);

        // 5. 状态行
        let (text, style) = if let Some(key) = &self.error_key {
            (i18n.t(key), Style::default().fg(theme::COLOR_ERROR))
        } else if self.running {
            (
                format!("{} | {}", i18n.t("diag_status_running"), i18n.t("diag_msg_stop")),
                Style::default().fg(Color::Green),
            )
        } else {
            (
                format!("{} | {}", i18n.t("diag_status_stopped"), i18n.t("diag_msg_start")),
                Style::default().fg(Color::Red),
            )
        };
        f.render_widget(Paragraph::new(text).style(style), chunks[4]);
    }

    fn draw_config(
        &self,
        f: &mut Frame,
        area: Rect,
        i18n: &I18n,
        is_focused: bool,
        active_focus: FocusArea,
    ) {
        let color = if is_focused && active_focus == FocusArea::Config {
            Color::Yellow
        } else {
            Color::Gray
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(i18n.t("diag_config_title"))
            .border_style(Style::default().fg(color));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let lines = vec![
            Line::from(vec![
                Span::styled(
                    format!("{}: ", i18n.t("diag_speed_server")),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(TEST_SERVER, Style::default().fg(theme::COLOR_SECONDARY)),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                i18n.t("diag_speed_note"),
                Style::default().fg(Color::DarkGray),
            )),
        ];
        f.render_widget(Paragraph::new(lines), inner);
    }
}

/// 同时给出 MB/s 与 Mbps 两种习惯单位，例如 "11.84 MB/s (94.7 Mbps)"。
fn speed_dual(bps: u64) -> String {
    let mb_s = bps as f64 / 1024.0 / 1024.0;
    let mbps = bps as f64 * 8.0 / 1_000_000.0;
    format!("{:.2} MB/s ({:.1} Mbps)", mb_s, mbps)
}
