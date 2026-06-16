# iptools

> 一站式 IP 管理 / 网络测试 **终端 TUI 工具** — 纯 Rust、模块化、面向新手、美观且键鼠皆可操作。
> *A one-stop, beginner-friendly IP management & network-testing TUI, written in pure Rust.*

[![CI](https://github.com/newcovid/iptools/actions/workflows/ci.yml/badge.svg)](https://github.com/newcovid/iptools/actions/workflows/ci.yml)
[![Release](https://github.com/newcovid/iptools/actions/workflows/release.yml/badge.svg)](https://github.com/newcovid/iptools/actions/workflows/release.yml)
[![Latest Release](https://img.shields.io/github/v/release/newcovid/iptools)](https://github.com/newcovid/iptools/releases/latest)
![Platform](https://img.shields.io/badge/platform-Windows-blue)
![Rust](https://img.shields.io/badge/rust-2021-orange)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)

把日常要装一堆小工具、敲一堆命令才能搞定的事——看本机/公网信息、配 IP、扫局域网、
看流量、ping/路由跟踪/端口扫描/测速/链路质量评测——收进**一个零外部依赖的单文件二进制**里，
用方向键和鼠标就能玩转。

---

## ✨ 特性一览

- 🧭 **六大模块，一个界面**：概览 · 适配器 · 扫描 · 流量 · 诊断 · 设置，`Tab` 切换。
- 🖱️ **键鼠双操作**：方向键/`hjkl`/数字键导航，鼠标点击切页/选项、滚轮滚动，文本框点击定位光标。
- 🌐 **零外部依赖**：不调用 `netsh`/`arp`/`ping.exe`，全部走原生 Windows API（windows-rs）。
- 🌏 **多语言 i18n**：中文 / 英文，运行时 `Ctrl+L` 切换，首启按系统区域自动选择。
- 📦 **单文件分发**：语言包经 `include_str!` 编译期内嵌，发布二进制开箱即用。
- ⚙️ **可定制快捷键**：`config.json` 里随意重绑任意动作。
- 💾 **参数记忆**：各页面输入过的目标 IP / 端口 / 发包间隔 / 超时 / 载荷大小等**全部自动保存**，重启即恢复，告别每次重填。扫描页 CIDR 每次启动按当前网卡自动推断默认值。链路质量更是**按网卡分别记忆**——无线、有线各记各的目标，切网卡自动跟随；还会记住**上次所在页面与诊断子工具**，设置页一键「清空参数记忆」。
- 🕘 **目标历史（MRU）**：目标 IP / 主机、扫描 CIDR **自动记最近用过的多条**，编辑时行尾灰字补全（`→` 采纳）、`Ctrl+R` 下拉回选；IP/主机历史跨工具共享、CIDR 独立。

## 🧩 功能模块

| 模块 | 能做什么 |
|---|---|
| **概览 Dashboard** | 本机信息（时间、活动网卡、SSID、IP 及静态/DHCP、物理/虚拟、实时速率、开机流量）＋ 公网信息（公网 IP、归属地、运营商）聚合一屏。公网信息走 **HTTPS 多端点回退**（ip.sb→ipinfo，可在 `public_ip` 自定义），**尊重系统代理**、8s 超时，**TUN/代理下也能识别** |
| **适配器 Adapter** | 列出物理/虚拟网卡（IPv4/IPv6/MAC/描述）＋ **配置静态 IP / 掩码 / 网关 / DNS / 切换 DHCP**（带校验 + 二次确认 + WMI 写入）。USB 插拔/启停后台自动刷新 |
| **扫描 Scanner** | 局域网 IP / MAC / 主机名扫描（基于原生 ARP），可自定义 CIDR 与并发数。主机名三级回退：反向 DNS →（VPN/无 PTR 时）NetBIOS → mDNS，不靠系统 DNS 也能认出设备名 |
| **流量 Traffic** | 实时上下行速率、当前会话累计、开机后累计 |
| **诊断 Diagnostics** | **6 合 1**：Ping · 路由跟踪 · 端口扫描 · 公网测速 · **内网测速（iperf 风格对测）** · **链路质量评测（有线/无线）** |
| **设置 Settings** | 扫描并发数、界面语言等 |

### 🔬 亮点：链路质量评测

可**选定具体网卡**（多网卡并存时分别评测），探测从该网卡源 IP 发出。测试全程**持续采样**，
按**多维加权模型**给出 0–100 综合评分与分项明细，并高亮最弱维度：

- **有线**：丢包 / 延迟 / 抖动 + 实时协商链路速率（降速/断链会如实反映）。
- **无线**：在连通性之外，额外采集 RSSI、信号质量、信道/频段/频率、PHY 制式、
  Tx/Rx 协商速率、BSSID、认证/加密，并纳入评分；延迟与 RSSI 双 Sparkline 曲线。

👉 **怎么看这些参数、数值高低代表什么、评分公式原理、有问题往哪查**：
请读 **[链路质量评测指南 docs/link-quality-guide.md](docs/link-quality-guide.md)**。

## 🚀 安装

### 方式一：下载预编译二进制（推荐）

前往 [**Releases**](https://github.com/newcovid/iptools/releases/latest) 下载对应平台的可执行文件，
解压即用，无需安装运行时。

> 适配器 IP 配置写入、ICMP 等部分功能需**管理员权限**，请「以管理员身份运行」。

### 方式二：从源码构建

需要 [Rust 工具链](https://rustup.rs/)（2021 edition）。

```powershell
git clone https://github.com/newcovid/iptools.git
cd iptools
cargo build --release        # 产物：target/release/iptools(.exe)
cargo run                    # 或直接运行
```

## 🎮 使用

```powershell
iptools                      # 启动 TUI
iptools -c D:\path\my.json   # 使用自定义配置文件
```

### 默认快捷键

| 操作 | 按键 |
|---|---|
| 切换标签页（下一个/上一个） | `Tab` / `Shift+Tab` |
| 上 / 下 / 左 / 右 | `↑↓←→` 或 `k j h l` |
| 确认 / 返回上一层 | `Enter` / `Esc` |
| 刷新 | `r` |
| 编辑（适配器配置等） | `e` |
| 开始/停止（测速、链路质量等） | `空格` |
| 切换语言 | `Ctrl+L` |
| 帮助 | `F1` |
| 退出 | `Ctrl+C` / `Ctrl+Q` |

> 底部帮助栏会随当前绑定**动态显示**实际按键。所有动作均可在 `config.json` 重绑，
> 样例见 [`config.example.json`](config.example.json)。

### 配置文件

首次运行在当前目录生成 `config.json`（可经 `-c` 指定路径），字段：

```jsonc
{
  "language": "zh-CN",        // 或 "en-US"
  "scan_concurrency": 256,     // 扫描/端口扫描并发数
  "keybindings": { /* 可选：重绑动作，见 config.example.json */ },
  "session": { /* 自动维护：各页面上次输入的参数，重启回灌；链路质量按网卡键分别保存。无需手改 */ },
  "public_ip": { /* 公网信息端点链(ipsb/ipinfo/plaintext)+是否走系统代理；缺省尊重代理 */ }
}
```

> `session` 段由程序在你改动参数时**自动写入**，不必手工编辑；想清空只需删掉该段或整个 `config.json`，程序会重建为默认值。

## 🏗️ 架构速览

- **事件循环**：`tokio` 异步主循环 + `ratatui` 渲染；`crossterm::EventStream` 在独立任务里把
  `Tick`/`Key`/`Mouse`/`Resize` 经 mpsc 发回主循环。`Tick`（250ms）是数据刷新的唯一驱动。
- **按键解耦**：物理按键 → 语义 `Action` 枚举（`keymap.rs`），各模块只处理 `Action`，故可自由重绑。
- **异步数据流**：模块在触发动作时 `tokio::spawn` 后台任务 → 经 `mpsc` 回传 → 每个 tick 的
  `update()` 里 `try_recv` 消费，UI 永不阻塞；长任务用 `Arc<Mutex<bool>>` 中止。
- **i18n**：`include_str!` 内嵌语言包，单测保证中英文 key 完全一致。

更详细的模块契约与约定见 [`CLAUDE.md`](CLAUDE.md)。

## 💻 平台支持

主要面向 **Windows** 与 **Linux**（Windows 走原生 API；Linux 走 sysfs/getifaddrs/socket2 原始套接字/AF_PACKET ARP/iw/nmcli·netplan·ip 分层后端）。
部分功能天然跨平台，其余在 macOS 等平台为 stub。

| 功能 | Windows | Linux | 其它 Unix |
|---|:---:|:---:|:---:|
| 端口扫描 / 公网测速 / 内网测速 | ✅ | ✅ | ✅ |
| 网卡枚举 / ARP 扫描 | ✅ | ✅（需 CAP_NET_RAW） | ⛔ stub |
| 路由跟踪 / 链路质量 | ✅ | ✅（需 CAP_NET_RAW） | ⛔ stub |
| 无线信息 | ✅ | ✅（需 `iw`） | ⛔ stub |
| IP 配置写入 | ✅ | ✅（nmcli/netplan/ip） | ⛔ stub |
| Ping | ✅ | ✅（需 root，`surge-ping`） | ✅（需 root） |

## Linux 支持

需 Rust 工具链：`cargo build --release`。构建依赖：`pkg-config`、`libssl-dev`（reqwest 在 Linux 走 openssl）。

**权限**：局域网扫描（ARP）、路由跟踪（Trace）、链路质量探测用原始套接字，需 `CAP_NET_RAW`：
- 推荐一次性授权二进制：`sudo setcap cap_net_raw+ep ./target/release/iptools`
- 或直接 `sudo ./target/release/iptools` 运行

**无线详情**：需安装 `iw`（`sudo apt install -y iw`）。

**IP 配置写入**：自动适配后端——NetworkManager（桌面，`nmcli`）/ netplan→systemd-networkd（服务器，写 `/etc/netplan/99-iptools.yaml` + `netplan apply`）/ `ip` 命令兜底（仅本次生效，重启失效）。写入需相应授权（PolicyKit 或 sudo）。

## 🧪 开发

```powershell
cargo check                  # 快速类型检查
cargo test                   # 单元测试（纯逻辑层）
cargo test locale            # 仅跑语言包一致性测试
```

> UI/网络交互无法自动化，靠手动运行 TUI 验证。新增任何 UI 文案后务必 `cargo test`——
> 中英文语言包 key 不一致会导致 `locale_keys_are_in_sync` 失败。

## 🤝 贡献

欢迎 Issue / PR。提交前请确保 `cargo test` 通过、`cargo build --release` 无警告，
并遵循仓库现有代码风格（注释为中文）。

## 📄 许可证

[MIT](LICENSE) © newcovid
