## 改动内容

<!-- 说明改了什么，以及为什么需要这个改动。 -->

## 验证

- [ ] `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`
- [ ] `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`
- [ ] `cargo test --manifest-path src-tauri/Cargo.toml`
- [ ] 如有 UI 改动，已在本机实际启动并检查
- [ ] 如有设备行为改动，已用真实挂灯验证

## 安全确认

- [ ] 没有提交 token、`config.json`、`session.json`、`browser_profile/` 或构建产物
- [ ] 截图和日志已经打码

## 关联 Issue

<!-- 例如：Closes #12 -->
