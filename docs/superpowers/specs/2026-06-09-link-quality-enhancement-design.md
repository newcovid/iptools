# 链路质量诊断增强 — 设计文档

> 日期：2026-06-09 · 模块：Diagnostics → Link Quality（`src/modules/diagnostics/link_quality.rs`）

## 目标

大幅完善诊断页「链路质量」子工具：

1. **网卡选择** — 多网卡并存时可指定要评测的物理网卡；ICMP 探测绑定到该网卡源 IP（真正测「这块卡」的链路，而非系统默认路由）。
2. **专业全面的参数** — 连通性参数更细（min/avg/max、抖动均值+标准差、最长连续丢包），无线网卡额外采集专业射频参数。
3. **无线质量参数** — RSSI(dBm)、信号质量%、PHY 类型/Wi-Fi 代际、频段+信道、收发协商速率、BSSID、认证/加密。
4. **持续采样** — 测试期间每轮探测同时采样射频状态，记录 RSSI 随时间变化（曲线 + min/avg/max）。
5. **多维加权评估体系** — 各维度独立子评分 + 分项进度条，加权汇总为总评级；有线/无线用不同权重。

## 已定决策（来自 brainstorming）

- 探测绑定：**绑定源地址到所选网卡**（改用 `IcmpSendEcho2Ex` 指定 `SourceAddress`）。
- 评估体系：**多维加权综合评分**，分项条 + 加权总评级。
- 射频采样：**测试期间持续采样**（RSSI 曲线 + 统计）。
- 代码组织：拆出独立 `src/utils/wlan.rs` 承载丰富的 WLAN 查询。

---

## 1. 文件与职责

| 文件 | 改动 |
|---|---|
| `src/utils/wlan.rs`（**新增**，Windows） | `WirelessInfo` 结构 + `query(guid) -> Option<WirelessInfo>`。经 `WlanQueryInterface(current_connection)` 取 SSID/BSSID/信号质量%/Rx-Tx 速率/PHY 类型/认证/加密；经 `WlanGetNetworkBssList` 取 **RSSI(dBm) 与频率**；由频率推导信道+频段（2.4/5/6 GHz），生成友好 PHY 标签（如「802.11ax · Wi-Fi 6」）。非 Windows stub → `None`。 |
| `src/utils/net.rs` | `InterfaceInfo` 增加 `link_speed_bps: Option<u64>`（取自 `adapter.transmit_link_speed()`），用于有线链路速率展示。 |
| `src/modules/diagnostics/icmp.rs` | 新增 `echo_once_from(src, dest, ttl, timeout_ms, payload_len)`，用 **`IcmpSendEcho2Ex`** 指定源地址，使探测从所选网卡出口。现有 `echo_once`（trace 用）保持不动。 |
| `src/modules/diagnostics/link_quality.rs` | 大改：网卡选择器、持续采样、多维评分、丰富布局。 |

---

## 2. WLAN 查询（`utils/wlan.rs`）

```rust
#[derive(Debug, Clone)]
pub struct WirelessInfo {
    pub ssid: String,
    pub bssid: String,          // AP MAC，冒号分隔
    pub signal_quality: u32,    // 0-100
    pub rssi_dbm: i32,          // 来自 BSS list（若取不到则由 quality 近似）
    pub phy_type: String,       // 友好标签，如「802.11ax · Wi-Fi 6」
    pub band: String,           // "2.4 GHz" / "5 GHz" / "6 GHz"
    pub channel: u32,           // 由频率推导
    pub freq_mhz: u32,
    pub rx_rate_mbps: u32,      // ulRxRate 单位 kbps → /1000
    pub tx_rate_mbps: u32,
    pub auth: String,           // 如 WPA2-Personal
    pub cipher: String,         // 如 CCMP/AES
}

#[cfg(target_os = "windows")]
pub fn query(guid: &windows::core::GUID) -> Option<WirelessInfo>;
#[cfg(not(target_os = "windows"))]
pub fn query(_guid: &()) -> Option<WirelessInfo> { None }
```

