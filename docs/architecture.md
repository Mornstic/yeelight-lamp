# Yeelight 屏幕挂灯 · 架构与实现详解

> 这份文档解释这个托盘应用是怎么工作的：用了什么语言、每一层代码负责什么、一次点击如何变成灯体上的动作，以及 miIO / MIoT / 小米云 token 在其中扮演什么角色。
>
> 目标不是复述代码，而是让你能回答三个问题：**为什么这样设计、数据怎么流动、出问题时该看哪里。**
>
> 适用版本：v0.1.1（2026-10-28）

---

## 目录

1. [产品边界：它做什么，不做什么](#1-产品边界它做什么不做什么)
2. [技术选型：为什么是 Rust + Tauri + 手写前端](#2-技术选型为什么是-rust--tauri--手写前端)
3. [全景架构：一个进程里的五层](#3-全景架构一个进程里的五层)
4. [一次点击的完整链路](#4-一次点击的完整链路)
5. [miIO：逐字节拆解局域网协议](#5-miio逐字节拆解局域网协议)
6. [MIoT：siid / piid 与这盏灯的能力](#6-miotsiid--piid-与这盏灯的能力)
7. [Token 与小米云：一次性初始化](#7-token-与小米云一次性初始化)
8. [后端模块导读](#8-后端模块导读)
9. [前端：单文件面板如何工作](#9-前端单文件面板如何工作)
10. [托盘、窗口、主题与快捷键](#10-托盘窗口主题与快捷键)
11. [设计系统与图标生成](#11-设计系统与图标生成)
12. [构建、测试与发布](#12-构建测试与发布)
13. [安全与隐私设计](#13-安全与隐私设计)
14. [已知限制与下一步](#14-已知限制与下一步)
15. [如何阅读这份代码：路线与练习](#15-如何阅读这份代码路线与练习)
16. [术语表与常见问题](#16-术语表与常见问题)
17. [开发历程回顾](#17-开发历程回顾)

---

## 1. 产品边界：它做什么，不做什么

一句话：**一个 Windows 托盘小工具，通过局域网直接控制 Yeelight 智能屏幕挂灯 2（Tube Plus2，型号 `yeelink.light.lamp43`），日常使用不依赖手机、账号或云端。**

它做的事：

- 左键托盘图标打开/收起玻璃面板；右键菜单提供显示面板、开关灯、自动补光、退出
- 开关、亮度 1–100%、色温 2700–6500K、四档预设、自动补光
- 托盘图标三态（开灯 / 关灯 / 离线），并跟随深色、浅色任务栏切换
- 全局快捷键（默认关闭）、开机自启、单实例、断线重连、设备换 IP 自动发现
- 全部控制指令直接发给灯体，运行时没有云请求、没有遥测

它刻意不做的事：

- 不支持其它灯型号（其它型号的 siid / piid 可能不同）
- 不支持 Yeelight 旧版 TCP 55443 局域网协议（这盏灯没有开放该端口，也没有 SSDP）
- 不做远程控制，不依赖米家 App 或 Yeelight 云常驻
- 不提供固件升级、灯光动画编排、屏幕内容联动

产品边界决定了很多设计：因为只服务一盏灯、一个面板，才敢用手写单文件前端；因为要求局域网可用，才必须自己实现 miIO 协议；因为设备只在 2.4GHz 普通 WPA2/WPA3 网络下工作，才需要电脑开移动热点给灯用。

---

## 2. 技术选型：为什么是 Rust + Tauri + 手写前端

### 2.1 一张表看清技术栈

| 层 | 技术 | 作用 |
|---|---|---|
| 桌面壳 | **Rust + Tauri 2** | 托盘、窗口、单实例、自启动、全局快捷键、打包 |
| 异步运行时 | **Tokio** | UDP 收发、超时、状态轮询、命令并发 |
| 加密 | `aes` + `cbc` + `md-5` | miIO 的 AES-128-CBC 与 MD5 校验 |
| 序列化 | `serde` / `serde_json` | 配置、设置、miIO JSON 报文 |
| 错误 | `thiserror` | 分层错误类型与中文错误信息 |
| 前端 | **原生 HTML / CSS / JavaScript** | 玻璃面板、设置页、交互与动画 |
| 渲染 | **WebView2** | Tauri 在 Windows 上使用的系统 WebView |
| 初始化脚本 | **Python 3.12** | 一次性获取 token（Playwright + micloud + python-miio） |
| 图标 / 素材 | **Python + Pillow** | 应用图标、托盘图标、安装器素材、社交预览图 |
| 打包 | **NSIS（Tauri bundler）** | 当前用户安装包，无需管理员权限 |
| CI | **GitHub Actions + CodeQL** | 格式、Clippy、测试、静态分析 |

### 2.2 为什么不是 Electron / Python 桌面

这个工具的核心诉求是“安静常驻”：

- 安装包要小、内存占用要低、开机自启不能拖慢系统
- 需要原生托盘、原生窗口定位、全局快捷键
- 网络与加密逻辑要稳定，且只有一盏灯、流量极低

**Electron** 会捆绑一整个 Chromium，安装包和内存都明显更重；**Python 桌面**则要解决解释器分发、托盘库、打包和杀毒误报。Tauri 用系统自带的 WebView2 渲染前端，Rust 后端编译成单个 exe，最终安装包约 2.5 MB，常驻内存也远小于 Electron 方案。

代价是：Windows 10 需要 WebView2 运行时（Windows 11 已自带），以及 Rust 的学习曲线。对这个产品来说，收益大于代价。

### 2.3 为什么前端不用 React / Vue

整个界面只有一个面板 + 一个设置页，没有路由、没有列表、没有复杂表单。手写 HTML / CSS / JS 的好处：

- 没有构建器、没有依赖树，`ui/index.html` 就是最终产物
- 启动即渲染，不需要等待 JS 框架初始化
- 动画和玻璃质感可以直接用 CSS 控制，调试路径短
- 代码量可控（约 1900 行，含样式），适合一个人长期维护

代价是：所有状态同步、事件绑定都要自己写；预设参数在 Rust 和 JS 各有一份。这些代价在“单屏应用”里是可接受的，后文会具体讲。

### 2.4 为什么还要 Python

Python 只做两件一次性的事：

1. **取 token**：小米账号登录、换票、拉设备列表，这部分生态成熟（`micloud`、`python-miio`、Playwright），用 Python 写几十行就能完成；用 Rust 重写反而增加维护面。
2. **生成图片素材**：Pillow 处理超采样、渐变、圆角、字体渲染比手写 Rust 图像代码更直接。

也就是说：**Python 是开发期/初始化期工具，不进入最终程序**。用户装好并写入 token 后，日常运行完全不碰 Python。

---

## 3. 全景架构：一个进程里的五层

```text
┌────────────────────────── yeelight-lamp.exe ──────────────────────────┐
│                                                                        │
│  ┌─────────────────────────────┐        invoke / event                │
│  │ WebView2: ui/index.html     │ ◄──────────────────────────────┐     │
│  │  · 玻璃面板 / 设置页         │                                │     │
│  │  · 乐观更新 / 写入队列       │ ──────────────────────────────► │     │
│  └─────────────────────────────┘        #[tauri::command]       │     │
│                                                                  │     │
│  ┌──────────────────────────────────────────────────────────────┴──┐  │
│  │ app.rs：Tauri 命令、托盘、菜单、轮询、单实例、自启动             │  │
│  │                                                                │  │
│  │   AppState { Lamp, Snapshot, Settings }  ← Arc<Mutex<...>>     │  │
│  └───────────────┬────────────────────────────────────────────────┘  │
│                  │                                                    │
│  ┌───────────────▼──────────────┐   ┌──────────────────────────────┐  │
│  │ device.rs：灯的能力与发现     │   │ hotkeys.rs：全局快捷键         │  │
│  │  · 开关/亮度/色温/模式/补光   │   │ window.rs：面板显示/隐藏       │  │
│  │  · IP 解析、DID 校验          │   │ theme.rs：任务栏明暗          │  │
│  └───────────────┬──────────────┘   │ settings.rs：设置持久化       │  │
│                  │                  │ config.rs：token 配置         │  │
│  ┌───────────────▼──────────────┐   └──────────────────────────────┘  │
│  │ miio.rs：miIO 协议客户端      │                                     │
│  │  · 握手 / AES / MD5 / UDP     │                                     │
│  └───────────────┬──────────────┘                                     │
└──────────────────┼────────────────────────────────────────────────────┘
                   │ UDP 54321（局域网）
           ┌───────▼────────┐
           │  Yeelight 挂灯  │
           └────────────────┘
```

### 3.1 线程与并发模型

- **主线程**：Tauri 事件循环、托盘、窗口、菜单。Tauri 要求部分 UI 操作在主线程执行，插件内部会做调度。
- **Tokio 运行时**：`poll_loop` 每 2.5 秒（在线）或 4 秒（离线）读取一次状态；命令处理、快捷键动作、重连都在异步任务里执行。
- **一份共享状态**：`Arc<Mutex<AppState>>`。所有设备操作（轮询、UI 命令、托盘菜单、快捷键）都要先拿到这把锁。这样同一时刻只有一条指令在跟灯通信，避免 UDP 会话的 stamp / id 被并发打乱。
- **锁的代价**：如果一次指令超时 1.6 秒，期间其它操作会排队。对“一盏灯、低频操作”的场景，这比引入连接池和复杂并发更简单可靠。UI 用乐观更新掩盖了大部分等待感。

### 3.2 启动过程

1. `main.rs` 调用 `yeelight_lamp::run()`，进入 `app.rs`。
2. 读取 `%APPDATA%\yeelight-lamp\settings.json`；读取 `config.json`（没有就进入“未配置”状态）。
3. 创建 Tauri 应用，注册插件：单实例、自启动、全局快捷键、日志。
4. `setup` 中定位面板窗口、创建托盘和菜单、按初始快照设置托盘图标。
5. 若不是开机自启（或设置要求显示），打开面板；开发模式下强制显示。
6. 启动轮询任务；异步同步快捷键和开机自启。
7. 轮询成功后，快照变为 `online`，托盘图标、tooltip、面板同步更新。

### 3.3 代码地图

| 文件 | 职责 | 你可以从这里开始读 |
|---|---|---|
| `src-tauri/src/main.rs` | Windows 入口，隐藏控制台 | 5 行 |
| `src-tauri/src/lib.rs` | 模块导出 | 12 行 |
| `src-tauri/src/app.rs` | 状态、命令、托盘、轮询、自启动 | `AppState`、`apply_request` |
| `src-tauri/src/device.rs` | 灯语义层：属性、预设、发现 | `Lamp::connect`、`status` |
| `src-tauri/src/miio.rs` | miIO 协议与加解密 | `MiioClient::exchange` |
| `src-tauri/src/config.rs` | token 配置读取、IP 回写 | `Config::parse` |
| `src-tauri/src/settings.rs` | 快捷键/自启动设置 | `Settings` |
| `src-tauri/src/window.rs` | 窗口定位、开合动画握手 | `close` / `toggle` |
| `src-tauri/src/hotkeys.rs` | 快捷键注册与映射 | `sync` / `code_for` |
| `src-tauri/src/theme.rs` | 任务栏明暗探测 | `taskbar_is_light` |
| `src-tauri/src/bin/lampctl.rs` | 命令行调试工具 | `run_lamp` |
| `ui/index.html` | 全部前端 | `render` / `writer` / 事件绑定 |
| `tools/py/get_token.py` | 一次性 token 获取 | `exchange` / `main` |
| `tools/gen_icons.py` | 应用与托盘图标 | `tray_icon` |

---

## 4. 一次点击的完整链路

### 4.1 打开面板

```text
用户左键托盘图标
  → Tauri TrayIconEvent::Click（app.rs）
  → window::toggle(&window)
  → 窗口当前不可见 → window::show()
      · 取消关闭状态、递增 epoch
      · 按鼠标所在显示器的工作区右下角定位（16dp 边距）
      · window.show() + set_focus()
      · emit("panel-show")
  → 前端 listen("panel-show") → animateIn()
      · 移除 is-leaving / is-entering，强制 reflow
      · 添加 is-entering → CSS panel-in 0.32s 播放
```

关闭时是反向握手：

```text
托盘再次点击 / 按 Esc / 点击面板外 / 点窗口关闭
  → window::close()
      · 若正在关闭则直接返回（幂等）
      · CLOSING = true，递增 epoch
      · emit("panel-hide")
      · 启动 450ms 兜底线程：若 epoch 没变，直接 hide()
  → 前端 animateOut()
      · 添加 is-leaving → CSS panel-out 0.18s
      · animationend 或 280ms 超时后 invoke("hide_panel")
  → window::finish_hide()
      · 递增 epoch、CLOSING = false、window.hide()
```

这套状态机解决了两个实际问题：

- **第二次点击不生效**：早期实现只判断 `is_visible()`，而窗口在退场动画期间仍然可见，导致点击被当成“打开”。现在 `CLOSING` 和 `epoch` 把“可见”和“正在关闭”分开处理。
- **WebView 忙时窗口残留**：前端动画回调不一定可靠，后端 450ms 兜底保证窗口一定会隐藏；`epoch` 保证兜底不会误伤刚刚重新打开的面板。

### 4.2 点按台灯区域开关灯

```text
stage click（ui/index.html）
  → 乐观更新：root.dataset.power = "on"，按钮态立即变化
  → invoke("set_power", { on: true })
  → #[tauri::command] set_power（app.rs）
  → apply_request(state, Request::SetPower(true))
      · 拿 AppState 锁
      · 若当前显示 connecting，先 emit 状态
      · lamp.set_power(true)
          → Lamp::call → MiioClient::send
              · 未握手 → hello()，记录 DID / stamp
              · 组包、AES 加密、MD5 校验、UDP 发送、等待 1.6s
              · 瞬时错误 → 重新握手再试一次
          → get_properties 的写响应 code == 0
      · 成功：snapshot.connection = online，power = true
      · 失败：snapshot.mark_offline(error)
  → publish()
      · emit("lamp-state", snapshot)
      · 更新托盘图标、tooltip、菜单文案
  → 前端 render(snapshot)
      · 以服务端快照为准，修正视觉状态
```

注意乐观更新和后端快照的关系：**前端负责“立即有反馈”，后端负责“最终正确”**。如果设备离线，下一次 `lamp-state` 会把 UI 拉回真实状态，并显示“设备离线 · 点按重连”。

### 4.3 拖动亮度/色温：写入队列

滑杆的 `input` 事件触发频率远高于设备能处理的频率。前端不是每个事件都发一条指令，而是用 `writer`：

```js
function writer(send) {
  let pending = null;
  let busy = false;
  async function drain() {
    busy = true;
    while (pending !== null) {
      const value = pending;
      pending = null;
      await send(value);                    // 发一条
      await new Promise(r => setTimeout(r, 40)); // 给设备喘息
    }
    busy = false;
  }
  return (value) => {
    pending = value;   // 永远只保留最新值
    if (!busy) drain();
  };
}
```

效果是：用户快速拖动时，中间的旧值会被丢弃，只按设备能承受的节奏发送最新值；松手后 UI 不会被“慢半拍”的旧响应拉回去。后端还有一层：手动调亮度/色温时，快照里的 `mode` 置 0（自定义）、`auto_fill` 置 false，因为固件的行为就是这样。

另外，`render` 里有一个 1200ms 的保护窗口：

```js
if (Date.now() - lastBrightnessEdit > 1200) { ...同步滑杆... }
if (Date.now() - lastTempEdit > 1200) { ...同步滑杆... }
```

轮询返回的状态可能比用户最后一次拖动更旧，这个窗口避免滑杆在拖动过程中被“拽回去”。

### 4.4 轮询与离线判断

`poll_loop` 的逻辑很简单但很关键：

```text
循环：
  拿锁 → 若还没有 Lamp，尝试重新加载 config
  → status_with_latency()：一条 get_properties 读 5 个属性
  → 成功：connection=online、记录延迟、更新快照
  → 失败：connection=offline、清除延迟、记录错误
  → emit("lamp-state") + 更新托盘
  → 在线睡 2.5s，离线睡 4s
```

- 在线时 2.5 秒一次，保证托盘图标、面板数值能跟上灯体被其它途径改变的状态。
- 离线时降到 4 秒，减少无效流量。
- 每次成功读取都会测往返耗时，面板上显示的 `· 12 ms` 就来自这里。
- 没有用长连接或推送，因为 miIO 这盏灯没有可订阅的状态通道；轮询是最稳的方案。

---

## 5. miIO：逐字节拆解局域网协议

### 5.1 它是什么

miIO 是小米生态设备的局域网 UDP 协议，端口 **54321**。很多工具（python-miio、Home Assistant 集成）都用它。它和“米家 App”是两回事：App 是控制界面，miIO 是设备在局域网里的通信方式。

这盏灯的情况：

- 只支持 2.4GHz Wi-Fi，不能完成 WPA2-Enterprise（校园网/企业网 802.1X）
- 旧版 Yeelight 局域网协议（TCP 55443）不可用，端口关闭，也没有 SSDP
- 必须先拿到设备 token，才能发加密指令；握手本身不需要 token

### 5.2 网络拓扑

推荐用法是电脑开 Windows 移动热点给灯用：

- 热点固定为 2.4GHz / WPA2，电脑本身可以继续连原来的 5GHz 网络
- Windows ICS 的网段固定为 `192.168.137.0/24`，网关 `192.168.137.1`
- 灯的 IP 由 ICS 的 DHCP 分配，可能变化；所以程序不能只依赖配置里的 IP
- 热点没有外网也能控制灯，但 Windows 创建热点需要一个“已联网”的连接配置

### 5.3 握手包

握手是一个 32 字节的 UDP 包：

```text
21310020 ffffffff ffffffff ffffffff ffffffff ffffffff ffffffff ffffffff
└─ magic ─┘ └────────────── 28 字节 0xff ──────────────┘
```

设备响应同样是 32 字节：

| 偏移 | 长度 | 含义 |
|---:|---:|---|
| 0–1 | 2 | magic `0x2131` |
| 2–3 | 2 | 总长度（握手为 32） |
| 4–7 | 4 | 保留 |
| 8–11 | 4 | 设备 ID（DID，大端） |
| 12–15 | 4 | 设备时间戳 stamp |

握手不需要 token，但可以用来：

1. 判断某个 IP 上是不是我们要的灯（DID 是否匹配）
2. 获取设备当前时间戳，作为后续加密报文的 stamp 基准
3. 广播发现局域网内的 miIO 设备

### 5.4 加密报文结构

```text
┌────────────── 16 字节头 ──────────────┬──── 16 字节 ────┬───── N 字节密文 ─────┐
│ magic │ length │ reserved │ did │ stamp │ MD5 校验和      │ AES-128-CBC/PKCS7    │
└────────────────────────────────────────┴─────────────────┴──────────────────────┘
```

- `magic`：`0x2131`
- `length`：密文长度 + 32（头 16 + 校验 16）
- `reserved`：4 字节，当前为 0
- `did`：设备 ID
- `stamp`：通常取设备上次响应里的 stamp + 1
- 校验：`MD5(header ‖ token ‖ ciphertext)`

密钥不是 token 本身，而是：

```text
key = MD5(token)
iv  = MD5(key ‖ token)
```

这与 python-miio 的 `Utils.key_iv` 一致。`miio.rs` 里有一个已知答案测试（`matches_python_miio_crypto`），用固定 token 和固定明文锁定 key、iv、密文，防止以后重构破坏互通性。

明文是 JSON 加一个 `\0` 结尾：

```json
{"id":1,"method":"get_properties","params":[{"did":"<DID>","siid":2,"piid":1}]}
```

接收端解密后要去掉尾部零字节再解析。指令 `id` 从 1 递增，到 9999 回到 1；`stamp` 每次从响应头更新。

### 5.5 请求与响应

读取多个属性：

```json
{
  "id": 12,
  "method": "get_properties",
  "params": [
    {"did": "<DID>", "siid": 2, "piid": 1},
    {"did": "<DID>", "siid": 2, "piid": 2},
    {"did": "<DID>", "siid": 2, "piid": 3},
    {"did": "<DID>", "siid": 2, "piid": 7},
    {"did": "<DID>", "siid": 4, "piid": 1}
  ]
}
```

响应（按请求顺序）：

```json
{
  "id": 12,
  "result": [
    {"code": 0, "value": true},
    {"code": 0, "value": 80},
    {"code": 0, "value": 4000},
    {"code": 0, "value": 3},
    {"code": 0, "value": false}
  ]
}
```

写入属性：

```json
{
  "id": 13,
  "method": "set_properties",
  "params": [
    {"did": "<DID>", "siid": 2, "piid": 2, "value": 65}
  ]
}
```

`code == 0` 表示成功。`device.rs` 会把非零 code 转成中文错误，避免 UI 直接暴露协议细节。

### 5.6 错误、超时与重试

`miio.rs` 把错误分成几类：

| 错误 | 含义 | 是否可重试 |
|---|---|---|
| `Io` | 绑定/发送/接收失败 | 是 |
| `Timeout` | 1.6 秒内没有响应 | 是 |
| `Malformed` | 包头、长度、magic 不对 | 是 |
| `Checksum` | MD5 校验失败，通常 token 不对 | 否 |
| `Payload` | 无法解密或 JSON 解析失败 | 是 |
| `Device(code)` | 设备明确返回错误码 | 否 |
| `InvalidToken` | token 不是 32 个十六进制字符 | 否 |

重试分两层：

1. `MiioClient::send` 遇到瞬时错误时，丢弃握手状态，重新 hello，再发一次。
2. `Lamp::call` 再包一层：如果连接不存在就 `connect()`，瞬时错误时重建客户端，最多两轮。

这样既能处理 Wi-Fi 抖动、设备刚上电，又不会对“token 错”“设备拒绝”这类确定性错误做无意义重试。

### 5.7 安全边界

miIO 的加密只保证“没有 token 的人看不懂报文、改不了报文”，它不是 TLS：

- token 是 16 字节设备密钥，等同于这盏灯的控制权
- 拿到 token 的同一局域网攻击者可以重放/构造指令
- 协议没有证书、没有云端身份校验
- 因此不要把 54321 暴露到公网，也不要在不可信网络里使用

对这个产品来说，信任边界是“你家的 2.4GHz 局域网 + 你的 Windows 账户”。

---

## 6. MIoT：siid / piid 与这盏灯的能力

### 6.1 规格

设备规格：

```text
urn:miot-spec-v2:device:light:0000A001:yeelink-lamp43:1:0000C802
```

MIoT 用 `siid`（service id）和 `piid`（property id）寻址。这盏灯用到的属性：

| siid | piid | 含义 | 取值 |
|---:|---:|---|---|
| 2 | 1 | 开关 | bool |
| 2 | 2 | 亮度 | 1–100，步进 1 |
| 2 | 3 | 色温 | 2700–6500K，步进 1 |
| 2 | 7 | 模式 | 0 自定义 / 1 办公 / 2 专注 / 3 阅读 / 4 休闲 |
| 2 | 9 | 上电默认状态 | 0 断电记忆 / 1 上电打开 / 2 上电关闭 |
| 4 | 1 | 自动补光 | bool |

动作 `aiid` 1–5 依次是 toggle、亮度 +、亮度 −、色温 +、色温 −。当前版本没有使用动作接口，而是直接读写属性，原因是属性接口更可预测、返回值更明确；动作更适合做无状态快捷键。

### 6.2 四个预设

| 预设 | mode | 亮度 | 色温 |
|---|---:|---:|---:|
| 办公 | 1 | 100% | 6000K |
| 专注 | 2 | 80% | 5000K |
| 阅读 | 3 | 100% | 4000K |
| 休闲 | 4 | 30% | 2700K |

这些是固件内置的组合。写 `piid 7` 切场景后，设备会同时应用亮度和色温。前端的预设表和后端的 `PRESETS` 保持一致；前端用它做即时视觉反馈，后端用它更新快照。

### 6.3 固件的三个“脾气”

1. **关灯状态下写亮度/色温会被丢弃，而且不报错。**
   所以 `apply_request` 在“当前是关灯 + 请求是亮度/色温/模式”时，会先 `set_power(true)`，再写目标值。命令行的 `lampctl light` 也走同样的 `apply_lighting` 逻辑。

2. **手动写亮度或色温后，设备把 `mode` 归 0（自定义）。**
   因此后端在成功写入后主动把快照的 `mode` 置 0、`auto_fill` 置 false，避免 UI 还显示“阅读”而实际已经是自定义。

3. **自动补光开启后，亮度和色温由灯自己决定。**
   所以 `set_auto_fill(true)` 成功后，后端会额外读一次 `status()`，把灯体真实值回填到快照，而不是相信用户之前拖的值。

### 6.4 没有的东西

这盏灯没有：

- 过渡时间（transition）属性：无法让固件自己做渐变，平滑效果只能由客户端连续写值模拟
- 环境光传感器读数、屏幕内容联动
- 独立的“夜灯”“阅读模式”之外的场景
- 可订阅的状态推送

这些限制决定了产品形态：数值以轮询为准，调光以“最新值 + 40ms 间隔”写入，不做花哨的灯光动画。

---

## 7. Token 与小米云：一次性初始化

### 7.1 token 是什么

设备 token 是 16 字节的密钥，通常以 32 个十六进制字符表示。它是 miIO 加密的核心：

- 没有 token：可以握手、可以发现设备，但不能发控制指令
- 有 token：可以解密/构造这盏灯的所有局域网指令

token 不是小米账号密码，也不是 `passToken`。它属于设备，保存在小米云端的设备列表里。

### 7.2 为什么需要小米云

这盏灯是 Yeelight 产品，但它的配网和设备信息托管在小米 IoT 体系里。要拿到 token，必须用**你自己的小米账号**查询**你自己账号下的设备列表**。这是一次性的初始化动作，不是日常依赖。

流程：

```text
Edge 登录小米账号（用户自己输入密码）
  → 浏览器 cookie：userId / passToken / cUserId / deviceId
  → GET https://account.xiaomi.com/pass/serviceLogin?sid=xiaomiio&_json=true
      带上 cookie，返回 ssecurity 与 location
  → GET location，获得 serviceToken
  → 用 userId / passToken / cUserId / ssecurity / serviceToken 调用小米云设备列表
  → 找到 model == yeelink.light.lamp43 的设备
  → 读取 name / did / token / ip / mac
  → 写入 %APPDATA%\yeelight-lamp\config.json
```

实现见 `tools/py/get_token.py`：

- `load_session()`：用 Playwright 启动持久化的 Edge 配置目录，登录后收集 cookie
- `exchange()`：`serviceLogin` 换 `ssecurity`，再拿 `serviceToken`
- `main()`：构造 `MiCloud` / `CloudInterface`，先查国区，找不到再查其它区域，按型号匹配，写入配置

### 7.3 为什么不用账号密码直接登录

小米账号的密码登录经常触发风险控制（错误码 87001，要求图形验证码/二次验证）。用浏览器复用登录态更稳：

- 密码只在小米官方页面里输入，脚本看不到、也不会保存
- 登录态由 Edge 管理，脚本只读取必要的 cookie
- 换票和拉设备列表仍走官方接口

### 7.4 敏感文件与边界

初始化过程会产生三类敏感数据：

| 文件/目录 | 内容 | 处理 |
|---|---|---|
| `config.json` | token、DID、IP、MAC | 程序运行必需；不进仓库、不进日志 |
| `session.json` | userId、passToken、cUserId 等 cookie | 换取 token 用；建议初始化成功后删除 |
| `browser_profile/` | Edge 持久化配置，含登录态 | 换取 token 用；建议初始化成功后删除 |

代码里的保护：

- `Token` 的 `Debug` 输出是 `Token(<redacted>)`
- `Config` 的 `Debug` 不打印 token
- `lampctl info` 只显示“token 已读取（长度 32，不在终端输出）”
- `get_token.py` 打印 token 时做前后 4 位掩码
- `.gitignore` 忽略 `config.json`、`session.json`、`browser_profile/`

必须遵守的规则：

- 不要把 `config.json`、`session.json`、完整 token 发到 Issue、聊天、截图或日志
- 不要把这几个文件提交到仓库
- 只在你自己拥有的小米账号和设备上使用这套脚本

### 7.5 token 泄漏了怎么办

token 等同于控制权。如果怀疑泄漏：

1. 在小米/米家体系里移除设备并重新绑定/配网（token 会变化）
2. 删除本机 `config.json`、`session.json`、`browser_profile/`
3. 重新运行 `get_token.py --login`

日常防护的最低要求是：Windows 账户有密码、不要共享 `%APPDATA%`、不要在不可信网络里开放热点给陌生设备。

### 7.6 “米家”与品牌的说明

- 协议和设备信息属于小米生态（miIO / MIoT / 小米云设备列表）
- 灯本身是 Yeelight 产品，界面品牌也始终是 **Yeelight**，不写成“米家”
- 运行时不需要米家 App；只有首次获取 token 时才访问小米账号服务

---

## 8. 后端模块导读

### 8.1 `miio.rs`：协议层

关键类型：

- `Token`：16 字节，`from_hex()` 校验 32 位十六进制，`key_iv()` 派生密钥
- `MiioClient`：保存 IP、token、DID、stamp、下一个 id、超时、是否已握手
- `HelloInfo`：一次握手的 IP、DID、stamp

关键方法：

- `hello(ip, wait)`：单播握手，900ms 超时（发现时 1400ms）
- `scan(window)`：向 `192.168.137.255` 和 `255.255.255.255` 广播握手包，按 DID 去重
- `handshake()`：记录 DID/stamp
- `send()`：未握手先握手；瞬时错误重建会话重试一次
- `exchange()`：组包、加密、校验、发送、验包、解密、解析

设计细节：

- 每次请求新建一个 UDP socket，不维护长连接；协议本身无连接，设备也无状态会话
- 响应必须校验 magic、声明长度、MD5，三者任一不对都拒绝解析
- 响应头里的 stamp 会写回客户端，下一条请求用 `stamp + 1`
- `id` 到 9999 回绕，避免设备端 id 溢出

### 8.2 `device.rs`：语义层

它把“miIO 属性”翻译成“灯的能力”：

- 常量：`SIID_LIGHT=2`、`PIID_POWER=1`、`PIID_BRIGHTNESS=2`、`PIID_COLOR_TEMP=3`、`PIID_MODE=7`、`SIID_CUSTOM=4`、`PIID_AUTO_FILL=1`
- `Status`：power / brightness / color_temp / mode / auto_fill
- `PRESETS`：四档场景的参数
- `Lamp`：持有 `Config` 和可选的 `MiioClient`

发现顺序（`resolve_ip`）：

1. 配置里的 IP：先单播 hello，DID 匹配就用
2. ARP 表：按配置里的 MAC 找 IP（DHCP 换 IP 后仍能找到）
3. UDP 广播：扫描热点网段和全网广播，按 DID 过滤

找到新 IP 后 `config.save_ip(ip)` 原子回写。`connect()` 握手后会再次校验 DID，不匹配返回 `WrongDevice`，防止误连到同一网段里的其它设备。

`status()` 一次 `get_properties` 读 5 个属性，要求至少 5 项、每项 `code == 0`，然后把值 clamp 到合法范围。`apply_lighting()` 和 `apply_preset()` 都遵循“先开灯再写”的固件规则。

### 8.3 `config.rs`：设备配置

- 默认路径 `%APPDATA%\yeelight-lamp\config.json`，可用 `YEELIGHT_LAMP_CONFIG` 覆盖（测试/隔离环境用）
- 解析 `name`、`model`、`did`、`token`、`ip`、`mac`；`did` 接受字符串或数字
- 保留原始 JSON（`raw`），只回写 `ip` 字段，不破坏用户可能添加的其它字段
- 写文件用 `config.json.tmp` + rename，避免写一半断电导致配置损坏
- `Debug` 输出不包含 token

### 8.4 `settings.rs`：应用设置

- 默认路径 `%APPDATA%\yeelight-lamp\settings.json`，可用 `YEELIGHT_LAMP_SETTINGS` 覆盖
- `Settings`：`hotkeys_enabled`（总开关，默认 false）、`Hotkeys`（3 组）、`autostart`（默认 true）、`show_on_launch`（默认 false）
- 所有字段 `#[serde(default)]`：旧版本/缺字段也能读，损坏时回落到默认值
- 保存同样使用临时文件 + rename

### 8.5 `app.rs`：应用状态机

这是最“中枢”的文件：

- `Snapshot`：发给前端的完整状态（connection、configured、power、brightness、colorTemp、mode、autoFill、ip、latencyMs、error、deviceName、model）
- `AppState`：`lamp`、`snapshot`、`settings`
- `Request`：SetPower / SetBrightness / SetColorTemp / SetMode / SetAutoFill / Reconnect
- `apply_request()`：所有写操作的统一入口
- `poll_once()` / `poll_loop()`：读状态
- `publish()`：`emit` 给前端 + 更新托盘
- `update_tray()`：根据连接/开关/主题选图标，更新 tooltip 和菜单
- `sync_autostart()`：把设置同步到 HKCU Run
- Tauri 命令：`lamp_snapshot`、`refresh_lamp`、`set_power`、`set_brightness`、`set_color_temp`、`set_mode`、`set_auto_fill`、`reconnect_lamp`、`close_panel`、`hide_panel`、`get_settings`、`save_settings`

`apply_request` 的几个关键点：

- 进门先拿锁，保证设备操作串行
- 非在线状态先置 `connecting`，让 UI 显示“正在重新连接…”
- `wake` 规则：关灯时调节亮度/色温/模式，先开灯
- 自动补光开启后额外回读一次状态
- 成功后按请求类型修正快照；失败标记离线
- 最后 `publish()`，无论成功失败都让前端和托盘看到最新状态

### 8.6 `window.rs`：窗口与关闭状态机

- `prepare()`：启动时定位
- `show()`：递增 epoch、清 CLOSING、定位、show、focus、emit `panel-show`
- `toggle()`：正在关闭或可见 → `close()`；否则 `show()`
- `close()`：幂等；emit `panel-hide`；450ms 兜底隐藏
- `finish_hide()`：前端动画结束后真正隐藏
- `place_bottom_right()`：取鼠标所在显示器的工作区，右下角留 16dp；高 DPI 用 `scale_factor` 换算

窗口本身是 `transparent: true`、无边框、`alwaysOnTop`、`skipTaskbar`、初始隐藏。**圆角由 CSS 绘制**，窗口保持逐像素透明交给 DWM 合成，这样边缘有抗锯齿；用 GDI region 裁切只有 1-bit，高 DPI 下会有锯齿。

### 8.7 `hotkeys.rs`：全局快捷键

- `HotkeyAction`：Toggle / BrightnessUp / BrightnessDown
- `HotkeyBindings`：快捷键 id → 动作的映射
- `handler()`：只处理 Pressed，异步执行 `apply_request`
- `sync()`：先清空旧注册，再按设置逐条注册
- `shortcut_for()`：把 `ctrl/alt/shift/win` 组合成 `Modifiers`，必须至少一个修饰键
- `code_for()`：把浏览器 `KeyboardEvent.key`（如 `L`、`ArrowUp`、`7`、`` ` ``）映射到 `Code`
- 重复组合键会拒绝并给出中文提示；注册失败（被其它软件占用）也会返回给 UI

亮度快捷键每次 ±5，并在 1–100 之间 clamp。

### 8.8 `theme.rs`：任务栏明暗

读取：

```text
HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize
  SystemUsesLightTheme (DWORD)
```

注意：任务栏用的是 `SystemUsesLightTheme`，应用窗口用的是 `AppsUseLightTheme`，两者是独立设置。读不到时按深色处理。`update_tray()` 每次发布状态都会重新读一次，所以用户切换任务栏主题后，最迟一个轮询周期内图标会跟着换。

### 8.9 `lampctl.rs`：命令行验证工具

不启动界面，直接和真实的灯对话：

```powershell
lampctl info
lampctl status
lampctl on | off | toggle
lampctl bright 65
lampctl temp 4000
lampctl preset read
lampctl autofill on
lampctl light 80 4000
lampctl scan
lampctl watch 10
lampctl raw get_properties '[{"did":"<DID>","siid":2,"piid":1}]'
```

它的价值：

- 在 UI 之前先验证协议和 token
- 出问题时绕过 Tauri/前端，判断是协议问题还是界面问题
- `info` 不打印 token；`raw` 是排障入口

---

## 9. 前端：单文件面板如何工作

### 9.1 结构

`ui/index.html` 分三块：

1. `<style>`：设计令牌、玻璃面板、灯具插画、滑杆、开关、设置页、动画
2. `<main class="app">`：
   - `.head`：标题、连接状态、设置按钮
   - `.stage`：可点击的台灯插画（halo / pool / mount / monitor / lightbar / emitter / 状态胶囊）
   - `.controls`：亮度滑杆、色温滑杆、四档分段控件、自动补光
   - `.view-settings`：返回、快捷键总开关、三组快捷键录制、开机自启、启动时显示
   - `.settings-foot`：版本号、局域网直连说明
3. `<script>`：状态、渲染、事件、动画、Tauri 通信

没有 `src/`、没有打包器，`tauri.conf.json` 的 `frontendDist` 直接指向 `ui/`。所有资源（除了字体）都在 HTML/CSS 里。

### 9.2 设计令牌与状态驱动

CSS 变量：

- `--lamp` / `--lamp-rgb`：当前色温对应的灯色，由 JS 在 5 个色温停靠点之间插值
- `--level`：亮度（0–1），控制光晕强度
- `--glow`：开关（0/1），关灯时光效消失
- `--text` / `--text-2` / `--text-3`：三级文字颜色

状态用 HTML 属性驱动：

- `html[data-power="on|off"]`：台灯发光、按钮状态
- `html[data-conn="online|connecting|offline"]`：状态点、胶囊文案
- `.app[data-view="main|settings"]`：主视图/设置视图切换

这样做的好处是：视觉变化主要由 CSS 完成，JS 只改属性和少量内联变量，避免在 JS 里堆样式。

### 9.3 玻璃质感怎么来的

面板没有调用 Windows 的原生 Acrylic/Mica，玻璃感由多层视觉叠加：

- 多个 `radial-gradient` / `linear-gradient`：顶部高光、底部暖光、冷色反光
- `::before` 用遮罩画出 1px 渐变描边
- `::after` 用 SVG `feTurbulence` 生成噪点，`soft-light` 混合，增加“材质感”
- 台灯区域有 `halo`、`pool`、`spill` 三层光效，跟随亮度和色温变化
- 状态胶囊用了 `backdrop-filter: blur(14px) saturate(130%)`，但面板主体不依赖它——透明窗口下 CSS 无法模糊桌面，所以主体靠渐变和噪点模拟

这是刻意的选择：不追求“真透明模糊”，而是追求在深色桌面上稳定、清晰、有层次的视觉效果。

### 9.4 台灯插画

`.stage` 里的灯不是图片，而是 CSS 图形：

- `.monitor` / `.display`：显示器与屏幕
- `.lightbar` / `.emitter`：灯管与发光面
- `.mount` / `.neck` / `.foot`：卡扣、支架、底座
- `.halo` / `.pool` / `.spill`：环境光、桌面光池、屏幕溢光

开灯时：

- `.emitter` 变成暖白色，带内阴影和光晕
- `.lightbar` 增加一点暖色反射
- `--level` 让 `halo`/`pool`/`spill` 的透明度变化
- 色温变化时，JS 重算 `--lamp`，所有光效一起变色

### 9.5 交互与渲染

核心状态：

```js
let snapshot = null;        // 后端快照
let settings = null;        // 设置
let connection = "connecting";
let capturing = null;       // 正在录制的快捷键
let lastBrightnessEdit = 0; // 防轮询回拽
let lastTempEdit = 0;
```

`render(next)` 做这些事：

1. 更新 `snapshot` / `connection` / `root.dataset.conn`
2. 更新开关视觉
3. 如果距离上次编辑超过 1.2 秒，同步亮度/色温滑杆
4. 更新自动补光、模式、设备名、型号
5. 更新连接状态文案、IP、延迟、错误 title

`call(command, args)` 统一处理 `invoke`、错误日志和渲染：

```js
async function call(command, args) {
  try {
    const next = await invoke(command, args);
    if (next) render(next);
    return next;
  } catch (error) {
    console.error(command, error);
    return null;
  }
}
```

事件绑定：

- `stage` 点击：离线时重连；在线时切换开关（乐观更新）
- 滑杆 `input`：乐观更新 → `writer` 节流 → 后端
- 预设按钮：本地先显示预设值 → `set_mode`
- 自动补光：切换 aria-checked → `set_auto_fill`
- 设置按钮/返回：`setView()`，用 `inert` 隔离另一视图
- 快捷键录制：捕获阶段监听 `keydown`，要求至少一个修饰键，Esc 取消
- Esc：设置页返回，主视图请求关闭面板
- 右键/拖拽：禁用默认菜单和拖拽，避免 WebView 的原生行为破坏“应用感”

### 9.6 可访问性

虽然是一个小面板，代码里做了不少语义：

- `aria-pressed`：台灯开关
- `aria-checked`：自动补光、各快捷键开关、自启动开关
- `aria-valuetext`：亮度/色温滑杆的“亮度 65%”“色温 4000 K”
- `aria-label`：台灯在不同连接状态下的动作说明
- `inert`：设置视图和主视图互斥，避免焦点跑到不可见区域
- `title`：错误信息通过 title 暴露，不打断视觉

### 9.7 前端取舍

- **没有框架**：状态少，手写更直接；但状态多了以后需要自律
- **预设表重复**：Rust 的 `PRESETS` 和 JS 的 `presets` 各一份；前者是权威，后者用于即时反馈。修改预设时要同时改两处
- **单文件**：易分发、易审查；但样式和逻辑都在一个文件里，需要靠注释和分区维持可读性
- **无构建**：改完刷新即可；但也意味着没有 TypeScript 类型检查，接口字段靠约定

---

## 10. 托盘、窗口、主题与快捷键

### 10.1 托盘图标三态

`app.rs` 把状态映射成三张图：

```text
connection != online → offline（珊瑚红点）
online && power       → on（暖色灯管 + 暖屏）
online && !power      → off（灰色灯管）
```

再看任务栏明暗：

```text
light = theme::taskbar_is_light()
icon = [offline, off, on][state] + (light ? _light 版本 : 深色版本)
```

`TrayHandles.icon_key` 缓存“状态 + 主题”的组合，只有变化时才调用 `set_icon`，避免每 2.5 秒重复设置同一张图。tooltip 也会同步：

- 开灯：`Yeelight 屏幕挂灯 · 开灯 80%`
- 关灯：`Yeelight 屏幕挂灯 · 已关灯`
- 连接中/离线：对应文案

右键菜单包含“显示面板 / 开灯或关灯 / 自动补光 / 退出”。`show_menu_on_left_click(false)` 让左键不弹菜单，而是走 `toggle`；右键才弹菜单。

### 10.2 窗口定位与 DPI

`place_bottom_right()`：

1. 取鼠标位置
2. 找鼠标所在显示器（找不到就用主显示器）
3. 取工作区（排除任务栏）
4. 窗口尺寸 + 16dp 边距
5. `set_position(PhysicalPosition)`

`ScaleFactorChanged` 时重新定位，保证 125%/150%/200% 缩放下都贴着右下角。窗口尺寸在 `tauri.conf.json` 里是 380×578，逻辑像素。

### 10.3 关闭动画状态机

前文已经讲过时序，这里强调设计意图：

- **为什么不在后端直接 hide？** 因为要播放退场动画；直接 hide 会“啪”一下消失
- **为什么不让前端完全负责？** WebView 可能卡顿、动画事件可能丢；后端必须有兜底
- **为什么用 epoch？** 用户可能在 450ms 内重新打开面板；旧兜底线程不能把新面板藏掉
- **为什么 close 要幂等？** 焦点丢失、Esc、托盘点击可能几乎同时发生，重复 close 不能重复 emit/开线程

### 10.4 开机自启与单实例

自启：

- `tauri-plugin-autostart` 在 HKCU Run 写 `"<exe>" --autostart`
- 设置保存时同步；每次启动也同步一次，保证 exe 换路径后开机项跟着更新
- 以 `--autostart` 启动时，默认不显示面板（除非 `show_on_launch`）

单实例：

- 第二个实例启动时，`single_instance` 插件触发回调
- 如果参数里有 `--autostart`，直接忽略（避免开机时两个实例互相弹面板）
- 否则显示已有实例的面板

### 10.5 全局快捷键

默认全部关闭，原因是很多软件会占用 `Ctrl+Alt+...` 组合，默认开启容易冲突。打开总开关后：

- 按设置逐条注册；失败的条目把原因返回给设置页
- 重复组合键在注册前就被拒绝
- 修改快捷键后先 `unregister_all()` 再重新注册
- 亮度快捷键在快照亮度基础上 ±5

---

## 11. 设计系统与图标生成

### 11.1 图标设计原则

托盘图标只有 16×16 逻辑像素（200% 缩放下 32×32 物理像素），设计上遵循：

1. **先保证轮廓**：实心显示器边框 + 悬浮灯管，缩到 16px 仍能看出“屏幕 + 灯”
2. **光学居中**：灯管、卡扣、显示器统一以 x=32（64 单位网格）对称，避免“灯是歪的”
3. **灯管悬浮**：与显示器上边框留 2.5 单位间隙，中央卡扣连接，读起来是独立产品，不是显示器顶盖
4. **状态靠颜色，不靠细节**：开灯暖色、关灯中性灰、离线珊瑚红点
5. **两套配色**：深色任务栏用浅色墨迹，浅色任务栏用深色墨迹

### 11.2 生成管线

`tools/gen_icons.py`：

- 以 64 单位网格绘制，`px=32` 时实际在 1024×1024 上超采样（`ss=16`，`n=px*ss*2`）
- 用 Pillow 的 `mask` / `gradient` / `ImageChops` / `ImageDraw` 组合形状
- 灯管有顶面高光、底部接触暗缝，避免 16px 下糊成一块
- 开灯态有紧贴灯管的暖光晕、屏幕顶部洗光；离线态有透明挖环 + 珊瑚红点
- 最后 `LANCZOS` 缩到 32×32
- 输出 6 张：`tray_on.png`、`tray_off.png`、`tray_offline.png` 及 `_light` 版本

图标通过 `include_bytes!` 编译进 exe，所以**修改图标后必须重新构建**，只换 PNG 文件不会影响已安装的程序。

应用图标、安装器页眉/侧栏、社交预览图也由类似脚本生成：

- `tools/gen_icons.py`：应用图标（1024）与托盘图标
- `tools/gen_installer_art.py`：NSIS header 150×57、sidebar 164×314
- `tools/gen_social_preview.py`：GitHub 社交预览

### 11.3 为什么值得花这么多轮

托盘图标是用户每天看到次数最多的界面元素。它在任务栏里要和微信、浏览器、系统图标并排，轮廓、重心、明暗必须经得起真实尺寸对比。只看 8 倍放大图会“自我感觉良好”，真正有效的是**截取任务栏、按 32px 原始尺寸比较**。这也是最后几轮迭代的方法：原型 → 实际任务栏截图 → 调整 → 再截图。

---

## 12. 构建、测试与发布

### 12.1 本地开发

前置：

- Rust stable（MSVC）
- Visual Studio C++ 生成工具
- Node.js 20+
- WebView2（Windows 11 自带）

```powershell
npm install
npm run tauri dev
```

前端是静态文件，`frontendDist` 指向 `ui/`，没有打包器。修改 `ui/index.html` 后，开发模式下刷新窗口即可；发布构建会把它嵌入 exe。

调试时可用：

- `YEELIGHT_LAMP_KEEP_OPEN=1`：点击面板外不自动关闭，方便截图/检查
- `YEELIGHT_LAMP_CONFIG` / `YEELIGHT_LAMP_SETTINGS`：指定备用配置/设置路径，便于隔离测试

### 12.2 测试

单元测试覆盖的是“容易悄悄坏掉”的部分：

| 文件 | 测试 |
|---|---|
| `miio.rs` | token 解析、握手包布局、与 python-miio 一致的加密已知答案 |
| `device.rs` | 预设按 id/中文名查找、数值 clamp |
| `config.rs` | did 字符串/数字两种写法、坏 token 拒绝 |
| `settings.rs` | 缺字段回落默认值、JSON 往返 |
| `hotkeys.rs` | 浏览器键名到 Code、必须带修饰键、可读组合键 |

命令：

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

CI 在 Windows runner 上执行同样的三项检查；CodeQL 另外做 Python/Actions 的静态分析。真实灯体的联调不在 CI 里，靠 `lampctl` 人工验证。

### 12.3 打包

```powershell
npm run tauri build
```

产物：

- `src-tauri/target/release/yeelight-lamp.exe`
- `src-tauri/target/release/bundle/nsis/Yeelight 屏幕挂灯_<version>_x64-setup.exe`

NSIS 配置：

- `installMode: currentUser`：装到用户目录，无需管理员权限
- 简体中文界面
- 品牌化 header/sidebar 图
- 安装包约 2.5 MB

发布资产用 `tools/package_release.ps1`：

- 从 `package.json` 读取版本（v0.1.1 起）
- 复制 NSIS 安装包为 ASCII 文件名
- 把 `yeelight-lamp.exe` 压成 portable zip
- 生成 `SHA256SUMS.txt`
- 输出到 `src-tauri/target/release/release-assets/`

### 12.4 版本发布流程

一次完整发版要改这些地方：

| 位置 | 内容 |
|---|---|
| `package.json` / `package-lock.json` | 版本号 |
| `src-tauri/Cargo.toml` / `Cargo.lock` | Rust 版本 |
| `src-tauri/tauri.conf.json` | 应用版本 |
| `ui/index.html` | 设置页版本号 |
| `README.md` | 下载文件名、当前版本 |
| `CHANGELOG.md` | 更新记录 |
| `docs/release-notes/<version>.md` | Release Notes |
| 安装器素材 | sidebar 上的版本文字 |
| Issue 模板 | 版本占位符 |

然后：

1. `npm run tauri build`
2. `tools/package_release.ps1`
3. 提交并推送，等 CI 绿
4. `gh release create v<version> --notes-file ... <assets>`
5. 本机用新安装包覆盖安装，验证文件版本、自启动路径、托盘行为

v0.1.0 是首个公开版本；v0.1.1 是托盘图标重绘 + 面板动画 + CI checkout v5 的补丁版本。Release 资产保持不可变，新改动走新版本，不覆盖旧资产。

### 12.5 没有自动化的部分

- GitHub Social preview 需要在仓库 Settings 手动上传
- 安装包没有代码签名（需要证书），SmartScreen 会提示未知发布者
- 还没有自动更新（Tauri updater）；升级方式是下载新安装包覆盖安装
- 真实灯体的协议回归测试是人工的

---

## 13. 安全与隐私设计

### 13.1 敏感数据清单

| 数据 | 位置 | 敏感度 |
|---|---|---|
| 设备 token | `%APPDATA%\yeelight-lamp\config.json` | 极高：等同设备控制权 |
| 小米账号 cookie | `session.json` | 高：可换取 token |
| 浏览器登录态 | `browser_profile/` | 高：等同登录态 |
| DID / MAC / IP | `config.json` | 中：设备标识与网络信息 |
| 设置 | `settings.json` | 低 |
| 日志 | 日志目录 `yeelight-lamp.log` | 低：不含 token |

### 13.2 代码里的防线

- `Token` 的 `Debug` 被手写为 `Token(<redacted>)`
- `Config` 的 `Debug` 不包含 token
- `get_token.py` 只打印掩码后的 token，并且不打印密码
- 程序日志只记录启动、快捷键注册、错误等，不记录 token 或完整报文
- `.gitignore` 明确排除 `config.json`、`session.json`、`browser_profile/`
- README / SECURITY.md 明确要求：Issue、讨论、截图中不要出现完整凭据

### 13.3 网络与运行时

- 日常运行只访问局域网 UDP 54321，不访问云端
- 没有遥测、没有崩溃上报、没有自动更新请求
- 只有用户主动运行 `get_token.py --login` 时才访问小米账号服务
- 安装包未签名，因此发布页提供 SHA-256 校验；下载后应核对

### 13.4 威胁模型

这套设计防的是：

- 同一 Wi-Fi 下没有 token 的旁观者（报文加密）
- 误连到同网段的其它 miIO 设备（DID 校验）
- 仓库/日志/截图里的凭据泄漏（脱敏、忽略规则、文档约束）

它不防：

- 已经拿到你 Windows 账户权限的攻击者（可直接读 `config.json`）
- 被攻陷的电脑、恶意软件、键盘记录器
- 公网暴露 54321 的网络配置
- 物理接触设备后的重新配网

所以最重要的安全措施其实是：保护 Windows 账户、不要把热点开放给不可信设备、不要把 `%APPDATA%` 里的文件发给别人。

---

## 14. 已知限制与下一步

| 限制 | 影响 | 可能的改进 |
|---|---|---|
| 只支持 `yeelink.light.lamp43` | 其它型号不能用 | 按型号抽象 `DeviceSpec`，从配置文件读取 siid/piid |
| 轮询而非推送 | 状态最多延迟 2.5s | 保持轮询；或研究设备是否有可订阅通道 |
| token 明文存放 | 本机账户被攻破即泄漏 | 用 Windows DPAPI 加密 `config.json` 中的 token |
| 安装包未签名 | SmartScreen 提示 | 购买代码签名证书，接入 CI 签名 |
| 没有自动更新 | 用户需手动下载覆盖 | 接入 Tauri updater + 签名 manifest |
| 玻璃质感是模拟的 | 面板背后桌面不会被实时模糊 | 可选接入 Windows 11 Mica/Acrylic，但需处理兼容与性能 |
| 预设表前后端各一份 | 修改时容易漏 | 由后端 `get_settings`/`snapshot` 下发预设 |
| 2.4GHz / 802.1X 限制 | 校园网/企业网无法直连 | 热点方案已覆盖大多数场景；无法在应用层解决 |
| 无过渡时间 | 调光无固件渐变 | 客户端连续写值模拟，但会增加指令量 |
| 单设备 | 不能同时控制多盏灯 | 引入设备列表与每设备会话 |

如果继续迭代，我会优先做：**token 的 DPAPI 加密**（安全收益明确、改动局部）、**代码签名**（消除安装摩擦）、**自动更新**（用户留存），最后才是多设备/多型号（会显著增加复杂度）。

---

## 15. 如何阅读这份代码：路线与练习

### 15.1 推荐阅读顺序

1. `src-tauri/src/main.rs` / `lib.rs`：入口，5 分钟
2. `src-tauri/src/app.rs`：先看 `run()`、`apply_request()`、`poll_once()`，理解状态怎么流动
3. `src-tauri/src/device.rs`：理解“灯的能力”和固件脾气
4. `src-tauri/src/miio.rs`：理解协议、加密、重试
5. `src-tauri/src/config.rs` / `settings.rs`：理解持久化与脱敏
6. `src-tauri/src/window.rs` / `hotkeys.rs` / `theme.rs`：桌面集成细节
7. `ui/index.html`：先看 HTML 结构，再看 `render`、`writer`、事件绑定
8. `tools/py/get_token.py`：理解 token 从哪来
9. `tools/gen_icons.py`：理解图标怎么生成

### 15.2 可以直接做的练习

1. **改轮询间隔**：把 `ONLINE_POLL` 从 2.5s 改成 1s，观察托盘/面板响应和流量。
2. **加一个托盘菜单项**：在 `setup_tray()` 里加“阅读模式”，调用 `apply_request(Request::SetMode(3))`。
3. **读取上电默认状态**：`piid 9` 已写进协议笔记，尝试在 `status()` 里多读一个属性，并在设置页显示。
4. **加第五个预设**：前端加一个按钮，后端加 `Preset`；注意固件只认 1–4，超出要自己组合亮度和色温。
5. **写一个解析测试**：给 `parse_hello()` 构造一个带 DID/stamp 的 32 字节响应，断言解析结果。
6. **改图标**：调整 `tray_icon()` 的灯管宽度/间隙，重新生成、重建，用真实任务栏截图对比 32px 效果。
7. **支持另一盏灯**：先查清它的 MIoT spec 与 siid/piid，再抽象常量；不要直接改死在本项目里。
8. **完整发一次版**：按 12.4 的清单走一遍，体会版本号、资产、校验和、Release 的关系。

### 15.3 容易踩的坑

- 改了 `ui/index.html` 但没重启 dev/重新构建，看到的还是旧界面（发布构建会嵌入资源）
- 改了托盘 PNG 但没重新编译，托盘不变（`include_bytes!`）
- 忘了 `config.json` 里的 IP 会变；换网络后先看 `lampctl scan`
- 在灯关着时直接写亮度，以为失败，其实是固件丢弃
- 同时开两个实例调试，托盘/自启动状态会互相干扰（用单实例或不同 `YEELIGHT_LAMP_SETTINGS`）
- 把 `config.json` 或 `session.json` 提交进 Git

---

## 16. 术语表与常见问题

### 16.1 术语

| 术语 | 含义 |
|---|---|
| **miIO** | 小米生态设备的局域网 UDP 协议，端口 54321 |
| **MIoT** | 小米 IoT 设备模型，用 siid/piid 描述服务与属性 |
| **siid / piid** | service id / property id，定位一个可读写的属性 |
| **DID** | 设备 ID，握手包和 miIO 报文里都有 |
| **token** | 16 字节设备密钥，miIO 加密的核心 |
| **stamp** | 设备时间戳（开机秒数），用于报文头 |
| **AES-128-CBC** | 对称加密模式；密钥/IV 由 token 经 MD5 派生 |
| **PKCS7** | 分组填充方案 |
| **NSIS** | Windows 安装包制作工具，Tauri 用它生成 setup.exe |
| **WebView2** | Windows 上的 Chromium 内核 WebView，Tauri 的渲染层 |
| **ICS** | Windows Internet Connection Sharing，移动热点的网段来源 |
| **DID 校验** | 连接/发现时确认设备 ID 与配置一致，防止误连 |
| **乐观更新** | 先改 UI，再等后端确认；失败时由快照纠正 |

### 16.2 常见问题

**为什么不用米家云控制？**
日常控制走云会增加延迟、依赖网络和账号，也无法在断网时使用。miIO 局域网直连更快、更可靠，隐私也更好。

**为什么一定要 token？**
这是设备的加密密钥，没有它无法构造合法指令。握手可以不要 token，但控制必须要有。

**为什么灯关着调亮度没反应？**
固件会丢弃关灯状态下的亮度/色温写入，而且不报错。程序会先开灯再写值；如果直接用 `lampctl bright`，也会遇到同样的固件行为。

**为什么状态有 2.5 秒延迟？**
没有推送通道，只能轮询。2.5 秒是响应速度和网络流量的折中；面板上的操作是乐观更新，所以手感不受影响。

**为什么面板不能模糊桌面？**
透明窗口里的 CSS `backdrop-filter` 无法采样桌面，玻璃感是用渐变、描边、噪点和光效模拟的；这是 WebView 的边界，不是样式没写。

**为什么托盘图标有两套？**
任务栏可能是深色或浅色。浅色任务栏上浅色图标几乎看不见，所以按 `SystemUsesLightTheme` 切换深/浅两套。

**为什么安装包没有签名？**
代码签名需要证书。当前安装包未签名，SmartScreen 可能提示未知发布者；请从 Releases 下载并核对 SHA-256。

**运行时会联网吗？**
不会。只有你主动运行 `get_token.py --login` 时才访问小米账号服务；日常只和局域网内的灯通信。

**换网络/换设备怎么办？**
换网络后程序会通过配置 IP → ARP → 广播重新找灯；换设备或重新配网后，重新运行 `get_token.py --login` 覆盖 token。

**可以控制多盏灯吗？**
当前版本不行。`config.json` 只保存一台设备；多设备需要引入设备列表和每设备会话。

---

## 17. 开发历程回顾

这个项目不是“先写个界面再想办法连灯”，而是反过来：

### 阶段一：确认网络与协议

- 确认挂灯只支持 2.4GHz WPA2/WPA3，无法走 802.1X
- 用 Windows 移动热点给灯组网，确认 ICS 网段和设备可达
- 验证旧版 Yeelight TCP 55443 不可用，转向 miIO UDP 54321
- 确认 MIoT 规格与属性表，实测固件行为（关灯丢写入、模式归零、自动补光回读）
- 先用 Python/命令行把握手、加密、读写跑通，再谈 UI

### 阶段二：Rust 核心与 CLI

- 手写 miIO 客户端，加入与 python-miio 一致的加密已知答案测试
- 写 `lampctl`，在没有界面的情况下验证开灯、亮度、色温、预设、自动补光、发现和重连
- 把 token、DID、IP、MAC 的读取与回写做稳，保证换 IP 后能自动恢复

### 阶段三：桌面壳与产品化

- Tauri 2 + 托盘 + 透明无边框窗口
- 状态快照、轮询、命令、托盘三态、tooltip、菜单
- 单实例、自启动、全局快捷键、设置持久化
- 断线重连、错误信息、关闭动画状态机

### 阶段四：视觉与交互打磨

- 手写玻璃面板与台灯插画，色温/亮度实时驱动光效
- 滑杆写入队列、1.2 秒防回拽、乐观更新
- 托盘图标多轮重绘：先解决“灯管偏心”，再解决“像显示器顶盖”，最后按真实 32px 任务栏截图验收
- 开合动画：前端 CSS + 后端兜底 + epoch 防竞态
- 设置页、快捷键录制、可访问性语义

### 阶段五：发布工程

- README、协议笔记、截图、贡献指南、行为准则、安全策略
- CI（fmt / clippy / test）、CodeQL、依赖告警清理
- NSIS 品牌化安装包、portable zip、SHA-256
- v0.1.0 首发，v0.1.1 图标与动画更新
- 本机安装验证：文件版本、卸载项、自启动路径、托盘行为

### 留下的经验

1. **协议先行**：先用 CLI 把设备跑通，UI 才不会掩盖协议问题。
2. **真实尺寸验收**：图标、间距、动画都要在真实任务栏/真实 DPI 下看，不能只看放大稿。
3. **状态机要显式**：窗口开合、连接状态、乐观更新，模糊的布尔值早晚会打架。
4. **凭据纪律**：token 一旦泄漏就是设备控制权；脱敏、忽略、文档约束要一起做。
5. **发版是工程**：版本号散落在多个文件，资产、校验和、Release Notes、CI 必须一起走。

---

## 附：关键文件索引

| 主题 | 位置 |
|---|---|
| miIO 报文与加密 | `src-tauri/src/miio.rs` |
| 设备属性与发现 | `src-tauri/src/device.rs` |
| 状态与命令 | `src-tauri/src/app.rs` |
| 窗口开合 | `src-tauri/src/window.rs` |
| 快捷键 | `src-tauri/src/hotkeys.rs` |
| token 配置 | `src-tauri/src/config.rs` |
| 应用设置 | `src-tauri/src/settings.rs` |
| 任务栏主题 | `src-tauri/src/theme.rs` |
| 面板前端 | `ui/index.html` |
| token 获取 | `tools/py/get_token.py` |
| 图标生成 | `tools/gen_icons.py` |
| 安装器素材 | `tools/gen_installer_art.py` |
| 发布打包 | `tools/package_release.ps1` |
| 协议实测笔记 | `docs/protocol.md` |
| 版本记录 | `CHANGELOG.md` |

---

*本文档随代码演进；如果实现与文档不一致，以代码和 `docs/protocol.md` 的实测结论为准。*
