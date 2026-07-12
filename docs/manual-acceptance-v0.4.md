# v0.4 人工复核流程与验收标准

本文用于自动化检查全绿后的发布前人工验收。目标不是重新设计界面，而是确认 v0.4 在更换内部架构后仍保留 v0.3.1 的功能、布局和操作体验，并确认 Web 只运行模拟数据。

## 1. 准备

- 两个相同尺寸的终端：左侧运行 v0.3.1 release 或标签 `v0.3.1`，右侧运行当前 v0.4 release。
- 分别使用 `80×24`、`120×36`、`160×48`，并在中文、英文下重复核心项目。
- Windows 10/11 和当前 Ubuntu LTS 各执行一次原生验收；写网卡配置时使用非关键测试网卡。
- Chromium、Firefox、WebKit 各执行一次 Web 验收。
- 保存版本号、操作系统、终端、字体、浏览器版本、截图和失败日志。

开始前执行：

```text
iptools --version
iptools --config <临时配置路径>
iptools --demo --scenario home-network
```

通过标准：二进制名、CLI 参数和配置 JSON 与 v0.3.1 兼容；首次启动能创建配置，退出后终端光标、颜色、鼠标和回显恢复正常。

## 2. 外壳与 UI 对照

逐个尺寸检查：

1. 标题为 `IP Tools CLI`，Demo 只增加 `DEMO` 后缀。
2. 六个标签、分隔符、绿色/青色主题、黄色焦点边框和一行页脚与 v0.3.1 同级。
3. 中文没有半字、截断后残留、边框错位或页面切换后的续格污染。
4. `Tab`、`Shift+Tab`、方向键/WASD、鼠标点击、滚轮、`F1` 和 `Ctrl+L` 均有效。
5. 在临时配置中把退出改为 `Ctrl+x`、向下改为 `j`；重启后页脚/帮助显示实际绑定，文本输入框仍可输入普通字符。

通过标准：允许不同字体造成像素差异，但面板比例、字段、状态、选择、颜色角色和信息密度不得低于 [`ui-compatibility.md`](ui-compatibility.md)。

## 3. 六页功能

### Dashboard

- 启动后显示主机、OS、活动网卡、描述、DHCP/静态、本地 IP、代理和公网信息。
- 观察 5 秒，实时与累计流量持续更新；断网时仍保留本机信息并显示明确公网错误。
- 按 `r` 可重新获取；不得周期性重复请求公网端点。

### Adapters 与 Adapter Edit

- 列表包含活动和非活动网卡，详情包含类型、SSID、MAC、IPv4/CIDR、IPv6、链路与流量。
- 编辑 DHCP/静态模式，验证非法 IP、掩码、网关和 DNS；取消不得写系统。
- 在非关键测试网卡上确认二次确认、权限错误、成功结果和刷新后的实际值。

### Scanner

- 编辑 CIDR，启动、停止、再次启动；确认进度、IP、MAC、厂商和主机名。
- 新任务替换旧任务后，旧结果不得混入；退出时扫描立即取消且无遗留进程/线程。

### Traffic

- 每行显示 RX/TX 实时速率、会话累计和系统累计；选择项在刷新后按网卡名保留。
- 制造流量后速率变化，停止流量后回落；虚拟/忽略接口过滤与 v0.3.1 一致。

### Diagnostics

对每个工具执行 `start → cancel → restart → success`，并至少制造一次失败：

- Ping：目标、间隔、超时、载荷；回复/超时、累计丢包和延迟统计。
- Trace：目标、跳数、超时；逐跳地址、RTT、主机名和权限错误。
- Port Scan：目标、起止端口、超时；开放端口服务名、进度、选中结果。
- Link Quality：网卡、目标和探测参数；有线/无线维度、RSSI/PHY/速率与评级。
- Public Speed：端点回退、当前/平均/峰值、总量和取消；系统代理行为与说明一致。
- LAN Speed：两台机器分别做 Server/Client；覆盖 TCP/UDP、上传/下载/双向，检查流数、载荷、UDP 限速、丢包、乱序和抖动。

通过标准：运行中参数锁定；取消后不再出现新样本；重启后的旧 generation 事件不污染当前结果；样本刷新平稳且 UI 不堆积卡顿。

### Settings

- 切换语言、调整扫描并发数、清除参数记忆并重启。
- 清除后保留当前页面位置，但工具参数、MRU 和 Adapter Edit 表单恢复默认。

## 4. Demo 与 Web

分别验证 `home-network`、`wifi-degraded`、`multi-adapter`：

1. native `--demo` 与 Web 在同场景、同语言下产生一致的关键结果顺序。
2. URL 参数优先于 LocalStorage；重置后恢复场景默认。
3. 中文默认 DOM、英文默认 Canvas；显式 `renderer=dom|canvas` 始终生效。
4. 键盘、鼠标、滚轮和触控软键栏均可完成 Scanner、Port Scan 和六个诊断工具。
5. 首次在线加载后断网刷新成功；更新提示由用户确认后刷新。
6. 浏览器 Network 面板只出现同源静态资源，无遥测、真实扫描或外部 API。
7. 80%–150% 缩放下中文双宽字符不系统性错位；窄屏竖屏显示横屏提示。
8. portable Web dist 放到个人服务器的子路径后可加载和离线使用。

通过标准：页面始终显示 `v0.4 PREVIEW / SIMULATED DATA`；Adapter Edit 只返回模拟结果，浏览器所在机器的网络配置不改变。

## 5. 退出、性能与安全

- 在 Scanner、Ping、Public Speed 和 LAN Server 等运行状态下直接退出，确认终端恢复且进程结束。
- 连续快速 start/cancel/restart 20 次，无 panic、死锁、迟到结果或后台任务遗留。
- Canvas `120×36` 连续输入响应 p95 不超过 100 ms。
- JS+WASM 压缩后不超过 2.5 MiB，总首载不超过 4 MiB。
- `cargo audit` 无未处置漏洞；依赖树不含 OpenSSL。

## 6. 签署记录

每个平台填写一份：

| 项目 | 结果/证据 |
|---|---|
| commit / version | |
| OS / terminal / font | |
| UI 三尺寸、中英文 | Pass / Fail + 截图 |
| 六页与六诊断工具 | Pass / Fail + 日志 |
| 退出与取消 | Pass / Fail |
| 配置与自定义快捷键 | Pass / Fail |
| Web 三浏览器与离线 | Pass / Fail + 报告 |
| 权限与非关键网卡写入 | Pass / Fail / Not tested |
| 审核人、日期 | |

发布闸门：所有必测项 Pass；`Not tested` 仅允许明确不适用的平台项。任何功能缺失、配置不兼容、中文错位、真实 Web 请求、退出遗留任务或 v0.3.1 关键布局退化都阻止 v0.4.0 发布。
