# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

> 文档语言：中文（项目源注释、`功能列表.md` 与提交记录均为中文）。

## 项目目标

一站式 IP 管理 / 测试 CLI 工具（见 `功能列表.md`）：纯 Rust、模块化、跨平台、"零外部依赖"（不调用 `netsh`/`arp` 等系统命令，改用原生 API）。面向新手，强调美观的 TUI 与流畅键盘交互。

**当前实现进度（重要 — 文档目标 ≠ 已实现）：**

| 模块 (Tab) | `功能列表.md` 目标 | 实际状态 |
|---|---|---|
| Dashboard 概览 | 本地+公网信息聚合 | ✅ 基本完成 |
| Adapter 适配器 | 列出网卡 **+ 配置静态IP/DNS/DHCP**（item 4） | ⚠️ **只读**。展示已完成；**IP 配置功能完全未实现** |
| Scanner 扫描 | 局域网 IP/MAC/主机名扫描 | ⚠️ **仅 Windows**。`resolve_mac_address` 在非 Windows 返回 `None`，Unix 上扫不到任何结果 |
| Traffic 流量 | 实时速率 / 会话 / 累计 | ✅ 完成 |
| Diagnostics 诊断 | Ping/路由跟踪/端口扫描/链路质量/公网测速/内网测速 | ⚠️ **仅 Ping 实现**；其余 5 个工具是 `"Coming Soon..."` 占位（`diagnostics/mod.rs` draw 的 `_ =>` 分支） |
| Settings 设置 | 并发数、语言等 | ✅ 仅语言 + 扫描并发数两项 |

修改任何模块前，先核对此表确认它是"已实现需维护"还是"待从零实现"。

## 构建与运行

```powershell
cargo build              # 调试构建
cargo build --release    # 发布构建（单文件二进制，locale 经 include_str! 内嵌，无需附带 assets）
cargo run                # 直接运行 TUI
cargo check              # 快速类型检查（首次约 70s，含全部依赖编译）
cargo run -- -c <FILE>   # 注意：-c/--config 参数当前被解析但忽略，见下方缺陷
```

无测试套件（`cargo test` 无用例）、无 lint 配置。验证改动靠 `cargo check` + 手动运行 TUI。

**主要面向 Windows**：`Cargo.toml` 用 `cfg` 区分平台依赖。Windows 走 `windows` (windows-rs 0.58)、`ipconfig`、`winreg`；非 Windows 走 `surge-ping`（需 root）。许多功能（网卡枚举、ARP、ICMP）只有 Windows 实现，非 Windows 分支大多为 stub。

## 架构

### 事件循环（`main.rs` → `event.rs` → `app.rs`）

单线程 ratatui 渲染 + tokio 异步后台任务的经典组合：

1. `main.rs`：`tokio::main` 异步主循环。每次迭代 `tui.draw()` 后 `await` 一个 `Event`。
2. `event.rs`：`EventHandler` 在独立 tokio 任务里用 `crossterm::EventStream` + 250ms `interval`，把 `Tick`/`Key`/`Mouse`/`Resize` 经 **unbounded mpsc** 发回主循环。**Tick 是数据刷新/动画的唯一驱动**——所有周期性更新都挂在 `app.on_tick()` 上。
3. `app.rs`：`App` 持有全部 6 个模块实例 + `Config` + `I18n`，是唯一的可变状态根。`on_key` 负责全局快捷键和按 `current_tab` 的事件分发。

### 模块契约（关键模式）

每个模块（`src/modules/*.rs`）遵循同一套约定，新增/修改模块务必沿用：

- `new()` — 构造，通常立刻起后台任务或预取数据
- `update(&mut self)` — 由 `on_tick` 调用；**非阻塞地** `try_recv()` 排空自己的 mpsc，把后台结果合并进 UI 状态。绝不在这里阻塞。
- `on_key(&mut self, key, ...)` — 处理已聚焦时的按键
- `draw(f, area, app)`（自由函数，非方法）— 渲染。`ui/mod.rs::draw` 按 `current_tab` 调用对应模块的 `draw`。

