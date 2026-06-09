# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

> 文档语言：中文（项目源注释、`功能列表.md` 与提交记录均为中文）。

## 项目目标

一站式 IP 管理 / 测试 CLI 工具（见 `功能列表.md`）：纯 Rust、模块化、跨平台、"零外部依赖"（不调用 `netsh`/`arp` 等系统命令，改用原生 API）。面向新手，强调美观的 TUI 与流畅键盘交互。

**当前实现进度（重要 — 文档目标 ≠ 已实现）：**

| 模块 (Tab) | `功能列表.md` 目标 | 实际状态 |
|---|---|---|
| Dashboard 概览 | 本地+公网信息聚合 | ✅ 基本完成 |
| Adapter 适配器 | 列出网卡 **+ 配置静态IP/DNS/DHCP**（item 4） | ✅ 展示 + IP 配置均已实现（编辑态 UI + 校验 + 二次确认 + WMI 写入）。⚠️ 写入路径已编译通过但**尚未真机实测**，需在非关键网卡验证 |
| Scanner 扫描 | 局域网 IP/MAC/主机名扫描 | ⚠️ **仅 Windows**。`resolve_mac_address` 在非 Windows 返回 `None`，Unix 上扫不到结果 |
| Traffic 流量 | 实时速率 / 会话 / 累计 | ✅ 完成 |
| Diagnostics 诊断 | Ping/路由跟踪/端口扫描/链路质量/公网测速/内网测速 | ✅ **6/6 全部实现**：Ping、端口扫描、公网测速、路由跟踪、链路质量、内网测速 |
| Settings 设置 | 并发数、语言等 | ✅ 语言 + 扫描并发数（快捷键可经 config.json 自定义） |

修改任何模块前，先核对此表确认它是"已实现需维护"还是"待从零实现"。

## 构建与运行

```powershell
cargo build              # 调试构建
cargo build --release    # 发布构建（单文件二进制，locale 经 include_str! 内嵌，无需附带 assets）
cargo run                # 直接运行 TUI
cargo run -- -c <FILE>   # 用自定义配置文件路径（已实现）
cargo test               # 运行单元测试
cargo test locale        # 只跑 i18n 语言包一致性测试（按名称过滤）
cargo check              # 快速类型检查
```

**测试**：纯逻辑层有单元测试，UI/网络交互无法自动化（靠手动运行 TUI 验证）。关键测试：
- `utils::i18n::tests::locale_keys_are_in_sync` — 两份语言包 key 必须完全一致。**新增任何文案后务必 `cargo test`**，漏翻会让此测试失败。
- `keymap::tests::*` — 快捷键解析 / 覆盖 / 容错。
- `utils::format::tests::*` — 速率/体积格式化。

**主要面向 Windows**：`Cargo.toml` 用 `cfg` 区分平台依赖。Windows 走 `windows` (windows-rs 0.58)、`ipconfig`、`winreg`；非 Windows 走 `surge-ping`（需 root）。许多功能（网卡枚举、ARP、ICMP）只有 Windows 实现，非 Windows 分支大多为 stub。

## 架构

### 事件循环（`main.rs` → `event.rs` → `app.rs`）

单线程 ratatui 渲染 + tokio 异步后台任务的经典组合：

1. `main.rs`：`tokio::main` 异步主循环。每次迭代 `tui.draw()` 后 `await` 一个 `Event`。
2. `event.rs`：`EventHandler` 在独立 tokio 任务里用 `crossterm::EventStream` + 250ms `interval`，把 `Tick`/`Key`/`Mouse`/`Resize` 经 **unbounded mpsc** 发回主循环。**Tick 是数据刷新/动画的唯一驱动**——所有周期性更新都挂在 `app.on_tick()` 上。
3. `app.rs`：`App` 持有全部 6 个模块实例 + `Config` + `I18n` + `KeyMap`，是唯一的可变状态根。`on_key` 先把 `KeyEvent` 经 `keymap` 解析成语义 `Action`，再做全局快捷键与按 `current_tab` 的分发。

