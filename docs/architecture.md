# 架构与开发约定

iptools 是一个五 crate virtual workspace。内部架构允许重建，外部必须保留 `iptools` 二进制名、默认 CLI、配置 JSON 和发布包兼容性。

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

## 原生 Runtime 与迁移桥

`RuntimeSupervisor` 使用 `JoinSet`、容量 512 的有界 mpsc 与 `CancellationToken` 管理共享架构中的任务。每项任务带 `JobId { tool, generation }`；同一工具的新任务先取消旧任务，core 会忽略旧 generation 的迟到事件。退出时 supervisor 取消并等待所有子任务。

真实原生页面的既有网络算法暂时保留在 `iptools-native/src/modules` 和 `utils` 中，作为兼容迁移桥；新增跨端行为必须进入 core/ui/runtime 边界，不能继续增加 detached spawn 或无界通道。迁移桥删除前，真实网络算法行为不得因 Web 展览而改变。

错误策略：binary 顶层可用 `anyhow`，领域与 runtime 边界使用强类型错误；结构化日志使用 `tracing`，core 不依赖 tracing。

## 配置

原生配置保持现有 Serde schema 与默认值。保存通过临时文件、flush 和原子替换完成。Web 只在 `iptools.web.v1.*` LocalStorage 键中保存语言、场景和渲染器，不读写原生配置，也不执行适配器变更。

## Web、字体与 PWA

Web 默认使用 Canvas，中文自动选择 DOM；`?renderer=canvas` / `?renderer=dom` 可显式覆盖。Ratzilla 0.3.1 通过 `vendor/ratzilla` 保留两个窄补丁：

- Canvas 非 ASCII 裁剪区按 Unicode 宽度扩展，避免中文被切成半个字；
- DOM resize 交接帧忽略旧 frame 超出新 cell vector 的单元，下一帧由 Ratatui autoresize 对齐。

Maple Mono CN 字体以 OFL-1.1 许可分发。Web 只携带项目字符、Latin-1、箭头和框线符号的 WOFF2 子集，避免完整 CJK 字体突破首载预算。

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