要点：
- `current_connection` 给出 `wlanAssociationAttributes`（SSID、dot11Bssid、dot11PhyType、wlanSignalQuality、ulRxRate、ulTxRate）与 `wlanSecurityAttributes`（dot11AuthAlgorithm、dot11CipherAlgorithm）。
- BSS list 用 BSSID 匹配当前关联项，取 `lRssi`（dBm）与 `ulChCenterFrequency`（kHz → MHz）。
- 频率→信道：2.4G `ch = (freq-2407)/5`；5G `ch=(freq-5000)/5`；6G `ch=(freq-5950)/5`（freq 用 MHz）。频段按频率区间判定。
- PHY 类型经 `DOT11_PHY_TYPE` 映射友好标签与 Wi-Fi 代际。
- 取不到 BSS list 时，RSSI 由 `quality` 近似：`rssi ≈ quality/2 - 100`（Windows 标准映射 0%↔-100dBm，100%↔-50dBm）。

实现期间统一封装 unsafe FFI，句柄 open/close 成对，参照 `net.rs::get_ssid_map_via_win32`。

---

## 3. ICMP 源绑定（`icmp.rs`）

新增：

```rust
#[cfg(target_os = "windows")]
pub fn echo_once_from(src: Ipv4Addr, dest: Ipv4Addr, ttl: u8, timeout_ms: u32, payload_len: usize) -> EchoResult;
```

- 用 `IcmpSendEcho2Ex(handle, Event=None, ApcRoutine=None, ApcContext=None, SourceAddress=src_u32, DestinationAddress=dest_u32, ...)`。
- 解析回复缓冲沿用现有 `ICMP_ECHO_REPLY` 路径（`status==u32::MAX` 表示本地调用失败/平台不支持）。
- payload 长度由参数控制（替代当前硬编码 32 字节），REPLY_SIZE 相应保证足够。
- 非 Windows stub 返回 `status=u32::MAX`。

---

## 4. 配置面板字段

经现有 `config_field_item` 渲染。共 6 个字段：

| # | 字段 | 类型 | 默认 | 说明 |
|---|---|---|---|---|
| 0 | 网卡 Interface | **选择器**（←/→ 切换） | 默认路由网卡 | 列出活跃物理网卡（up + 有 IPv4），显示名称 + IPv4 |
| 1 | 目标 Target | 文本（host/IP，`filter_host`） | `8.8.8.8` | |
| 2 | 次数 Count | 数字 | 20 | clamp 5..100 |
| 3 | 间隔 Interval(ms) | 数字 | 200 | clamp 50..5000 |
| 4 | 超时 Timeout(ms) | 数字 | 1000 | clamp 100..10000 |
| 5 | 包大小 Packet size(B) | 数字 | 32 | clamp 0..1472 |

- 选择器字段：`Left`/`Right` 在候选网卡间循环；渲染当前网卡名 + IPv4，附「←→ 切换」提示。
- 文本/数字字段沿用现有带光标编辑与提示（直接输入 / 仅数字）。
- 候选列表为空（无活跃物理网卡）时，选择器显示「无可用网卡」，开始即报错。
- 选择器索引随枚举刷新做边界保护（网卡数变化时夹紧）。

---

## 5. 采集的指标

- **连通性（始终）**：sent/recv、丢包%、min/avg/max 延迟、抖动（平均绝对差 + 标准差）、最长连续丢包数。
- **无线（所选网卡为 Wi-Fi 时）**：SSID、BSSID、信号质量%（min/avg/max）、RSSI dBm（min/avg/max，持续采样）、PHY 类型 + Wi-Fi 代际、频段 + 信道、频率、收发协商速率、认证 + 加密。
- **有线（所选网卡为以太网时）**：协商链路速率、媒体状态、MAC、IPv4。

每轮探测后采样动态射频字段（RSSI、质量、Tx/Rx），存入样本序列 → 支持 RSSI 时间曲线；静态字段（SSID/BSSID/PHY/频段/信道/认证）开始时采一次。

样本结构示意：

```rust
struct Sample {
    latency_ms: Option<u64>,
    rssi_dbm: Option<i32>,
    quality: Option<u32>,
}
```

---

## 6. 评分模型

各维度 → 0–100 子评分（各配一条进度条）：

