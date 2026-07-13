# Changelog

本项目的用户可见变化记录在此。版本号遵循 [Semantic Versioning](https://semver.org/)。

## [0.4.1] - 2026-07-13

### 修复与改进

- 局域网扫描结果按数值 IP 排序，并在异步结果到达时保持当前设备选择；
- 扫描、网卡和流量列表仅在选中项越过视口边界时滚动，双向浏览更自然；
- 扫描范围默认使用当前生效网卡的网络 CIDR，历史范围仍可通过历史记录选择；
- 扫描运行或取消后退出时立即恢复终端，并让主机名探测更及时响应取消；
- 中文界面的语言按钮显示 `Language`，英文界面显示 `切换语言`；
- Web Demo 同步上述共享状态、列表和交互逻辑。

## [0.4.0] - 2026-07-12

### 新增

- GitHub Pages WebAssembly 交互演示：三种确定性场景、DOM/Canvas、触控键栏、离线 PWA 和 portable Web 包；
- Nord、Catppuccin Mocha、Dracula 配色方案；
- Windows 与 Linux 共用的五 crate workspace、单向状态更新和结构化任务管理；
- 诊断参数、CIDR、目标和适配器输入的最近使用历史及鼠标选择；
- 中英文 README、公开架构说明和可重复生成的 Maple Mono CN Web 字体子集。

### 改进

- 原生、原生 Demo 和 Web 共用同一套 reducer 与 Ratatui renderer；
- 完善键盘、鼠标、滚轮、底部快捷操作和文本编辑光标；
- 丰富 Ping 日志、端口服务识别、路由跟踪、局域网厂商/主机名解析和流量展示；
- Dashboard 时间按秒刷新，配置文件改为原子写入；
- Web 拦截应用快捷键、持久化主题和 session，并对齐现代终端 ANSI 色板；
- 升级 Rust Edition、Ratatui、Crossterm、Reqwest、Sysinfo 等依赖，HTTP 默认使用 rustls。

### 兼容性

- 保留 `iptools` 二进制名、CLI 参数和既有配置 JSON 的兼容读取；
- Web 始终使用模拟数据，不读取或修改浏览器所在设备的网络配置。

[0.4.1]: https://github.com/newcovid/iptools/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/newcovid/iptools/compare/v0.3.1...v0.4.0
