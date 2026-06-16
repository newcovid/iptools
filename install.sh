#!/usr/bin/env sh
# iptools Linux 安装脚本
# 作用：给二进制授予 CAP_NET_RAW，使「局域网扫描 / Ping / Trace / 链路质量」免 sudo 即可运行。
#
# 用法：
#   sudo ./install.sh                 # 给同目录的 ./iptools 授权
#   sudo ./install.sh /path/to/iptools# 给指定路径的二进制授权
#   sudo ./install.sh --system        # 额外复制到 /usr/local/bin（之后可全局 `iptools` 运行）
#
# 说明：CAP_NET_RAW 是「创建原始套接字」的最小权限（拼发 ARP / 收发原始 ICMP）。
#       比 `sudo 运行整个程序` 更安全——只授予这一项能力，程序仍以普通用户身份运行。

set -eu

SYSTEM=0
BIN=""
for arg in "$@"; do
    case "$arg" in
        --system) SYSTEM=1 ;;
        *) BIN="$arg" ;;
    esac
done

# 默认取脚本所在目录的 ./iptools
if [ -z "$BIN" ]; then
    BIN="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/iptools"
fi

if [ ! -f "$BIN" ]; then
    echo "✗ 找不到二进制：$BIN"
    echo "  用法：sudo ./install.sh [iptools 路径] [--system]"
    exit 1
fi

if [ "$(id -u)" -ne 0 ]; then
    echo "✗ 需要 root 权限，请用：sudo ./install.sh"
    exit 1
fi

if ! command -v setcap >/dev/null 2>&1; then
    echo "✗ 未找到 setcap，请先安装：apt install -y libcap2-bin"
    echo "  （或不安装，改用 sudo 运行 iptools）"
    exit 1
fi

setcap cap_net_raw+ep "$BIN"
echo "✓ 已授予 CAP_NET_RAW：$BIN"

if [ "$SYSTEM" -eq 1 ]; then
    install -m 0755 "$BIN" /usr/local/bin/iptools
    setcap cap_net_raw+ep /usr/local/bin/iptools
    echo "✓ 已安装到 /usr/local/bin/iptools —— 现在可直接运行：iptools"
else
    echo "  现在可免 sudo 运行：$BIN"
fi

echo
echo "其它可选项："
echo "  · Wi-Fi 详情需安装 iw：           sudo apt install -y iw"
echo "  · 改 IP 配置仍需 sudo / PolicyKit（NetworkManager/netplan 后端）"