### 快捷键 / 动作层（`keymap.rs`）—— 修改按键处理前必读

按键与功能**解耦**：`keymap::Action` 是语义动作枚举（Quit/NextTab/Up/Confirm/Toggle…），`KeyMap` 把物理 `KeyEvent` 解析为 `Action`。各模块的 `on_key` 接收 `Option<Action>` 而非裸 `KeyCode`。

- 用户可在 `config.json` 的 `keybindings` 段覆盖任意动作绑定（见 `config.example.json`）；无法解析的组合被安全忽略。
- **新增一个动作**：在 `Action` 的 `name`/`from_name`/`ALL`/`default_combos` 四处同步，再在用到它的模块 `on_key` 里 `match`。
- **需要文本输入的模块**（scanner 的 CIDR、ping/port_scan 的目标框）同时收到原始 `KeyEvent`，文本态走 `key.code`，普通态走 `action`。
- 底部帮助栏由 `ui/mod.rs::footer_text` 依当前绑定动态生成，勿写死按键名。

### 模块契约（关键模式）

每个模块（`src/modules/*.rs`）遵循同一套约定，新增/修改模块务必沿用：

- `new()` — 构造，通常立刻起后台任务或预取数据
- `update(&mut self)` — 由 `on_tick` 调用；**非阻塞地** `try_recv()` 排空自己的 mpsc，把后台结果合并进 UI 状态。绝不在这里阻塞。
- `on_key(&mut self, key, ...)` — 处理已聚焦时的按键
- `draw(f, area, app)`（自由函数，非方法）— 渲染。`ui/mod.rs::draw` 按 `current_tab` 调用对应模块的 `draw`。

**异步数据流统一为：模块 `new`/触发动作时 `tokio::spawn` 后台任务 → 任务通过 `mpsc::Sender` 回传 → `update()` 在 tick 里 `try_recv` 消费。** Dashboard 的公网 IP、Scanner 的扫描、Ping 都是这个套路。中止用 `Arc<Mutex<bool>>` 的 abort flag（Scanner、Ping）。

### Diagnostics 的嵌套焦点模型

诊断页是唯一有两级焦点的页：`app.diag_focused`（是否进入交互模式，Confirm 进 / Back 出）+ 模块内 `FocusArea`（Menu/Main/Config，NextTab 循环切换）。新增诊断子工具时，需在 `diagnostics/mod.rs` 的 `update`/`on_key`/`draw` 三处 `match current_tool` 各加一个分支，并参照现有工具建子结构。

六个子工具及可参照的范式：
- `ping.rs`（`PingTool`）— 持续型：循环发包，结构化日志 `PingLog` 渲染时本地化。
- `port_scan.rs`（`PortScanTool`）— 批量型：`stream::buffer_unordered` 并发 + 进度条 + abort，复用全局并发数。
- `public_speed.rs`（`PublicSpeedTool`）— 流式型：reqwest 分块下载，任务内计时算瞬时速率，Sparkline 曲线。
- `trace.rs`（`TraceTool`）— 逐跳 ICMP（TTL 递增），Windows 原生，复用 `icmp::echo_once`。
- `link_quality.rs`（`LinkQualityTool`）— ICMP 突发采样 → 延迟/抖动/丢包 → 综合评级；识别有线/无线。
- `lan_speed.rs`（`LanSpeedTool`）— 服务端/客户端 TCP 吞吐对测（iperf 风格），跨平台。

`icmp.rs` 收敛单次 ICMP Echo 的 unsafe FFI（trace/link_quality 共用）；`ping.rs` 因用途不同保留自己的发包循环。子工具统一持有 config/state + `mpsc` 回传 + `Arc<Mutex<bool>>` abort flag；`draw(f, main_area, config_area, i18n, is_focused, active_focus)` 签名一致。**诊断 `match current_tool` 已无 `_` 兜底分支——新增工具须在 update/on_key/draw 三处显式补齐，否则编译报 non-exhaustive。**

