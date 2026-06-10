# iptools Linux 跨平台移植评估报告

> 生成日期：2026-06-10 | 基于 commit: ef7b364 (v0.2.0)

---

## 一、总览

| 维度 | 现状 |
|------|------|
| 当前平台 | Windows x86_64（唯一完整支持） |
| Linux 编译 | ✅ 可编译（`cfg` 分支齐全，无编译错误） |
| Linux 运行 | ⚠️ 仅 Ping 可用，其余 5 个模块为 stub |
| 已有跨平台基础 | `surge-ping`（ICMP）、`dns_lookup`（DNS）、标准 UDP（NetBIOS/mDNS） |

### 模块可用性矩阵（Linux 运行时）

| 模块 | Windows | Linux | 缺失原因 |
|------|---------|-------|----------|
| Dashboard 概览 | ✅ | ⚠️ 无活跃网卡 | `get_interfaces()` 返回空 |
| Adapter 适配器 | ✅ | ❌ 空列表 | 同上 |
| Scanner 扫描 | ✅ | ⚠️ 无 MAC | `resolve_mac_address` 无 Linux 实现 |
| Traffic 流量 | ✅ | ❌ 无数据 | 依赖 `get_interfaces()` |
| Ping | ✅ | ✅ | `surge-ping` 已实现 |
| Trace 路由 | ✅ | ❌ stub | `icmp::echo_once` 无 Linux 实现 |
| Port Scan 端口扫描 | ✅ | ✅ | 纯 TCP，天然跨平台 |
| Link Quality 链路质量 | ✅ | ❌ stub | `icmp::echo_once_from` 无 Linux 实现 |
| Public Speed 公网测速 | ✅ | ✅ | reqwest，天然跨平台 |
| LAN Speed 内网测速 | ✅ | ✅ | 纯 TCP，天然跨平台 |
| Settings 设置 | ✅ | ✅ | 纯逻辑 |

---

## 二、Hard Block 清单（Linux 完全不可用）

### 2.1 网卡枚举 — `utils/net.rs::get_interfaces()`

**现状：** 非 Windows 返回空 `Vec`，导致适配器列表、Dashboard 活跃网卡、流量监控全部失效。

**Linux 实现方案：**

| 方案 | 依赖 | 侵入性 | 推荐 |
|------|------|--------|------|
| A. 读 `/sys/class/net/` + `ioctl` | 无额外 crate | 低 | ⭐ |
| B. `netlink-packet-route` + `netlink-sys` | +2 crate | 中 | |
| C. `pnet` crate | +1 crate（较大） | 低 | |

**推荐方案 A**：读 `/sys/class/net/` 获取接口列表、MAC、operstate、speed；用 `ioctl(SIOCGIFADDR)` 获取 IPv4 地址；用 `getifaddrs`（libc）获取 IPv6 和前缀长度。

**需要实现的字段映射：**

| InterfaceInfo 字段 | Linux 数据源 |
|-------------------|-------------|
| `name` | `/sys/class/net/<name>` 目录名 |
| `guid` | 无直接对应，用接口名或 MAC 替代 |
| `mac` | `/sys/class/net/<name>/address` |
| `ipv4` | `ioctl(SIOCGIFADDR)` 或 `getifaddrs` |
| `ipv6` | `getifaddrs` |
| `cidr` | `getifaddrs` 的 `ifa_prefixlen` |
| `is_up` | `/sys/class/net/<name>/operstate` == "up" |
| `is_physical` | `/sys/class/net/<name>/device` 是否存在 |
| `dhcp_enabled` | 读 NetworkManager/`/run/systemd/netif/leases/` |
| `ssid` | `ioctl(SIOCGIWESSID)` 或 `wpa_supplicant` D-Bus |
| `link_speed` | `/sys/class/net/<name>/speed` |

**预估工作量：** 3-5 天（含测试）

**难度：** ⭐⭐⭐ 中等 — 需要 unsafe `ioctl`/FFI，但逻辑清晰。

---

### 2.2 ICMP Echo — `diagnostics/icmp.rs`

**现状：** `echo_once()` 和 `echo_once_from()` 在非 Windows 返回 `status: u32::MAX`，导致路由跟踪和链路质量完全不可用。

**Linux 实现方案：**

`surge-ping` crate 已经是项目的非 Windows 依赖，且 `ping.rs::run_ping_unix()` 已在使用。问题在于 `icmp.rs` 的两个函数没有复用它。

