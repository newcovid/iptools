# 架构与开发约定

本文描述 iptools 当前稳定的架构边界和维护约定。面向需要理解实现或提交改动的开发者；使用方法请从项目根目录的 `README.md` 开始。

## 目录结构

```text
src/
├── main.rs                 程序入口与主事件循环
├── app.rs                  全局状态、事件分发和会话持久化
├── event.rs                终端事件与 250 ms tick
├── config.rs               配置加载、默认值和保存
├── session.rs              可持久化的界面参数
├── history.rs              最近使用历史（MRU）
├── keymap.rs               物理按键到语义动作的映射
├── modules/                六个页面及诊断子工具
├── ui/                     全局布局、主题和共享控件
└── utils/                  网络、平台后端、i18n 和格式化工具
```

`assets/locales/` 中的语言包通过 `include_str!` 编译进二进制。`config.example.json` 只展示用户适合手工维护的配置；实际运行产生的 `config.json` 不进入版本控制。

## 运行模型

主线程负责 Ratatui 渲染和状态更新，耗时操作由 Tokio 后台任务执行：

```text
终端输入 / 250 ms tick
          │
          ▼
    EventHandler
          │ mpsc
          ▼
        App
   ┌──────┴──────┐
   │ 事件分发     │ 每帧渲染
   ▼              ▼
当前模块状态 ──> Ratatui
   ▲
   │ try_recv
后台网络任务
```

`App` 是唯一的全局可变状态根。每轮循环先渲染，再等待一个 `Event`；`Tick` 是定时刷新、动画和后台结果消费的统一驱动。模块的 `update()` 必须使用 `try_recv()` 非阻塞地排空消息，不能在 UI 线程等待网络或磁盘操作。

## 模块契约

`src/modules/` 中的页面遵循相同模式：

- `new()`：建立初始状态，必要时启动首次加载。
- `update(&mut self)`：合并后台任务结果，不得阻塞。
- `on_key(...)`：处理当前焦点下的语义动作；文本输入状态可同时接收原始 `KeyEvent`。
- `draw(...)`：只负责渲染和登记当帧鼠标命中区域。

耗时操作使用以下数据流：

```text
用户动作 → tokio::spawn / spawn_blocking → mpsc::Sender → update() → UI 状态
```

持续任务通过共享中止标志停止。新增任务时应保证退出路径会发送最终状态，并避免让已结束的任务继续修改新一轮结果。

## 输入与焦点

### 快捷键

`keymap::Action` 将功能与物理按键解耦。模块只依赖 `Action`，用户绑定由 `KeyMap` 从配置加载。新增动作时必须同步更新：

1. `Action` 枚举及名称转换；
2. `Action::ALL`；
3. 默认组合键；
4. 使用该动作的模块处理逻辑；
5. 必要的中英文帮助文案。

底部帮助栏从当前 `KeyMap` 动态生成，界面中不要写死可配置按键。

### 文本输入与 MRU

所有编辑框使用 `utils::textinput::TextInput`，以获得一致的光标移动、中间插入、删除和鼠标定位行为。输入过滤器位于同一模块，包括 IPv4、CIDR 和主机名规则。

`HistoryStore` 维护三个最近使用池：通用目标、CIDR 和适配器配置值。行尾补全与 `Ctrl+R` 下拉由 `ui::mru` 提供。只有真正执行过的目标才应写入历史，单纯键入不记录。

### 诊断页

诊断页有两级焦点：`App::diag_focused` 控制是否进入诊断交互，模块内的 `FocusArea` 在菜单、主区和配置区之间切换。新增诊断工具时，需要在 `diagnostics/mod.rs` 的更新、输入和渲染分发中完整接线；这些匹配刻意不使用兜底分支，以便编译器发现遗漏。

## 配置与会话

`Config` 包含用户配置、来源路径和自动维护的 `session`。首次运行使用当前目录的 `config.json`，也可通过 `--config` 指定路径。

各工具通过 `export_persist()` / `apply_persist()` 导出和恢复纯数据快照。`App::maybe_persist()` 只在键盘或鼠标动作确实改变快照时保存；定时 `Tick` 不写磁盘。

持久化结构使用容器级 `#[serde(default)]` 和显式 `Default`，以便旧配置缺少新字段时逐字段回退。输入值按界面文本保存，真正执行时仍必须重新解析、限制范围并校验。

链路质量参数按稳定网卡键保存，键的优先级为 GUID、MAC、名称。切换网卡前先归档当前参数，再载入目标网卡参数。

## 平台边界

平台依赖通过 `cfg` 隔离：

| 能力 | Windows | Linux |
|---|---|---|
| 网卡枚举 | IP Helper / `ipconfig` crate | sysfs + getifaddrs |
| ARP | `SendARP` | AF_PACKET 原始套接字 |
| ICMP | Windows ICMP API | `socket2` / `surge-ping` |
| 无线信息 | WLAN API | `iw dev link` |
| IP 配置 | WMI | NetworkManager、netplan、`ip` |

`diagnostics/icmp.rs` 统一 traceroute 与链路质量使用的单次 Echo 原语。连续 Ping 有不同的生命周期和载荷需求，保留独立实现。

Linux 原始套接字功能需要 `CAP_NET_RAW`。网络配置后端可能改变系统连接状态，必须保留输入校验和二次确认；新增或修改写入逻辑后，应在非关键网卡上进行目标平台实测。

## 国际化

界面文本通过 `I18n::t(key)` 在渲染时解析。后台任务不持有语言状态，应回传结构化数据或 i18n 键，避免切换语言后显示旧语言文本。

新增或删除界面文案时必须同时修改：

- `assets/locales/en-US.json`
- `assets/locales/zh-CN.json`

`utils::i18n::tests::locale_keys_are_in_sync` 会校验两份语言包的键完全一致。

## 不变量与安全边界

- UI 线程不执行阻塞网络或系统调用。
- 适配器写入必须经过字段校验和二次确认。
- Windows 子网掩码应从与地址网络匹配的最长有效前缀推导，不能选中 `/32` 主机路由。
- WMI 无入参方法可能返回空的方法参数定义，这不代表方法不存在。
- 共享速率和流量格式统一使用 `utils::format`。
- 鼠标区域只在当前帧登记，不跨帧缓存布局坐标。
- 新增 UI 文案必须保持两份语言包键同步。

## 验证

提交前至少运行：

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all
cargo build --release
```

纯逻辑应优先补单元测试。终端布局、平台 FFI、权限行为和真实网络配置无法完全由 CI 覆盖，需要在相应系统上手动验证。
