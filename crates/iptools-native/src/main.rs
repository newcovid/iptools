use anyhow::Result;
use clap::{Parser, ValueEnum};
use iptools_demo::ScenarioId;

mod config;
mod demo;
mod event;
mod frontend;
mod keymap;
mod modules;
mod native_app;
pub mod runtime;
mod utils;

/// 模块化、跨平台的网络工具箱。
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// 配置文件路径；默认使用当前目录下的 config.json。
    #[arg(short, long, value_name = "FILE")]
    config: Option<String>,

    /// 使用确定性模拟数据运行，不访问真实网络或系统配置。
    #[arg(long)]
    demo: bool,

    /// 选择内置演示场景；仅与 --demo 一起使用。
    #[arg(long, value_enum, requires = "demo")]
    scenario: Option<ScenarioArg>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ScenarioArg {
    HomeNetwork,
    WifiDegraded,
    MultiAdapter,
}

impl From<ScenarioArg> for ScenarioId {
    fn from(value: ScenarioArg) -> Self {
        match value {
            ScenarioArg::HomeNetwork => Self::HomeNetwork,
            ScenarioArg::WifiDegraded => Self::WifiDegraded,
            ScenarioArg::MultiAdapter => Self::MultiAdapter,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = Args::parse();
    if args.demo {
        return demo::run(
            args.scenario.unwrap_or(ScenarioArg::HomeNetwork).into(),
            args.config,
        )
        .await;
    }
    native_app::run(args.config).await?;

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

fn init_tracing() {
    if std::env::var_os("RUST_LOG").is_some() {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_writer(std::io::stderr)
            .try_init();
    }
}