**异步数据流统一为：模块 `new`/触发动作时 `tokio::spawn` 后台任务 → 任务通过 `mpsc::Sender` 回传 → `update()` 在 tick 里 `try_recv` 消费。** Dashboard 的公网 IP、Scanner 的扫描、Ping 都是这个套路。中止用 `Arc<Mutex<bool>>` 的 abort flag（Scanner、Ping）。

### Diagnostics 的嵌套焦点模型

诊断页是唯一有两级焦点的页：`app.diag_focused`（是否进入交互模式，Enter 进 / Esc 出）+ 模块内 `FocusArea`（Menu/Main/Config，Tab 循环切换）。新增诊断子工具时，需在 `diagnostics/mod.rs` 的 `update`/`on_key`/`draw` 三处 `match current_tool` 各加一个分支，并参照 `ping.rs` 的 `PingTool`（自带 config/stats/mpsc/abort）建子结构。

### i18n（`utils/i18n.rs` + `assets/locales/*.json`）

`include_str!` 编译期内嵌 `en-US.json`/`zh-CN.json`。`app.t(key)` 查当前语言，缺失则回退英文，再缺失返回 `MISSING:<key>`。**加任何 UI 文案都必须同时在两个 JSON 里加 key**，否则界面显示 `MISSING:...`。

### 配置（`config.rs`）

`config.json` 读写**固定在当前工作目录**，非用户目录。`Config::load()` 文件不存在时写默认值。改语言/并发数会即时 `save()`。

## 已知缺陷与 BUG

按修复优先级大致排序：

1. **流量计数器回绕 panic 风险**：`dashboard.rs:189` 与 `adapter.rs:56` 用裸减法 `rx - prev.total_rx` 算速率。网卡切换或计数器重置导致 `rx < prev` 时，debug 构建会 panic、release 会算出巨大错误值。`traffic.rs` 已正确使用 `saturating_sub`——应统一改为 `saturating_sub`。
2. **`-c/--config` 参数失效**：`main.rs` 解析了 `args.config` 但赋给 `_args` 丢弃，`Config::load()` 永远只读 `./config.json`。要么实现自定义路径，要么移除该参数避免误导。
3. **默认语言不一致**：`i18n.rs` 的 `Language::default()` = `En`，但 `config.rs` 的 `Config::default()` = `Zh`。注释称"会根据系统环境调整"，但**并无系统语言检测逻辑**。
4. **Scanner 只能发现同二层子网主机**：基于 `SendARP`，跨网段、或目标禁 ARP/已离线但有缓存的情况都不可靠；且整列"厂商"(vendor) 在 locale 里标注"暂未实现"。
5. **公网 IP 用明文 HTTP**：`http://ip-api.com`（非 HTTPS，免费档 45 req/min 限流），并强制 `no_proxy()`。代理环境下仍直连。
6. **Mouse/Resize 事件被吞**：`app.on_mouse`/`on_resize` 为空实现（`EnableMouseCapture` 已开但无逻辑）。

## 添加新功能的提示

- **实现 Diagnostics 待办工具**：照 `ping.rs` 建独立子结构体，挂到 `diagnostics/mod.rs` 三处 match；记得加 locale key。
- **实现 Adapter 的 IP 配置（item 4）**：这是最大的未实现目标，需要 Windows 侧改写网卡配置（`netsh` 被禁用，须用 `windows-rs` 的 `CreateUnicastIpAddressEntry`/`SetInterfaceDnsSettings` 等原生 API），并设计 Adapter 模块的编辑态 UI（当前 Adapter 完全只读）。
- **跨平台**：任何新增网络能力都要在 `cfg(target_os = "windows")` 与 `cfg(not(...))` 两侧都给实现或明确 stub，参照 `utils/net.rs`、`diagnostics/ping.rs`。
