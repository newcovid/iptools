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

/// IP Tools CLI - 模块化、跨平台的网络工具箱
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// 配置文件路径 (可选)
    #[arg(short, long, value_name = "FILE")]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // 解析命令行参数
    let args = Args::parse();

    // 1. 初始化应用状态
    // App 结构体负责持有所有业务模块和全局配置
    // 透传 --config 指定的配置文件路径（缺省为 ./config.json）
    let mut app = App::new(args.config);

    // 2. 初始化 TUI 终端环境
    // 使用 Crossterm 作为后端，并开启备用屏幕
    let backend = CrosstermBackend::new(io::stdout());
    let terminal = Terminal::new(backend)?;
    let event_handler = EventHandler::new(250); // 设置 250ms 的 Tick 间隔用于动画/数据刷新
    let mut tui = Tui::new(terminal, event_handler);

    tui.enter()?;

    // 3. 主事件循环
    while app.running {
        // 绘制当前 UI 帧
        tui.draw(&mut app)?;

        // 异步等待并处理事件
        match tui.events.next().await? {
            Event::Tick => app.on_tick(),
            Event::Key(key) => app.on_key(key),
            Event::Mouse(mouse) => app.on_mouse(mouse),
            Event::Resize(w, h) => app.on_resize(w, h),
        }
    }

    // 4. 程序退出时恢复终端状态
    tui.exit()?;

    // 5. Linux：退出后（终端已恢复，提示可见）若缺 CAP_NET_RAW 则友好提醒——
    //    这正是「扫描扫不到设备 / Ping 报权限」的根因。已授权则不打扰。
    #[cfg(target_os = "linux")]
    {
        if !crate::utils::net::has_cap_net_raw() {
            eprintln!();
            eprintln!("提示：未检测到 CAP_NET_RAW —— 局域网扫描 / Ping / Trace / 链路质量需要它。");
            eprintln!("      一次性授权：sudo setcap cap_net_raw+ep <本程序路径>   （或用 sudo 运行）");
            eprintln!("      发行包内可直接：sudo ./install.sh");
        }
    }
    Ok(())
}