**方案：** 在 `#[cfg(not(windows))]` 分支中用 `surge-ping` 实现 `echo_once()`：

```rust
#[cfg(not(windows))]
pub fn echo_once(target: &str, timeout_ms: u64, ttl: u32, size: usize) -> EchoResult {
    // surge-ping 的 PingSequence、PingIdentifier 需要 tokio 运行时
    // 因此需要 block_on 或传入 runtime handle
}
```

**难点：**
- `surge-ping` 是异步 API，而 `echo_once` 是同步调用（在 `spawn_blocking` 中调用）。需要用 `tokio::runtime::Handle::current().block_on()` 或 `futures::executor::block_on()` 桥接。
- `echo_once_from()`（源地址绑定）：`surge-ping` 支持 `bind` 参数，可以指定源 IP。
- TTL 设置：`surge-ping` 的 `Pinger` 支持 `.ttl()` 方法。

**预估工作量：** 1-2 天

**难度：** ⭐⭐ 低 — `surge-ping` 已有完整 API，只需桥接同步/异步。

---

### 2.3 IP 配置写入 — `utils/ipconfig.rs`

**现状：** 非 Windows 返回 `Err("not supported")`，无法设置静态 IP 或切换 DHCP。

**Linux 实现方案：**

| 方案 | 说明 | 权限 | 推荐 |
|------|------|------|------|
| A. 调用 `ip` 命令 | `ip addr add/del`, `ip route`, `ip link set` | root | ⭐ 快速 |
| B. `netlink` crate | 直接发 netlink 消息 | root | |
| C. D-Bus 调用 NetworkManager | `org.freedesktop.NetworkManager` | user (PolicyKit) | |

**推荐方案 A**（第一阶段）：用 `std::process::Command` 调用 `ip` 命令。虽然违反项目"零外部依赖"原则，但 `ip` 在所有主流 Linux 发行版中预装，且实现最快。

**后续优化为方案 C**：通过 D-Bus 调用 NetworkManager，不依赖外部命令，且不需要 root 权限（通过 PolicyKit 授权）。

**预估工作量：** 2-3 天（方案 A 1 天，方案 C 额外 2 天）

**难度：** ⭐⭐ 低（方案 A）/ ⭐⭐⭐ 中等（方案 C）

---

## 三、Soft Degradation 清单（功能降级但不阻塞）

### 3.1 WiFi 详情 — `utils/wlan.rs::query()`

**现状：** 非 Windows 返回 `None`，链路质量工具不显示无线信息。

**Linux 实现方案：**
- `ioctl(SIOCGIWESSID)` 获取 SSID
- `ioctl(SIOCGIWSTATS)` 获取信号质量
- `/proc/net/wireless` 读取信号级别
- 或通过 `wpa_supplicant` D-Bus 接口

**预估工作量：** 2-3 天

**难度：** ⭐⭐⭐ 中等 — Linux 无线 API 分散且碎片化。

### 3.2 MAC 地址解析 — `utils/net.rs::resolve_mac_address()`

**现状：** 非 Windows 无实现，扫描器不显示 MAC。

**Linux 实现方案：** 发 ARP 请求（原始套接字）或读 `/proc/net/arp` 缓存。

**预估工作量：** 0.5-1 天

**难度：** ⭐ 低

### 3.3 有线链路速率 — `utils/net.rs::link_speed_for_guid()`

**现状：** 非 Windows 返回 `None`。

**Linux 实现方案：** 读 `/sys/class/net/<name>/speed`。

**预估工作量：** 0.5 天

**难度：** ⭐ 低

### 3.4 系统代理检测 — `dashboard.rs::detect_proxy()`

**现状：** 非 Windows 跳过注册表读取，仅检查环境变量。

**Linux 实现方案：** 检查 `gsettings get org.gnome.system.proxy`（GNOME）或 KDE 等效命令。环境变量已经是合理的回退。

**预估工作量：** 0.5 天

**难度：** ⭐ 低

### 3.5 DHCP 状态检测

**现状：** `get_interfaces()` 为空导致不可用。

**Linux 实现方案：** 在实现网卡枚举时一并解决（读 NetworkManager 状态或 `/run/systemd/netif/leases/`）。

**预估工作量：** 包含在 2.1 中

---

## 四、新增依赖评估

