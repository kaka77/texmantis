<div align="center">

# 青刃 TexMantis

**刃中藏螳 —— 一把藏在系统深处的精巧瑞士军刀**

Windows 便携式系统工具箱 × 局域网社交 · 免安装 · 无账号 · 数据全本地

`Tauri 2` · `Rust` · `Svelte 4` · `Win7 SP1 ~ Win11`

[![Platform](https://img.shields.io/badge/platform-Windows%207%20SP1%20%7C%2010%20%7C%2011-blue)](#系统要求)
[![License: MIT](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange)](https://tauri.app)
[![vibe coding](https://img.shields.io/badge/made%20with-娱乐%20vibe%20coding-ff69b4)](#关于本项目)

</div>

---

> **English** · A portable Windows toolbox with a built-in LAN social suite: process/service/startup/task management, Sysmon threat alerts, E2E-encrypted LAN chat with resumable file transfer, and a local push API for your own scripts. Single portable exe, no account, no cloud — all data stays on your machine. Written for fun (vibe coding), updated casually.

---

## 这是什么

**青刃 TexMantis** 是一款 **Windows 便携式系统精密工具箱**，附带一套完整的**局域网社交功能**。
整个程序是一个绿色免安装的 exe（外加 Win7 兼容垫片 DLL），不写注册表、不留残留、不需要账号、
没有任何云端 —— 拷到 U 盘就能走，双击即用。

它长着一副「macOS 毛玻璃颜值」的脸：Mica / Acrylic 透明窗口、无边框圆角、iOS 风格弹层与开关，
但干的全是系统级的粗活。

## 特色 ✨

### 🔧 系统精密工具箱
| 模块 | 能力 |
|---|---|
| **进程** | 2s 自动刷新、CPU/内存/启动时间排序、数字签名检测（✔/⚠/✖）、结束进程树（先子后父，失败回退 `taskkill /T /F`）、资源管理器定位 |
| **服务 / 启动项 / 计划任务** | 默认过滤微软自家项、启/停/禁用/删除、注册表 Run·RunOnce·启动文件夹、签名检测 |
| **网络监控** | 实时连接/流量视图 |
| **威胁预警** | 订阅 Sysmon 事件通道（`EvtSubscribe` 零轮询），可疑行为气泡提醒（如 LSASS 访问等） |
| **文件解锁** | 找出占用文件的进程并强制解锁 |
| **编解码 / 数据提取** | 常用格式转换与提取的顺手小工具 |
| **系统优化** | 双击 Ctrl 切换桌面、桌面快捷方式、资源管理器右键直达终端等一键优化 |

### 💬 局域网社交（无需服务器）
- **发现**：UDP 广播自动发现同网段同伴，一键雷达；离线灰显、上线即恢复
- **聊天**：单聊 / 群聊 / 表情 / 字号调节 / 消息记录查看器 / 右键转发 / **已读回执** / 抖一抖（原生窗口抖动）
- **文件传输**：**断点续传**、暂停/取消/失败重试、右键转发、双击打开；接收目录可配置
- **端到端加密**：X25519 会话密钥协商 + ChaCha20-Poly1305 AEAD 帧加密 —— 局域网明文嗅探拿不到内容
- **隐私**：无账号、无云端、无遥测；聊天记录与接收文件全部落在本机目录，一键清除

### 🔔 开放推送 API（脚本党的玩具）
内置一个**仅本机回环**的 HTTP 接口，任何脚本 / 定时任务 / 监控程序都能把消息推进应用的统一气泡：

```bash
# 社交 → 铃铛按钮（API 消息）→ 打开开关 → 复制地址与 10 位密钥
curl "http://127.0.0.1:25110/push?key=你的密钥&title=服务器CPU&body=92%25"
# 或 POST JSON：{"key":"…","title":"…","body":"…"}
```

10 位随机密钥鉴权（可随时重生成）、最近 10 条推送历史、300ms 节流防风暴。
写个 20 行的脚本监控磁盘 / 爬虫结果 / CI 状态，推送进来统一提醒 —— 就这么简单。

### 🛡️ 产物加固（对分发二进制的较真）
- `panic = "abort"` 消除栈展开表
- **构建环境路径擦除**：链接后自动清零 panic 定位字符串（依赖 crate registry 路径 / std `/rustc/` 路径），
  发布产物零构建机痕迹
- 自动化 PE 补丁脚本（`tools/win7_patch.py`）：导入表兼容改名 + 路径擦除，构建一条龙

## 便携使用（终端用户）

到 [Releases](../../releases) 下载 `TexMantis_x.y.z_windows_x64.zip`，解压即得：

```
TexMantis-portable/
  TexMantis.exe            主程序（前端资源已内嵌）
  WebView2Loader.dll       WebView2 加载器
  bcryptprimitives.dll     Win7 兼容垫片（自研，ProcessPrng → SystemFunction036）
  api-ms-win-core-synch-l1-2-0.dll  Win7 兼容垫片（WaitOnAddress 系列同步原语）
  使用说明.txt
```

整个文件夹拷贝到任意位置即可运行；首次运行弹出 UAC（拒绝则以普通权限继续，受限功能有提示）。

## 系统要求

| 项目 | 要求 |
|---|---|
| 系统 | Windows 7 SP1 / Server 2008 R2 ~ Windows 11，x64 |
| WebView2 | Win11 / 较新 Win10 自带；Win7 需安装 [Evergreen 或 109 固定版 Runtime](https://developer.microsoft.com/microsoft-edge/webview2/) |
| 权限 | 普通权限可用；管理员权限解锁服务控制 / 杀进程树 / 计划任务管理 |

> **Win7 兼容是怎么做到的**：Tauri 2 官方不支持 Win7。本项目通过自研垫片（`src-tauri/win7compat/`，
> 提供 `WaitOnAddress` 系列同步原语与 `ProcessPrng`）+ 导入表改名补丁
> （`GetSystemTimePreciseAsFileTime → GetSystemTimeAsFileTime`、`SetProcessDpiAwarenessContext → SetProcessDPIAware`）
> 让现代 Rust/Tauri 二进制跑在 2009 年的系统上。

## 从源码构建（开发者）

### 环境依赖

| 依赖 | 版本 | 说明 |
|---|---|---|
| Rust | stable（rustup 默认即可） | Tauri 2 全家桶 |
| Node.js | ≥ 20 | 前端构建 |
| Tauri 2 CLI | `^2.1` | 随 npm devDependencies 安装 |
| MinGW-w64 | `x86_64-w64-mingw32-gcc` | **交叉编译必需**（Linux/macOS 构建Windows 目标）；Windows 本机构建用 MSVC 工具链亦可 |
| Win7 垫片编译 | MinGW `gcc` | 仅重新编译 `src-tauri/win7compat/*.dll` 时需要（仓库已含编译好的 DLL） |
| Python | 3.x，仅标准库 | 运行 `tools/win7_patch.py`（导入表补丁 + 路径擦除） |
| Sysmon（可选） | 任意近期版本 | 不装则威胁预警页为空，其余功能不受影响 |

> ⚠️ **WebView2** 是**运行时**要求而非构建要求 —— 构建机不需要装；最终用户的机器需要（见上表）。

### 构建步骤（Linux 交叉编译 Windows 示例）

```bash
# 1. 前端依赖
npm install

# 2. 交叉编译目标 + 链接器
rustup target add x86_64-pc-windows-gnu
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc

# 3. 构建（--no-bundle = 只要 exe，不打安装器）
npx tauri build --no-bundle --target x86_64-pc-windows-gnu
#    产物：target/x86_64-pc-windows-gnu/release/texmantis.exe

# 4. PE 后处理（Win7 导入表兼容 + 构建路径擦除，必跑）
python3 tools/win7_patch.py target/x86_64-pc-windows-gnu/release/texmantis.exe

# 5. 与 WebView2Loader.dll、win7compat 两个垫片 DLL、使用说明.txt 同目录打包
```

开发模式（本机 Windows）：

```bash
npm run tauri dev      # Vite 热更新 + 桌面窗口
npm run tauri build    # 完整构建（含安装器）
```

### 目录结构

```
src/                      Svelte 4 前端（19 个视图组件）
  App.svelte              应用壳：标题栏 + 导航 + 路由
  lib/Social.svelte       局域网社交（最大组件：聊天/群/文件/设置）
  lib/ApiView.svelte      开放推送接口视图
src-tauri/src/
  main.rs                 入口 + 命令注册 + 窗口美化
  social.rs               局域网社交核心（发现/会话/传输/加密）
  monitor.rs              Sysmon 威胁预警引擎（EvtSubscribe）
  pushapi.rs              开放推送 HTTP 接口（127.0.0.1:25110）
  commands.rs             进程/服务/启动项/计划任务（sysinfo 原生 + PowerShell JSON 桥）
  optimize.rs             系统优化（低级键盘钩子等）
  settings.rs             配置持久化（exe 同目录 config.json）
  win7compat/             Win7 兼容垫片（C 源码 + 预编译 DLL）
tools/win7_patch.py       PE 后处理（导入表改名 + 构建路径擦除）
```

## 隐私与安全

- **无网络出站**：所有流量限于局域网（发现/会话）与本机回环（推送 API）；不连任何云端
- **数据本地化**：聊天记录、接收文件、配置全部在本地目录，设置内一键清除
- **传输加密**：会话经 X25519 + ChaCha20-Poly1305 端到端加密
- **双刃剑提醒**：本工具包含进程管理 / 服务控制 / 文件解锁等系统能力，**请仅在自己拥有授权的机器上使用**；
  由此产生的任何后果由使用者自行承担

## 关于本项目 🌴

这是一个**作者娱乐 vibe coding 的软件** —— 深夜灵感来了写两笔，手感好了加个功能，
没有 roadmap、没有 deadline、没有 KPI。

- **佛系更新**：更新频率看心情，版本号跳跃是常态，长期断更也正常
- Issue / PR 都欢迎，但回复速度同样佛系 🙂
- 如果它帮到了你，随手点个 Star 就是最大的鼓励

## License

[MIT](LICENSE)