| 维度 | 来源 | 100 ↔ 0 |
|---|---|---|
| 延迟 Latency | avg RTT | ≤20ms ↔ ≥300ms |
| 抖动 Jitter | 平均绝对差 | ≤2ms ↔ ≥80ms |
| 丢包 Loss | 丢包% | 0% ↔ ≥10% |
| 信号 Signal *(Wi-Fi)* | RSSI dBm | ≥-50 ↔ ≤-85 |
| 速率 Rate *(Wi-Fi)* | 协商速率 | ≥433Mbps ↔ ≤6Mbps |
| 制式 PHY gen *(Wi-Fi)* | Wi-Fi 6/6E=100, 5=80, 4=60, legacy=30 |

- 子评分映射为线性夹紧（端点外取 0/100），实现为纯函数便于单测。

**权重**：

- 有线：丢包 40 / 延迟 35 / 抖动 25。
- 无线：丢包 25 / 延迟 20 / 抖动 15 / 信号 25 / 速率 10 / 制式 5。

总分 = 加权和 → 评级带：**Excellent ≥85 / Good ≥70 / Fair ≥50 / Poor <50**（复用现有 4 级枚举 `Grade` 与配色）。

- 高亮最弱维度（瓶颈一目了然）。
- 无样本时总评级为「-」（沿用现有空态）。

---

## 7. 主面板布局（上→下）

1. **表头**：网卡名 + 有线/Wi-Fi 标识 + （SSID | 链路速率）。
2. **总评级 gauge**：分数% + 配色评级标签。
3. **分项评分条**：有线 3 条，Wi-Fi 6 条。
4. **紧凑双列 key:value 指标栅格**（延迟 min/avg/max、抖动、丢包；Wi-Fi 追加 RSSI avg、信号%、信道/频段、PHY、Tx/Rx、BSSID）。
5. **曲线**：延迟历史 sparkline；Wi-Fi 追加 RSSI 历史 sparkline。
6. **状态行**：运行/停止 + 开始/停止提示 + 错误。

- 50% 宽度偏紧：各区用 `Min` 约束，小终端优雅截断。
- 未聚焦时沿用现有 focus hint 空态。

---

## 8. 异步数据流

沿用模块契约：`start()` 时快照所选网卡（源 IP、介质、有线/无线静态信息）→ `tokio::spawn` 后台循环：每轮 `echo_once_from`（spawn_blocking）+ 采样 `wlan::query`（spawn_blocking）→ 经 `mpsc` 回传 `Sample` → `update()` 在 tick 里 `try_recv` 合并。中止用现有 `Arc<Mutex<bool>>` abort flag。射频静态信息在 `start()`（主线程）即查一次存入 state，避免异步任务里翻译/借用问题。

`LinkEvent` 扩展为携带 RSSI/quality 的样本变体；错误仍回传 i18n 键。

---

## 9. i18n

在 `en-US.json` 与 `zh-CN.json` **同时**新增全部 key：网卡/间隔/包大小标签、各维度名（延迟/抖动/丢包/信号/速率/制式）、无线参数标签（RSSI/信号质量/信道/频段/PHY/BSSID/认证/加密/Tx/Rx）、有线参数标签（链路速率/媒体状态）、min/avg/max、最弱维度提示、「无可用网卡」「无线信息不可用」、「←→ 切换」提示等。`cargo test locale_keys_are_in_sync` 必须通过。

---

## 10. 非 Windows

- `wlan::query` stub → `None`：无线区显示「无线信息不可用」，评分回退为「仅连通性」权重（丢包/延迟/抖动按有线权重归一）。
- `echo_once_from` stub 返回 `status=u32::MAX` → 首轮即报「不支持」退出（沿用现有 `diag_link_unsupported`）。
- `link_speed_bps` 非 Windows 为 `None`。

---

## 11. 测试

- 纯函数单测：各子评分映射（端点 + 中间值）、加权总分 + 评级带（有线 & 无线两组用例）、频率→信道映射、RSSI 由 quality 近似的映射。
- `utils::i18n::tests::locale_keys_are_in_sync`（新增文案后）。
- UI/FFI（WLAN 查询、源绑定 ICMP）无法自动化：手动运行 TUI + 管理员权限在多网卡（含一块 Wi-Fi）真机验证。

---

## 12. 范围之外（YAGNI）

- 信道利用率/邻居 AP 干扰扫描（需周期性 BSS 扫描，开销大）。
- IPv6 探测（现工具即 IPv4-only）。
- 历史结果持久化/导出。
- 双工模式、MTU（价值有限，暂不纳入有线指标）。