| Crate | 用途 | 是否必需 | 跨平台 | 体积影响 |
|-------|------|---------|--------|---------|
| `surge-ping` | ICMP echo | 已有 | ✅ | 已计入 |
| `nix` | `ioctl`, `getifaddrs` 等 Unix API | 推荐 | Unix only | 小 |
| `netlink-packet-route` | Netlink 网络配置 | 可选 | Linux only | 中 |
| `zbus` | D-Bus（NetworkManager） | 可选 | Linux/Unix | 中 |

**推荐最小依赖集：** `nix`（已有部分被 `surge-ping` 间接依赖），无需引入大型新 crate。

---

## 五、分阶段实施计划

### Phase 1：基础可用（5-7 天）

**目标：** Linux 下适配器列表、Ping、路由跟踪、链路质量（连通性维度）可用。

| 任务 | 文件 | 工作量 |
|------|------|--------|
| `get_interfaces()` Linux 实现 | `utils/net.rs` | 3-5 天 |
| `icmp::echo_once()` Linux 实现 | `diagnostics/icmp.rs` | 1-2 天 |
| `trace.rs` 移除 stub | `diagnostics/trace.rs` | 0.5 天 |
| `link_quality.rs` 移除 stub | `diagnostics/link_quality.rs` | 0.5 天 |

**Phase 1 完成后 Linux 可用功能：**
- ✅ Dashboard（本地网络信息）
- ✅ Adapter（列表 + 查看，暂不能编辑）
- ✅ Scanner（无 MAC）
- ✅ Traffic
- ✅ Ping
- ✅ Trace
- ✅ Port Scan
- ✅ Link Quality（无无线详情）
- ✅ Public Speed
- ✅ LAN Speed

### Phase 2：功能完善（3-5 天）

**目标：** 补全 Soft Degradation 项。

| 任务 | 文件 | 工作量 |
|------|------|--------|
| IP 配置写入（`ip` 命令） | `utils/ipconfig.rs` | 1 天 |
| MAC 地址解析 | `utils/net.rs` | 0.5 天 |
| 有线链路速率 | `utils/net.rs` | 0.5 天 |
| WiFi 详情查询 | `utils/wlan.rs` | 2-3 天 |

### Phase 3：CI/CD 与打包（1-2 天）

| 任务 | 文件 | 工作量 |
|------|------|--------|
| GitHub Actions 添加 Linux 构建 | `.github/workflows/release.yml` | 0.5 天 |
| 添加 Linux x86_64 + aarch64 产物 | `.github/workflows/release.yml` | 0.5 天 |
| README 更新 Linux 安装说明 | `README.md` | 0.5 天 |

---

## 六、风险与注意事项

1. **权限问题：** ICMP 和网络配置在 Linux 下需要 root 或 `CAP_NET_RAW`/`CAP_NET_ADMIN` 能力。需在文档中说明，或考虑 `setcap` 方案。
2. **发行版碎片化：** 网络管理方式差异大（NetworkManager / systemd-networkd / netplan / 手动配置）。Phase 1 优先支持读取 `/sys/class/net/`（通用），Phase 2 优先支持 NetworkManager（最广泛）。
3. **`surge-ping` 异步桥接：** `icmp.rs` 的同步调用模式需要小心处理 tokio runtime 嵌套问题。
4. **GUID 兼容性：** Windows 用 GUID 标识网卡，Linux 无此概念。需要改用接口名（如 `eth0`、`wlan0`）作为唯一标识，这会影响持久化配置的兼容性（`config.json` 中按 GUID 保存的适配器参数）。
5. **测试环境：** 需要 Linux 真机或 VM 测试，CI 仅能验证编译。

---

## 七、结论

| 维度 | 评估 |
|------|------|
| **总工作量** | Phase 1: 5-7 天 + Phase 2: 3-5 天 + Phase 3: 1-2 天 = **9-14 人天** |
| **技术难度** | ⭐⭐⭐ 中等 — 主要挑战是网卡枚举和 ICMP 异步桥接 |
| **风险等级** | 中 — 发行版碎片化 + 权限模型差异 |
| **最小可用版本** | Phase 1 完成后即可发布 Linux 版本（6/6 诊断工具可用 + 基础适配器/扫描） |
| **推荐优先级** | Phase 1 > Phase 3 > Phase 2 |

**关键结论：** 项目已有良好的跨平台骨架（`cfg` 分支、`surge-ping` 依赖、纯逻辑跨平台模块），核心瓶颈仅两处：网卡枚举（`get_interfaces`）和 ICMP 原语（`icmp.rs`）。解决这两处即可解锁 80% 的 Linux 功能。
