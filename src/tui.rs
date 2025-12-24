use crate::app::App;
use crate::event::EventHandler;
use crate::ui;
use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io;

/// TUI 状态管理器
pub struct Tui<B: Backend> {
    terminal: Terminal<B>,
    pub events: EventHandler,
}

impl<B: Backend> Tui<B> {
    pub fn new(terminal: Terminal<B>, events: EventHandler) -> Self {
        Self { terminal, events }
    }

    /// 初始化终端
    pub fn enter(&mut self) -> Result<()> {
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        self.terminal.hide_cursor()?;
        self.terminal.clear()?;
        Ok(())
    }

    /// 恢复终端
    pub fn exit(&mut self) -> Result<()> {
        if crossterm::terminal::is_raw_mode_enabled()? {
            self.terminal.show_cursor()?;
            execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
            disable_raw_mode()?;
        }
        Ok(())
    }

    /// 绘制 UI
    pub fn draw(&mut self, app: &mut App) -> Result<()> {
        self.terminal.draw(|frame| ui::draw(frame, app))?;
        Ok(())
    }
}
