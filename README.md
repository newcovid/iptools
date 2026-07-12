# iptools

跨平台的 IP 管理与网络诊断终端工具。基于 Rust 和 Ratatui，面向 Windows 与 Linux，提供统一的键盘和鼠标交互。

[![CI](https://github.com/newcovid/iptools/actions/workflows/ci.yml/badge.svg)](https://github.com/newcovid/iptools/actions/workflows/ci.yml)
[![Latest Release](https://img.shields.io/github/v/release/newcovid/iptools)](https://github.com/newcovid/iptools/releases/latest)
[![Platforms](https://img.shields.io/badge/platform-Windows%20%7C%20Linux-2563eb)](#平台支持)
[![License](https://img.shields.io/github/license/newcovid/iptools)](LICENSE)

`iptools` 将网卡信息、IP 配置、局域网发现、流量监控和常用网络诊断集中在一个 TUI 中。发布包是单文件程序，语言资源已内嵌，无需安装运行时。

## 在线交互体验

[在线体验 iptools v0.4 WebAssembly Preview](https://newcovid.github.io/iptools/) · [下载最新原生版本](https://github.com/newcovid/iptools/releases/latest)

> Web 版只运行确定性的模拟场景，不读取本机网络信息，也不执行真实扫描或系统配置修改。首次在线加载完成后可离线使用。

Web 展览与原生 `iptools --demo` 使用相同的状态机和确定性场景，可以实际切换页面、编辑、启动和停止任务。它始终显示 **SIMULATED DATA / 模拟数据**，不会扫描浏览器所在设备或局域网；首次在线加载后可离线使用。

![iptools WebAssembly 交互展览，中文使用 Maple Mono CN 开源等宽字体](docs/assets/web-demo.png)

## 主要能力

- 六个功能页：概览、适配器、局域网扫描、流量、诊断和设置。
- 完整键盘操作，并支持标签页、列表、输入框和滚轮的鼠标交互。
- 中文与英文界面；首次启动按系统区域选择，也可随时切换。
- 快捷键、扫描并发数和公网 IP 数据源可配置。
- 自动保存诊断参数、界面位置和最近使用的目标；设置页可一键清除。
- Windows 优先使用原生 API；Linux 的读取与探测使用 sysfs、getifaddrs 和原始套接字，网络配置按环境调用 `nmcli`、`netplan` 或 `ip`。

## 功能

| 模块 | 功能 |
|---|---|
| 概览 | 活动网卡、SSID、本地地址、DHCP 状态、实时速率、累计流量，以及通过 HTTPS 回退链获取的公网 IP、位置和运营商信息 |
| 适配器 | 查看物理/虚拟网卡、IPv4/IPv6、MAC 和链路信息；配置静态 IPv4、掩码、网关、DNS 或 DHCP，写入前校验并二次确认 |
| 扫描 | 按 CIDR 执行二层 ARP 扫描，显示 IP、MAC 和主机名；主机名依次尝试反向 DNS、NetBIOS 和 mDNS |
| 流量 | 查看实时收发速率、当前会话累计和系统启动后累计流量 |
| 诊断 | Ping、路由跟踪、端口扫描、公网测速、TCP/UDP 内网吞吐测试，以及有线/无线链路质量评分 |
| 设置 | 调整扫描并发数、界面语言并清除保存的会话参数 |

链路质量工具支持指定源网卡，持续采样延迟、抖动和丢包；无线网络还会采集 RSSI、信号质量、频段、信道、协商速率和安全类型。评分和排查方法见[链路质量评测指南](docs/link-quality-guide.md)。

## 安装

### 下载发行版

从 [Releases](https://github.com/newcovid/iptools/releases/latest) 下载对应平台的压缩包并解压：

- Windows：运行 `iptools-*-windows-x86_64.exe`。
- Linux：解压 `iptools-*-linux-x86_64.tar.gz`，进入目录后运行 `./iptools`。

涉及原始套接字或系统网络配置的功能需要额外权限：

- Windows：需要写入 IP 配置时，请以管理员身份运行。
- Linux：ARP 扫描、Ping、路由跟踪和链路质量需要 `CAP_NET_RAW`。发行包内可执行 `sudo ./install.sh` 为同目录二进制授予最小能力；使用 `sudo ./install.sh --system` 可同时安装到 `/usr/local/bin`。
- Linux 无线详情依赖 `iw`；IP 配置写入需要 PolicyKit 或 `sudo` 权限。

### 从源码构建

需要 Rust 1.97.0；仓库中的 `rust-toolchain.toml` 会为 rustup 自动选择该版本并安装 WASM、rustfmt 与 Clippy 组件。HTTP 客户端使用 rustls，不再要求 OpenSSL 开发包。

```bash
git clone https://github.com/newcovid/iptools.git
cd iptools
cargo build --release
```

可执行文件位于 `target/release/iptools`，Windows 下为 `iptools.exe`。

## 使用

```text
iptools
iptools --config /path/to/config.json
iptools --demo
iptools --demo --scenario wifi-degraded
```

默认配置文件是当前目录下的 `config.json`。程序会在需要时创建并更新它；完整的可编辑字段参见 [`config.example.json`](config.example.json)。其中 `session` 由程序维护，用于保存输入参数、最近使用历史和界面位置，通常无需手工修改。

### 默认快捷键

| 操作 | 按键 |
|---|---|
| 下一个 / 上一个标签页 | `Tab` / `Shift+Tab` |
| 导航 | 方向键或 `h` `j` `k` `l` |
| 确认 / 返回 | `Enter` / `Esc` |
| 编辑 | `e` |
| 开始或停止 | `Space` |
| 刷新 | `r` |
| 最近使用历史 | `Ctrl+R` |
| 切换语言 | `Ctrl+L` |
| 帮助 | `F1` |
| 退出 | `Ctrl+C` / `Ctrl+Q` |

底部帮助栏始终显示当前上下文和实际绑定。所有动作均可在配置文件的 `keybindings` 中重绑。

## 平台支持

| 功能 | Windows | Linux | 其它 Unix |
|---|:---:|:---:|:---:|
| 端口扫描、公网测速、内网测速 | 支持 | 支持 | 支持 |
| 网卡枚举、ARP 扫描 | 支持 | 支持，需要 `CAP_NET_RAW` | 暂不支持 |
| 路由跟踪、链路质量 | 支持 | 支持，需要 `CAP_NET_RAW` | 后端能力有限 |
| 无线详情 | WLAN API | `iw` | 暂不支持 |
| IP 配置写入 | WMI | `nmcli` / `netplan` / `ip` | 暂不支持 |
| Ping | 支持 | 支持，需要 `CAP_NET_RAW` | 通常需要 root |

局域网扫描基于 ARP，只能可靠发现同一二层网络中的在线设备。网络配置写入会短暂中断连接，请确认目标网卡和参数后再执行。

## 项目文档

- [架构与开发约定](docs/architecture.md)
- [链路质量评测指南](docs/link-quality-guide.md)
- [贡献指南](CONTRIBUTING.md)
- [安全策略](SECURITY.md)

## 开发与验证

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
cargo check -p iptools-web --target wasm32-unknown-unknown
```

Web 展览使用 Trunk 0.21.14 构建：

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk --version 0.21.14 --locked
cd crates/iptools-web
trunk serve
```

浏览器端以 Ratzilla 0.3.1 为基础。项目携带三个小型可审计补丁：Canvas 按 Unicode 双宽裁剪，DOM 在 resize 交接帧跳过越界单元，并在全宽字符被替换时恢复隐藏续格；中文字体是 OFL-1.1 的 Maple Mono CN 约 334 KiB 可重复字形子集，许可证、固定源文件哈希和生成脚本说明位于 `crates/iptools-web/assets/fonts/`。

UI 和真实网络写入仍需要在目标系统上手动验证。新增界面文案时必须同时更新 `assets/locales/en-US.json` 和 `assets/locales/zh-CN.json`，并重新生成 Web 字体子集；测试会检查语言包和浏览器字体加载。

## 许可证

本项目采用 [MIT License](LICENSE)，版权归 newcovid 所有。