### i18n（`utils/i18n.rs` + `assets/locales/*.json`）

`include_str!` 编译期内嵌 `en-US.json`/`zh-CN.json`。`app.t(key)` 查当前语言，缺失则回退英文，再缺失返回 `MISSING:<key>`。**加任何 UI 文案都必须同时在两个 JSON 里加 key**，否则 `cargo test` 的 `locale_keys_are_in_sync` 会失败。首次启动按系统区域（Windows `GetUserDefaultLocaleName` / Unix `LANG`）推断默认语言。异步任务里无法访问 `i18n`：回传 i18n 键而非成品文案，渲染时再翻译（参见 `ping.rs` 的 `PingLog` / `PingEvent::Error{key,detail}`）。

### Adapter IP 配置写入（`modules/adapter_edit.rs` + `utils/ipconfig.rs`）

只读视图按 `Edit`(默认 `e`) 进入编辑态（`AdapterModule.edit: Option<EditForm>`）。表单：模式(DHCP/静态)/IP/掩码/网关/DNS1/DNS2，`Confirm` 先校验(IPv4/连续掩码)再弹**二次确认浮层**，确认后在 `spawn_blocking` 里调 `utils::ipconfig` 写入，结果经 `mpsc` 回传。`utils::ipconfig` 用 **`wmi` crate**（非手写 windows-rs COM——0.58 的 `VARIANT` 不透明、手建 SAFEARRAY 需 transmute 不可靠）调 `Win32_NetworkAdapterConfiguration` 方法。**修改时务必保留校验+确认这两道安全闸**；写入逻辑改动需在非关键网卡实测。

### 配置（`config.rs`）

`config.json` 默认在当前工作目录，可经 `-c/--config` 指定。`Config` 记录来源路径、`save()` 写回同一文件。字段：`language`、`scan_concurrency`、`keybindings`。该文件已**不再纳入 git**（本地运行时状态），参考样例见 `config.example.json`。

### 共享格式化（`utils/format.rs`）

`format_speed`/`format_bytes` 是全项目唯一的人类可读单位实现（二进制单位，含单元测试）。各模块一律复用，勿再各自定义。

## 已知缺陷与 BUG（待处理）

> 历史上的「计数器回绕 panic」「`-c/--config` 失效」「默认语言不一致」三项已修复。

1. **Scanner 只能发现同二层子网主机**：基于 `SendARP`，跨网段、或目标禁 ARP/已离线但有缓存的情况都不可靠；且整列"厂商"(vendor) 在 locale 里标注"暂未实现"（尚无 OUI 数据库）。
2. **公网 IP 用明文 HTTP**：`http://ip-api.com`（非 HTTPS，免费档 45 req/min 限流），并强制 `no_proxy()`。代理环境下仍直连。
3. **Mouse/Resize 事件被吞**：`app.on_mouse`/`on_resize` 为空实现（`EnableMouseCapture` 已开但无逻辑）。

## 待办 / 提示

- **实测 Adapter IP 配置写入**：`utils::ipconfig` 已编译通过但未真机验证；需管理员权限运行，先在非关键网卡测 静态↔DHCP 往返。DHCP 模式下 DNS 清空为尽力而为（空 BSTR 数组在 WMI 中表达繁琐，暂略）。
- **Scanner OUI 表**：`utils::oui` 仅收录少量高置信度前缀，可扩充（或接入完整 OUI 数据库）。
- **跨平台迁移**：当前聚焦 Windows。端口扫描/公网测速/内网测速已天然跨平台；网卡枚举/ARP/ICMP(traceroute,链路质量)/IP 配置仅 Windows，非 Windows 多为 stub 或本地化"不支持"。迁移时在 `cfg(not(windows))` 侧补实现，参照 `utils/net.rs`、`diagnostics/icmp.rs`。
