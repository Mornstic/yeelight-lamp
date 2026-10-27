# 贡献指南

感谢你愿意改进 Yeelight 屏幕挂灯。这个项目由个人维护，欢迎高质量的 Issue 和 Pull Request。

## 报告问题

- 先阅读 README 的「常见问题」，并搜索已有 Issue。
- 使用 Bug 报告模板，附上 Windows 版本、程序版本、安装方式、灯型号和网络环境。
- 涉及设备行为时，说明灯是直连路由器还是连接 Windows 移动热点，以及是否同一网段。
- **不要上传设备 token、`config.json`、`session.json`、`browser_profile/` 或未打码的登录截图。**
- 安全漏洞请不要开公开 Issue，按 [SECURITY.md](SECURITY.md) 私下报告。

## 开发环境

- Windows 10 / 11
- Rust stable（MSVC 工具链）
- Visual Studio C++ 生成工具
- Node.js 20+
- WebView2 运行时（Windows 11 已自带）

```powershell
npm install
npm run tauri dev
```

设备 token 由 `tools/py/get_token.py` 写入本机，详见 README 的「首次连接」。
`src-tauri` 里的 `lampctl` 可以在终端直接验证设备行为：

```powershell
cargo run --manifest-path src-tauri/Cargo.toml --bin lampctl -- status
```

## 代码约定

- Rust 使用 `cargo fmt` 格式化，并通过 `cargo clippy -- -D warnings` 与 `cargo test`。
- 前端保持无框架、无打包器，`ui/index.html` 直接由 Tauri 嵌入；如需引入构建链，请先开 Issue 讨论。
- 设备 siid / piid、握手和加密逻辑以 `docs/protocol.md` 为准；修改协议层时请补充测试。
- 项目不引入云端依赖、遥测或账号常驻登录。设备控制必须在局域网内完成。
- 新增依赖需要说明用途，优先使用 Rust 标准库或已有依赖。

## 提交 Pull Request

1. 从 `main` 切出主题分支。
2. 一个 PR 只处理一件事，提交信息使用 `feat:`、`fix:`、`docs:`、`refactor:` 或 `chore:` 前缀。
3. 在 PR 中写清改动原因、验证方式和影响的设备/系统范围。
4. 涉及 UI 的改动请附截图；涉及设备的改动请说明实灯验证结果。
5. 确认没有提交 token、配置、会话文件或构建产物。

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

## 适配新设备

当前只正式支持 `yeelink.light.lamp43`（Yeelight 智能屏幕挂灯 2 / Tube Plus2）。
适配其它型号前请先开 Issue，说明型号、固件版本以及可用的 siid / piid 信息；
不要直接在 PR 中猜测未知设备的参数。
