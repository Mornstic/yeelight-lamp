# 安全策略

## 支持版本

| 版本 | 支持安全更新 |
| --- | --- |
| 0.1.x | 是 |
| 更早版本 | 否 |

## 报告漏洞

请使用 GitHub Security 页面的 **Report a vulnerability** 私下报告：

<https://github.com/Mornstic/yeelight-lamp/security/advisories/new>

不要在公开 Issue、讨论或截图中披露未修复的漏洞。

报告中请尽量包含：

- 受影响的版本和 Windows 版本
- 复现步骤、影响范围和建议的修复方向
- 是否与设备 token、局域网协议、安装包或 WebView 相关

请使用占位符代替真实凭据。**不要提交 `config.json`、`session.json`、设备 token、小米账号密码或 `passToken` / `ssecurity`。**

## 安全边界

- 设备 token 只保存在本机 `%APPDATA%\yeelight-lamp\config.json`，不会上传到云端。
- 日常控制通过局域网 miIO（UDP 54321）完成，不使用 Yeelight 云；局域网本身的隔离和 Wi-Fi 加密由用户网络负责。
- 安装包当前未做代码签名，Windows SmartScreen 可能提示「未知发布者」。请从本仓库 Releases 下载，并用 `SHA256SUMS.txt` 校验。
- 本项目不提供账号常驻登录、遥测或远程控制能力。

## 处理流程

维护者会尽快确认报告，评估影响并准备修复；在修复发布前，请给维护者合理的处理时间。
修复完成后，会在 Release Notes 或 GitHub Security Advisory 中说明影响范围。
