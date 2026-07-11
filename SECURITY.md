# 安全策略

## 支持范围

安全修复面向最新 GitHub Release 和 `main` 分支。旧版本通常不会单独回补，请先确认问题在最新版本中仍可复现。

## 报告漏洞

请通过仓库的 [GitHub Security Advisories](https://github.com/newcovid/iptools/security/advisories/new) 私下报告安全问题，不要创建公开 Issue。

报告中请包含受影响版本和平台、复现步骤、潜在影响，以及可行的缓解建议。维护者会在确认问题后协调修复和披露时间。

## 高权限功能

iptools 的原始套接字探测和网络配置功能可能需要管理员权限、root、PolicyKit 或 `CAP_NET_RAW`。请只从官方 Releases 下载二进制，并在确认目标网卡与参数后执行网络配置写入。
