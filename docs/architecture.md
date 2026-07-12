# 架构与开发约定

iptools 是一个五 crate virtual workspace。内部架构允许重建，外部必须保留 `iptools` 二进制名、默认 CLI、配置 JSON 和发布包兼容性。

共享渲染器必须遵守 [`ui-compatibility.md`](ui-compatibility.md)：状态和运行时可以现代化，但不得顺带降低 v0.3.1 的终端层级、信息密度与交互体验；Web 专属控制保留在 Ratatui 画面之外。

## Workspace 边界

```text
crates/
├── iptools-core    领域状态、输入、Message、Effect、RuntimeEvent
├── iptools-ui      纯 Ratatui 渲染、布局与命中区域
├── iptools-demo    确定性场景、假时钟和模拟 Runtime
├── iptools-native  原生二进制、Tokio、Crossterm、文件与系统网络
└── iptools-web     Ratzilla、浏览器输入、LocalStorage 与 PWA
```

依赖只能沿以下方向：

```text
core <- ui
core <- demo
core + ui + demo <- native
core + ui + demo <- web
```

`iptools-core` 不依赖 Ratatui、Tokio 或 I/O；`iptools-ui` 不知道 Crossterm/Ratzilla；`iptools-demo` 不读取真实机器。`scripts/check-architecture.ps1` 和 CI 会检查这些边界。

## 单向数据流

平台输入先转换为 `InputEvent`，共享 reducer 是状态变更的唯一入口：

```text
native / web input
        │
        ▼
Message::Input ──> AppModel::update ──> Vec<Effect>
                         ▲                    │
                         │                    ▼
                  RuntimeEvent <── native/demo runtime
                         │
                         ▼
                 render(model, ui)
```

`AppModel` 保存业务与导航状态。`UiState` 只保存 Ratatui StatefulWidget 临时状态、终端尺寸和当前帧鼠标命中区域。`render` 不执行 I/O、不启动任务、不读取系统时间。

Web 和 native `--demo` 使用 `iptools-demo::DemoRuntime` 的固定 seed 与定时事件，因此同一场景产生一致结果。Web 永远不创建真实网络请求。

## 原生 Runtime

顶层唯一的 `NativeRuntime` 使用 `JoinSet<TaskResult>`、容量 512 的有界 mpsc 与 `CancellationToken` 管理共享架构中的任务。每项任务带 `JobId { tool, generation }`；同一工具的新任务先取消旧任务，core 会忽略旧 generation 的迟到事件。退出时 runtime 取消并等待所有子任务；shutdown 在 join 期间持续排空事件队列，避免生产者已被有界队列背压时与退出流程互相等待。终端输入使用独立的容量 128 有界通道，并在恢复终端前 shutdown/join。

跨平台协议当前为 `ARCHITECTURE_VERSION = 2`。Ping、Trace、Port Scan、Public Speed、Link Quality 与 LAN Speed 各自拥有强类型 Request、Sample、Summary 和 Failed RuntimeEvent；禁止用通用字符串进度承载业务数据。

默认原生入口与 `--demo`、Web 现在共用同一个 `AppModel` reducer 和 `iptools-ui` renderer；Crossterm 只转换输入，`NativeRuntime` 只执行 Effect。v0.3.1 的网络算法保留在 native 的诊断算法和系统适配层中，但旧 `App` 状态根、页面私有状态机、重复 renderer、模块通道和迁移桥已经删除。`scripts/check-architecture.ps1` 会阻止这些文件或 `unbounded_channel`、`Arc<Mutex<bool>>`、`AtomicBool` 取消标志重新进入源码。

