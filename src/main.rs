use anyhow::Result;
use clap::Parser;
use std::io;

mod app;
mod config;
mod event;
mod history;
mod keymap;
mod modules;
mod session;
mod tui;
mod ui;
mod utils;

use crate::app::App;
use crate::event::{Event, EventHandler};
use crate::tui::Tui;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

/// 模块化、跨平台的网络工具箱。
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// 配置文件路径；默认使用当前目录下的 config.json。
    #[arg(short, long, value_name = "FILE")]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let mut app = App::new(args.config);

    let backend = CrosstermBackend::new(io::stdout());
    let terminal = Terminal::new(backend)?;
    let event_handler = EventHandler::new(250);
    let mut tui = Tui::new(terminal, event_handler);

    tui.enter()?;

    while app.running {
        tui.draw(&mut app)?;

        match tui.events.next().await? {
            Event::Tick => app.on_tick(),
            Event::Key(key) => app.on_key(key),
            Event::Mouse(mouse) => app.on_mouse(mouse),
            Event::Resize => {}
        }
    }

    tui.exit()?;

    // 终端恢复后再显示权限提示，避免信息被备用屏幕吞掉。
    #[cfg(target_os = "linux")]
    {
        if !crate::utils::net::has_cap_net_raw() {
            eprintln!();
            eprintln!("提示：未检测到 CAP_NET_RAW —— 局域网扫描 / Ping / Trace / 链路质量需要它。");
            eprintln!(
                "      一次性授权：sudo setcap cap_net_raw+ep <本程序路径>   （或用 sudo 运行）"
            );
            eprintln!("      发行包内可直接：sudo ./install.sh");
        }
    }
    Ok(())
}
