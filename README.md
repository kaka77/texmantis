<div align="center">

[English](README.en.md) | **简体中文**

</div>

# 青刃 TexMantis

Windows 便携式系统工具箱，附带局域网即时通讯。基于 Tauri 2（Rust 后端 + Svelte 4 前端），以单个可执行文件分发，免安装、无账号、不依赖服务端。

- 运行环境：Windows 7 SP1 ~ Windows 11（x64）
- 许可证：MIT

[下载](https://github.com/kaka77/texmantis/releases) · [构建方法](#构建) · [源码结构](#源码结构)

## 功能

### 系统工具

| 模块 | 说明 |
|---|---|
| 进程 | 2 秒定时刷新；按 CPU / 内存 / 启动时间排序；数字签名状态检查；结束单个进程或按父子关系结束进程树（优先 `TerminateProcess`，失败时回退 `taskkill /T /F`）；在资源管理器中定位文件 |
| 服务 | 列出与启 / 停 / 禁用 / 删除，默认过滤 Microsoft 签名的服务 |
| 启动项 | 注册表 `Run` / `RunOnce`（HKLM、HKCU、WOW6432Node）与启动文件夹；支持删除、定位 |
| 计划任务 | 列出非 `\Microsoft` 路径的任务；运行 / 结束 / 启用 / 禁用 / 删除 |
| 网络监控 | TCP / UDP 连接表刷新；可选加载 ip2region 离线库做 IP 归属地查询 |
| 威胁预警 | 通过 `EvtSubscribe` 订阅 Sysmon/Operational 通道，按内置规则匹配可疑行为（如以内存读取权限访问 LSASS）并弹窗提示。依赖系统已安装 Sysmon |
| 文件解锁 | 基于 Restart Manager 查询文件占用进程；支持强制结束占用进程后删除 |
| 编解码 | 文本 Base64 / URL 编解码 |
| 数据提取 | 从粘贴的文本中批量提取 IPv4 / IPv6 / URL / 域名 / 主机名，支持去重与排序 |
| 系统优化 | 双击 Ctrl 显示 / 恢复桌面（低级键盘钩子实现）；桌面快捷方式创建；资源管理器右键菜单项写入 |

### 局域网社交

不依赖服务器，同一局域网内的两台设备装上即可互发消息。

- **发现**：UDP 广播（TCP / UDP 复用 25100–25109 端口段，被占自动顺延）；支持手动雷达扫描
- **消息**：单聊、群聊、表情、聊天记录查看器、右键转发、已读回执、窗口抖动
- **文件**：任意大小，分块传输；支持断点续传、暂停 / 取消 / 失败重试；接收目录可配置，默认为 `下载\TexMantis`
- **加密**：每次会话以 X25519 协商密钥，帧数据用 ChaCha20-Poly1305（AEAD）加密；协商基于对端公钥指纹核对
- **数据存放**：聊天记录（JSONL）、接收文件、配置均在本机磁盘，设置页可一键清除

### 本地推送接口

供同机脚本调用的 HTTP 接口，把自定义消息推入应用的气泡提醒：

```
GET/POST http://127.0.0.1:25110/push
鉴权：?key=… / X-API-Key 头 / Authorization: Bearer …（10 位随机密钥，在设置页生成）
参数：title（≤80 字）、body（≤600 字）
限制：仅监听 127.0.0.1；保留最近 10 条推送；气泡弹窗有 300ms 节流
```

## 下载与使用

从 [Releases](https://github.com/kaka77/texmantis/releases) 下载 zip，解压后得到：

```
TexMantis-portable/
├── TexMantis.exe                     主程序（前端资源已内嵌）
├── WebView2Loader.dll                WebView2 加载器
├── bcryptprimitives.dll              Win7 垫片：ProcessPrng → SystemFunction036
├── api-ms-win-core-synch-l1-2-0.dll  Win7 垫片：WaitOnAddress / WakeByAddress*
└── 使用说明.txt
```

整个目录拷贝到任意位置即可运行；卸载即删除目录。首次运行弹出 UAC，拒绝提权则以普通权限继续（部分功能受限，界面有提示）。

### 系统要求

| 项目 | 要求 |
|---|---|
| 操作系统 | Windows 7 SP1 / Server 2008 R2 及以上，x64 |
| WebView2 Runtime | Win11 与较新 Win10 自带；Win7 需自行安装（Evergreen 对 Win7 的最后支持版本为 109，可用固定版） |
| 权限 | 普通权限可运行；管理员权限解锁服务控制、结束进程树、计划任务管理 |

### Win7 兼容说明

Tauri 2 生成的二进制默认引用 Win8+ 的 API。本项目在链接后对 PE 导入表做改名（`GetSystemTimePreciseAsFileTime` → `GetSystemTimeAsFileTime`、`SetProcessDpiAwarenessContext` → `SetProcessDPIAware`），并附带两个自研垫片 DLL 补齐 `WaitOnAddress` 系列同步原语与 `ProcessPrng`。补丁脚本与垫片源码见 `tools/win7_patch.py` 与 `src-tauri/win7compat/`。

## 构建

### 依赖

| 依赖 | 用途 |
|---|---|
| Rust stable（rustup 默认工具链） | 后端编译 |
| Node.js ≥ 20 + npm | 前端编译 |
| MinGW-w64（`x86_64-w64-mingw32-gcc`） | Linux / macOS 交叉编译 Windows 目标时作链接器；Windows 本机可用 MSVC |
| Python 3（仅标准库） | 运行 `tools/win7_patch.py`（导入表补丁 + 构建路径擦除） |
| MinGW gcc | 仅需重新编译 `src-tauri/win7compat/` 垫片 DLL 时使用（仓库已含编译好的 DLL） |

WebView2 是最终用户的运行时要求，构建机不需要。

### 步骤（Linux 交叉编译示例）

```bash
npm install
rustup target add x86_64-pc-windows-gnu
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
npx tauri build --no-bundle --target x86_64-pc-windows-gnu
# 产物：target/x86_64-pc-windows-gnu/release/texmantis.exe
python3 tools/win7_patch.py target/x86_64-pc-windows-gnu/release/texmantis.exe
```

之后将 exe 与 `WebView2Loader.dll`、两个垫片 DLL 放入同一目录即可运行。

Windows 本机开发：`npm run tauri dev`（热更新）/ `npm run tauri build`。

## 源码结构

```
src/                      Svelte 4 前端
  App.svelte              应用壳：标题栏、导航、视图路由
  lib/Social.svelte       局域网社交（聊天 / 群 / 文件 / 设置）
  lib/ApiView.svelte      推送接口视图
  lib/api.ts              Tauri invoke 的 TypeScript 封装
src-tauri/src/
  main.rs                 入口、命令注册
  social.rs               社交核心：发现、会话、传输、加密、回执
  monitor.rs              Sysmon 预警引擎（EvtSubscribe）
  pushapi.rs              推送接口 HTTP 服务（127.0.0.1:25110）
  commands.rs             进程 / 服务 / 启动项 / 计划任务（sysinfo 原生 + PowerShell JSON）
  optimize.rs             系统优化（键盘钩子、右键菜单等）
  settings.rs             配置持久化（数据目录 config.json）
  win7compat/             Win7 垫片（C 源码 + 预编译 DLL）
tools/win7_patch.py       PE 后处理：导入表改名、panic 定位路径清零
```

## 隐私与数据

- 程序不连接任何服务器。网络流量仅有两类：局域网内的发现与通讯（UDP/TCP 25100–25109），本机回环的推送接口（127.0.0.1:25110）
- 消息与文件在传输层加密；落盘的聊天记录与接收文件为明文，存放于本地数据目录
- 发布产物在链接后清零 panic 定位字符串中的构建机路径（`tools/win7_patch.py`），可自行复核
- 本工具包含进程、服务、文件删除等系统能力，请在自己拥有操作授权的机器上使用，由此产生的后果由使用者承担

## 已知限制

- Win7 上 WebView2 需手动安装，且最高可用版本为 109
- 局域网发现依赖 UDP 广播，跨网段（不同 VLAN / 子网）不可见
- 威胁预警依赖 Sysmon，未安装时该页为空
- 群聊无已读回执；消息记录为本地明文存储

## 关于

作者业余时间编写的个人项目，练手与自用为主，没有路线图，更新不定期；Issue 与 PR 均欢迎，但不保证及时回复。

## License

[MIT](LICENSE)