Scanner、Port Scan、Dashboard、Adapter 读取、Traffic、Adapter Edit、Ping、Trace、Public Speed、Link Quality 与 LAN Speed 均由 `NativeRuntime::dispatch(Effect)` 驱动：ARP/主机名解析、TCP 连接扫描、Dashboard 的系统/公网信息、适配器枚举和流量采样、网卡 DHCP/静态地址写入、ICMP Ping、逐跳跟踪、公网流式下载、源地址绑定的链路探测及局域网 TCP/UDP 吞吐测试均留在 native 边界。所有操作使用 JobId；同一工具的新任务取代旧 generation，core 丢弃迟到结果。Ping、Link Quality 与 LAN Speed 的高频样本最多以 4 Hz 进入 UI；Trace 保留 v0.3.1 的 IPv4 逐跳算法；Public Speed 保留三端点顺序、`no_proxy()`、6 秒连接超时和 15 秒总时长。Link Quality 保留按 GUID/MAC/名称识别网卡、每网卡独立参数、无线 RSSI/PHY/速率采样和有线/无线差异化评分，评分纯函数位于 core。LAN Speed 保留 v0.3.1 的 Server/Client、TCP/UDP、上传/下载/双向、流数、载荷、UDP 限速和丢包/乱序/抖动汇总；协议与包装器统一使用 `CancellationToken`，并等待本地 JoinSet/JoinHandle。诊断工具共用 v0.3.1 的目标 MRU 和 Menu/Main/Config 焦点模型，参数继续写入兼容的 session JSON。Adapter 读取与写入共用单许可 blocking gate，确保不可中断的系统写入完成并被等待后，更新的写入才会执行；Traffic 使用独立快速采样路径，并同步 Dashboard 的实时速率。Adapter Edit 在 core 与 native 边界各自校验请求，区分永久生效、Linux 运行时临时生效与 Demo 模拟生效；Web 永远只修改确定性场景。

错误策略：binary 顶层可用 `anyhow`，领域与 runtime 边界使用强类型错误；结构化日志使用 `tracing`，core 不依赖 tracing。

## 配置

可序列化的 `ConfigData`、会话参数、快捷键文本映射和公网端点定义位于 `iptools-core`，保持现有 Serde schema 与默认值。native 只负责路径、首次运行系统语言检测和 `FsConfigStore` 原子保存。共享 Settings reducer 通过 `Effect::PersistPreferences` 显式请求保存语言与扫描并发数；Dashboard reducer 从同一 `ConfigData` 构造公网查询请求。native Demo 写入 `ConfigData`，Web 写入 `iptools.web.v1.*` LocalStorage。Web 不读写原生配置，也不执行适配器变更或真实 Dashboard 请求。

## Web、字体与 PWA

Web 默认使用 Canvas，中文自动选择 DOM；`?renderer=canvas` / `?renderer=dom` 可显式覆盖。Ratzilla 0.3.1 通过 `vendor/ratzilla` 保留四个窄补丁：

- Canvas 非 ASCII 裁剪区按 Unicode 宽度扩展，避免中文被切成半个字；
- DOM resize 交接帧忽略旧 frame 超出新 cell vector 的单元，下一帧由 Ratatui autoresize 对齐。
- DOM 在全宽字符被替换时恢复其隐藏续格，并禁止续格跨越行边界，避免页面切换后整行逐格左移。
- DOM 向 Ratatui 报告终端容器的实测行列数而非整个浏览器视口，避免底部日志、状态栏和页脚被布局到不可见单元。

Maple Mono CN 字体以 OFL-1.1 许可分发。Web 只携带项目字符、Latin-1、箭头和框线符号的 WOFF2 子集，避免完整 CJK 字体突破首载预算。
子集固定使用 Maple Mono NF CN 7.900 的 SHA-256，并由 `scripts/subset-web-font.py` 从共享源码、场景与 Web 外壳重新生成；CI 验证所有必需字符均存在。

所有资源同源且无遥测。Service Worker 对导航采用 network-first，对静态资源采用 cache-first；首次在线加载后支持离线 Demo。

## 验证

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
cargo check -p iptools-web --target wasm32-unknown-unknown
pwsh scripts/check-architecture.ps1
pwsh scripts/check-web-size.ps1
```

Playwright 覆盖 Chromium、Firefox、WebKit 的键盘、软键、Canvas/DOM 和同源资源约束。真实网络权限、平台 FFI 和适配器写入仍须在 Windows/Linux 非关键网卡上实测。

`cargo audit` 当前会报告信息级 [`RUSTSEC-2026-0097`](https://rustsec.org/advisories/RUSTSEC-2026-0097.html)：`atomic-write-file 0.3.0` 传递引入 `rand 0.9.2`。实际 feature 图未启用该 advisory 的必要 `rand/log` feature，iptools 也没有“自定义 logger 内调用 `rand::rng()`”的触发路径，因此记录为不适用；一旦上游发布不受影响的依赖版本，仍应移除这条传递依赖警告。
