# 架构说明

iptools 使用五个职责单一的 crate，让原生程序、原生 Demo 和 WebAssembly 演示共享同一套状态与界面，同时把系统权限和真实网络 I/O 限制在原生边界内。

## Workspace

```text
crates/
├── iptools-core    领域状态、输入、Message、Effect、RuntimeEvent、配置 schema
├── iptools-ui      Ratatui 渲染、布局、主题和鼠标命中
├── iptools-demo    确定性场景与模拟 Runtime
├── iptools-native  iptools 二进制、Crossterm、Tokio、文件和系统网络后端
└── iptools-web     Ratzilla、浏览器输入、LocalStorage 和 PWA
```

允许的依赖方向：

```text
core <- ui
core <- demo
core + ui + demo <- native
core + ui + demo <- web
```

- `iptools-core` 不依赖 UI、异步运行时、文件系统或网络库；
- `iptools-ui` 不知道 Crossterm、Ratzilla 或真实网络；
- `iptools-demo` 不读取运行设备；
- 平台输入只负责转换事件，平台 Runtime 只负责执行 Effect。

`scripts/check-architecture.ps1` 在 CI 中检查这些边界和禁止的并发模式。

## 单向数据流

```text
keyboard / mouse / touch
          │
          ▼
      InputEvent
          │
          ▼
AppModel::update(Message) ──> Vec<Effect>
          ▲                       │
          │                       ▼
    RuntimeEvent <──── native or demo runtime
          │
          ▼
   render(model, ui)
```

`AppModel` 是业务和导航状态的唯一来源。`UiState` 只保存当前帧布局、光标和鼠标命中区域。渲染函数不执行 I/O、不创建任务，也不读取系统时间。

原生程序、`iptools --demo` 和 Web 都调用同一个 reducer 与 renderer。原生运行真实 Effect，Demo/Web 运行确定性的模拟 Effect；因此平台差异不会形成第二套页面逻辑。

## 原生 Runtime 与并发

`NativeRuntime` 统一管理网络和系统任务：

- 每项工作带 `JobId { tool, generation }`；同一工具的新任务会取消旧任务；
- core 忽略旧 generation 的迟到事件；
- 使用有界通道，避免高频样本无限堆积；
- Ping、链路质量和内网测速样本合并到最高 4 Hz 的 UI 更新；
- 取消使用 `CancellationToken`，子任务由顶层 supervisor 或可等待的局部 `JoinSet` 管理；
- 退出时先取消并等待任务，再恢复终端；
- 领域和 Runtime 错误为强类型，`anyhow` 只用于二进制顶层。

真实 ARP、ICMP、TCP/UDP、无线信息、流量采样和网卡配置均位于 `iptools-native`。Web 构建不包含这些后端，也不会发起真实诊断请求。

## 配置与持久化

可序列化的 `ConfigData` 位于 core。原生与 Web 复用同一个纯函数处理 `PersistPreferences`、`PersistSession` 和适配器编辑参数：

- 原生 `FsConfigStore` 把 JSON 写入临时文件后原子替换；
- Web 把同一 schema 写入 `iptools.web.v1.config` LocalStorage；
- Web 的场景与 renderer 使用独立键，URL 参数优先于本地设置；
- 原生配置路径、系统语言检测和文件 I/O 不进入 core。

## Web 渲染

Web 使用 Ratzilla 0.3.1，并在 `vendor/ratzilla` 中保留少量可审计补丁：

- Canvas 按 Unicode 显示宽度裁剪，避免中文右半部分被截断；
- DOM 在 resize 时安全处理旧帧越界单元；
- DOM 正确恢复全宽字符的续格；
- DOM 使用终端容器尺寸计算行列；
- 命名 ANSI 色使用现代 Windows Terminal Campbell 色板，RGB 主题保持原值。

中文字体是 Maple Mono CN 的固定版本 WOFF2 子集。生成脚本会收集共享 UI、场景和 Web 外壳字符，并验证源字体哈希和必需字形。

浏览器适配器会拦截应用快捷键（包括 `Ctrl+R` 和 `Ctrl+L`）、转换鼠标/滚轮/触控事件、更新本地时钟，并把配置写入 LocalStorage。Web 外壳中的场景、全屏和下载控件不进入共享终端画面。

Service Worker 对页面导航使用 network-first，对哈希静态资源使用 cache-first；首次在线加载后可完整离线运行。所有资源同源，无 CDN 和遥测。

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

Playwright 在 Chromium、Firefox 和 WebKit 中覆盖 DOM/Canvas、键盘、鼠标、触控键栏、主题、历史、离线刷新和同源请求。系统权限、无线 API、原始套接字和网络配置写入仍需在对应平台的非关键网卡上实测。
