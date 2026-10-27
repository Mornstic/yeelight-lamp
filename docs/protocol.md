# 设备协议笔记

Yeelight 智能屏幕挂灯 2（Yeelight Tube Plus 2），型号 `yeelink.light.lamp43`，MIoT 规格
`urn:miot-spec-v2:device:light:0000A001:yeelink-lamp43:1:0000C802`。

## 网络

- 挂灯只支持 2.4GHz Wi-Fi，且无法完成 WPA2-Enterprise（校园网）认证。
- 电脑开 Windows 移动热点（2.4GHz / WPA2），电脑本体可同时连接 5GHz 校园网。
- ICS 固定网段 `192.168.137.0/24`，网关 `192.168.137.1`；设备 IP 由 ICS DHCP 分配，会变化。
- 本地控制不需要热点有外网；但 Windows 创建热点需要一个联网连接配置。
- Yeelight 旧版局域网协议（TCP 55443）不可用，端口全关，也没有 SSDP。

## miIO

- UDP 54321。握手包 32 字节：`21310020` + 28 字节 `ff`；响应第 8–12 字节为 DID（大端）。
  握手不需要 token，但加密控制必须 token。
- 报文结构：16 字节头 + 16 字节 MD5 校验 + AES-128-CBC/PKCS7 密文。
  - 头：magic `0x2131`、总长度（密文长度 + 32）、4 字节保留、4 字节设备 ID、4 字节 stamp。
  - 校验：`MD5(header ‖ token ‖ 密文)`；握手包（长度 32）的校验字段不校验。
  - 密钥不是 token 本身：`key = MD5(token)`，`iv = MD5(key ‖ token)`，
    与 python-miio 的 `Utils.key_iv` 一致（已有已知答案测试锁定）。
  - 明文为 JSON + `\0`，解析时去掉尾部零字节。
- stamp 取设备上次响应头里的时间戳 +1（设备时间戳是开机秒数，不是 Unix 时间）；
  指令 ID 从 1 递增，到 9999 回到 1。
- 指令格式：`{"id": n, "method": "get_properties", "params": [{"did": "...", "siid": 2, "piid": 1}]}`，
  多属性读取一次下发，返回数组按请求顺序排列，每项含 `code`（0 为成功）与 `value`。

## 属性表

| siid | piid | 含义 | 取值 |
|---|---|---|---|
| 2 | 1 | 开关 | bool |
| 2 | 2 | 亮度 | 1–100，步进 1 |
| 2 | 3 | 色温 | 2700–6500K，步进 1 |
| 2 | 7 | 模式 | 0 空场景 / 1 办公 / 2 专注 / 3 阅读 / 4 休闲 |
| 2 | 9 | 上电默认状态 | 0 断电记忆 / 1 上电打开 / 2 上电关闭 |
| 4 | 1 | 自动补光 | bool |

动作 aiid 1–5 依次为 toggle、亮度 +、亮度 −、色温 +、色温 −。

## 实测行为

- 模式是预设组合：办公 100%/6000K、专注 80%/5000K、阅读 100%/4000K、休闲 30%/2700K。
- 手动写亮度或色温后，设备把 mode 归 0（自定义），UI 必须反映这一点。
- 关灯状态下写亮度/色温不会生效，也不报错；需要先开灯。
- 没有过渡时间、环境光、屏幕联动等属性；平滑效果只能由客户端连续写值模拟。

## 设备发现

按可靠性排序：配置里的 IP → ARP 表按 MAC 匹配 → miIO UDP 广播（按 DID 过滤）。
找到新 IP 后回写配置。

## Token 获取

小米密码登录会被风险控制要求图形验证码（87001）。可行路径是复用浏览器登录态：

1. 用 Edge 登录小米账号，取 `passToken`、`userId` 等 cookie。
2. `GET https://account.xiaomi.com/pass/serviceLogin?sid=xiaomiio&_json=true` 携带这些 cookie，
   直接返回 `ssecurity` 与 `location`。
3. `GET location` 后获得 `serviceToken`。
4. 用 `ssecurity`/`serviceToken` 调用小米云设备列表接口，读取该设备的 16 字节 token。

实现见 `tools/py/get_token.py`。token 只保存在 `%APPDATA%\yeelight-lamp\config.json`，
不得写入仓库或日志。
