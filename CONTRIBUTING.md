# 贡献指南

感谢你改进 iptools。提交 Issue 或 Pull Request 前，请先确认问题边界，并尽量让改动保持单一目的。

## 开发环境

- 稳定版 Rust 工具链；
- Windows 10/11，或常见 x86_64 Linux 发行版；
- Linux 构建需要 `pkg-config` 和 OpenSSL 开发包；
- 测试原始套接字功能时需要管理员权限、root 或 `CAP_NET_RAW`。

```bash
git clone https://github.com/newcovid/iptools.git
cd iptools
cargo build
cargo test --all
```

## 提交流程

1. 先搜索现有 Issue 和 Pull Request，避免重复工作。
2. 对较大的功能或会改变交互的方案，先创建 Issue 说明目标、使用场景和平台影响。
3. 从 `main` 创建短生命周期分支，每个提交保持可构建。
4. 为纯逻辑改动补充单元测试；涉及 Windows/Linux 后端时说明实测环境和权限。
5. 提交前执行完整检查。

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo build --release
```

## 代码约定

- 遵循 `rustfmt` 和 Clippy；不要用无说明的 `allow` 掩盖警告。
- 注释解释约束、原因和平台差异，不复述代码，也不保留临时调试记录或阶段性计划。
- UI 线程不得执行阻塞操作；耗时任务使用 Tokio 后台任务和消息通道。
- 新增按键动作时同步更新动作枚举、名称映射、默认绑定和帮助文案。
- 新增界面文案时同时更新中英文语言包，并运行全部测试。
- 网络配置写入必须保留输入校验和二次确认。
- 不提交 `config.json`、编辑器设置、AI 助手指令或本地工作流文件。

详细实现约定见[架构文档](docs/architecture.md)。

## Pull Request 内容

请在描述中包含：

- 改动解决的问题；
- 关键实现选择及其原因；
- 用户可见影响；
- 已运行的自动检查；
- Windows/Linux 手动测试结果，或尚未覆盖的环境。

请避免把重构、格式化、依赖升级和功能改动混在同一个 Pull Request 中，除非它们不可分割。
