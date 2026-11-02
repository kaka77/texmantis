// ---------------------------------------------------------------------------
// v0.9.28 i18n：中英双语基础设施
// 用法：组件内 import { T, lang } from "./i18n"; 模板中 {$T("key")}
// T 是派生 store —— 切换语言后所有 $T(...) 自动重渲染。
// 语言偏好持久化在 localStorage("tz-lang")；首次启动跟随系统语言。
// ---------------------------------------------------------------------------
import { writable, derived } from "svelte/store";

export type Lang = "zh" | "en";

function initial(): Lang {
  try {
    const saved = localStorage.getItem("tz-lang");
    if (saved === "zh" || saved === "en") return saved;
  } catch {}
  return (typeof navigator !== "undefined" && navigator.language?.toLowerCase().startsWith("zh")) ? "zh" : "en";
}

export const lang = writable<Lang>(initial());
lang.subscribe((v) => { try { localStorage.setItem("tz-lang", v); } catch {} });

type Dict = Record<string, string>;

const zh: Dict = {
  // 导航
  "nav.home": "简介", "nav.processes": "进程", "nav.services": "服务",
  "nav.startup": "启动项", "nav.tasks": "计划任务", "nav.network": "网络监控",
  "nav.events": "事件日志", "nav.alerts": "威胁预警", "nav.unlock": "文件解锁",
  "nav.codec": "编解码", "nav.extract": "数据提取", "nav.optimize": "系统优化",
  "nav.social": "社交", "nav.api": "API 消息",
  // 标题栏 / 全局
  "app.tagline": "系统精密工具箱",
  "app.banner.nonAdmin": "以管理员身份运行可获得完整能力（杀进程树 / 服务控制 / 计划任务管理）。",
  "app.badge.admin": "✓ 管理员权限", "app.badge.user": "⚠ 普通权限",
  "app.btn.settings": "设置", "app.btn.api": "API 消息 · 开放推送接口",
  "app.btn.minimize": "最小化", "app.btn.maximize": "最大化", "app.btn.close": "关闭",
  "app.slogan.1": "连接美好，自在沟通。", "app.slogan.2": "言出由心，阅后即安。",
  "app.slogan.3": "安全，是沟通的底线。", "app.slogan.4": "懂你的人，都在圈里。",
  "app.slogan.5": "让沟通回归纯粹。",
  "app.confirm.title": "操作确认", "app.confirm.danger": "⚠️ 危险操作确认",
  "app.confirm.cancel": "取消", "app.confirm.ok": "确定",
  "app.exit.confirm": "确定退出青刃 TexMantis？\n\n社交功能将离线，消息与未完成的文件传输会中断。",
  "splash.tagline": "连接美好，自在沟通。",
  // 首页
  "home.sub": "系统精密工具箱 · Windows x64 绿色免安装",
  "home.intro": "青刃是一把藏在系统深处的精密小刀：进程、服务、启动项、计划任务一览无余，数字签名甄别可疑程序；文件解锁配合备份语义强删，专治最顽固的占用；网络监控直连内核 API，外联 IP 离线归属地、右键一键微步在线 / VirusTotal 威胁情报查询；数据提取从日志与流量中批量捞取 IP、URL 与域名；系统优化让桌面与资源管理器更顺手。它无服务、无驱动、无常驻进程，关闭即退、绝不驻留；启动即自动启用管理特权组，直取用户态最高权限——青竹之雅，利刃之锋。",
  "home.zone.sec": "安全检测与预警", "home.zone.file": "文件与数据", "home.zone.sys": "系统管理",
  "home.f.alerts": "威胁预警", "home.f.alerts.d": "Sysmon 规则 · 实时气泡告警",
  "home.f.network": "网络监控", "home.f.network.d": "外联 IP · 情报查询",
  "home.f.events": "事件日志", "home.f.events.d": "登录 · 账户 · 反取证",
  "home.f.unlock": "文件解锁", "home.f.unlock.d": "占用检测 · 强制删除",
  "home.f.extract": "数据提取", "home.f.extract.d": "IP · URL · 域名",
  "home.f.codec": "编解码", "home.f.codec.d": "Base64 / URL",
  "home.f.processes": "进程", "home.f.processes.d": "签名甄别 · 进程树",
  "home.f.services": "服务", "home.f.services.d": "启停禁用 · 非微软过滤",
  "home.f.startup": "启动项", "home.f.startup.d": "Run 键 · 启动文件夹",
  "home.f.tasks": "计划任务", "home.f.tasks.d": "非系统任务管理",
  "home.f.optimize": "系统优化", "home.f.optimize.d": "桌面 · 快捷键 · 右键",
  "home.tip": "提示：标题栏 ⚙ 可开启开机启动、托盘驻留与贴边浮漂（右缘磁吸拉片）。",
  // 设置（全局）
  "set.group.startup": "启动与托盘", "set.group.alert": "威胁预警", "set.group.res": "资源占用说明",
  "set.group.lang": "语言 / Language",
  "set.autostart": "开机自动启动",
  "set.autostart.d": "写入当前用户注册表 Run 键（不建服务/计划任务，零常驻开销）；取消勾选即还原",
  "set.closeToTray": "关闭按钮隐藏到托盘",
  "set.closeToTray.d": "点击 ✕ 时隐藏窗口并保留托盘图标，右键托盘图标可彻底退出；配合开机启动可后台待命",
  "set.float": "贴边浮漂（迷你 UFO）",
  "set.float.d": "屏幕右缘趴着一艘迷你三角 UFO：关闭主窗自动停靠贴边，点浮标一键呼出主窗，不遮挡屏幕",
  "set.pauseOnBlur": "窗口失焦时暂停自动刷新",
  "set.pauseOnBlur.d": "资源占用核心策略：窗口不在前台/被最小化时，进程、网络等页面的轮询自动空转，切回前台立即恢复，CPU 占用趋近于 0",
  "set.diag": "诊断日志",
  "set.diag.d": "记录预警链路 / 端到端自检 / 健康巡检等 debug 明细到 data\\logs\\diag.log，默认关闭——不开启则完全不产生日志；关闭时会自动删除已有日志文件",
  "set.sysmon": "Sysmon 威胁监控",
  "set.sysmon.d": "基于 Windows 事件订阅的高风险行为告警（事件驱动、零轮询、无临时文件）；需本机安装 Sysmon，规则可在「威胁预警」页配置",
  "set.retention": "告警保留期",
  "set.retention.d": "自动清除 alerts.json（与工具同目录）中多久之前的告警数据，变更即时生效并立即清理",
  "set.ret.7": "一周前", "set.ret.30": "一个月前", "set.ret.90": "三个月前",
  "set.res.body": "· 无任何后台服务 / 驱动 / 常驻进程，关闭窗口（未开托盘/浮漂驻留）即完全退出\n· 托盘仅为消息泵，无轮询线程；网络监控走 iphlpapi 内核 API（毫秒级、无 PowerShell 开销）\n· 自动刷新定时器仅在其页面处于前台可见时运行，失焦自动暂停（可关闭此策略）\n· IP 归属地为离线库查询，数据不出本机\n· 启动时自动启用 SeDebug / SeTakeOwnership / SeBackup / SeRestore 等管理特权（用户态最高权限组合）",
  "set.loading": "加载中…",
  "set.toast.saved": "已保存",
  "set.toast.auto.on": "已加入开机启动（当前用户，无需管理员）", "set.toast.auto.off": "已取消开机启动",
  "set.toast.diag.on": "诊断日志已开启（data\\logs\\diag.log）", "set.toast.diag.off": "诊断日志已关闭，日志文件已删除",
  "set.toast.amon.on": "威胁预警监控已开启（需本机安装 Sysmon，事件驱动零轮询）", "set.toast.amon.off": "威胁预警监控已关闭，立即生效",
  "set.toast.ret": "告警保留期已设为 {n}，过期数据已即时清除",
  "ret.week": "一周", "ret.month": "一个月", "ret.quarter": "三个月",
};

const en: Dict = {
  "nav.home": "Home", "nav.processes": "Processes", "nav.services": "Services",
  "nav.startup": "Startup", "nav.tasks": "Scheduled Tasks", "nav.network": "Network",
  "nav.events": "Event Log", "nav.alerts": "Threat Alerts", "nav.unlock": "File Unlock",
  "nav.codec": "Encode/Decode", "nav.extract": "Data Extract", "nav.optimize": "Tweaks",
  "nav.social": "Social", "nav.api": "API Messages",
  "app.tagline": "System Toolbox",
  "app.banner.nonAdmin": "Run as administrator for full capabilities (kill process trees, service control, scheduled tasks).",
  "app.badge.admin": "✓ Administrator", "app.badge.user": "⚠ Standard user",
  "app.btn.settings": "Settings", "app.btn.api": "API messages · push interface",
  "app.btn.minimize": "Minimize", "app.btn.maximize": "Maximize", "app.btn.close": "Close",
  "app.slogan.1": "Connect beautifully, talk freely.", "app.slogan.2": "Say what you mean, then let it go.",
  "app.slogan.3": "Security is the baseline of conversation.", "app.slogan.4": "Your people, in one circle.",
  "app.slogan.5": "Communication, back to pure and simple.",
  "app.confirm.title": "Confirm", "app.confirm.danger": "⚠️ Dangerous operation",
  "app.confirm.cancel": "Cancel", "app.confirm.ok": "OK",
  "app.exit.confirm": "Quit TexMantis?\n\nThe LAN session will go offline; messages and unfinished transfers will be interrupted.",
  "splash.tagline": "Connect beautifully, talk freely.",
  "home.sub": "System toolbox · Windows x64, portable",
  "home.intro": "TexMantis is a compact toolkit that lives inside Windows: processes, services, startup entries and scheduled tasks at a glance, with signature checks to flag suspicious binaries. File Unlock ends the most stubborn file locks, with backup-semantics force delete. Network monitoring talks to kernel APIs directly, resolves external IPs against an offline location database, and offers one-click lookups on MicroStep / VirusTotal. Data Extract sweeps IPs, URLs and domains out of logs and traffic. Tweaks make the desktop and Explorer more comfortable. No services, no drivers, no resident processes — it exits fully when closed; on start it enables the management privilege groups for the highest user-mode access available.",
  "home.zone.sec": "Security & Alerts", "home.zone.file": "Files & Data", "home.zone.sys": "System Management",
  "home.f.alerts": "Threat Alerts", "home.f.alerts.d": "Sysmon rules · live popups",
  "home.f.network": "Network", "home.f.network.d": "outbound IPs · intel lookup",
  "home.f.events": "Event Log", "home.f.events.d": "logon · accounts · anti-forensics",
  "home.f.unlock": "File Unlock", "home.f.unlock.d": "lock detection · force delete",
  "home.f.extract": "Data Extract", "home.f.extract.d": "IP · URL · domain",
  "home.f.codec": "Encode/Decode", "home.f.codec.d": "Base64 / URL",
  "home.f.processes": "Processes", "home.f.processes.d": "signature check · trees",
  "home.f.services": "Services", "home.f.services.d": "start/stop · filter MS",
  "home.f.startup": "Startup", "home.f.startup.d": "Run keys · startup folder",
  "home.f.tasks": "Scheduled Tasks", "home.f.tasks.d": "non-system tasks",
  "home.f.optimize": "Tweaks", "home.f.optimize.d": "desktop · hotkey · context menu",
  "home.tip": "Tip: the ⚙ button in the title bar toggles auto-start, tray residency and the edge dock.",
  "set.group.startup": "Startup & Tray", "set.group.alert": "Threat Alerts", "set.group.res": "Resource Footprint",
  "set.group.lang": "Language / 语言",
  "set.autostart": "Start with Windows",
  "set.autostart.d": "Writes the current user's Run key (no service or scheduled task, zero resident cost); untick to revert",
  "set.closeToTray": "Close button hides to tray",
  "set.closeToTray.d": "✕ hides the window and keeps the tray icon; right-click the tray icon to quit fully",
  "set.float": "Edge dock (mini UFO)",
  "set.float.d": "A mini UFO rests on the right screen edge when the main window closes; click it to summon the window back",
  "set.pauseOnBlur": "Pause auto-refresh when unfocused",
  "set.pauseOnBlur.d": "Core resource strategy: when the window is not in the foreground, polling on processes/network pages idles; it resumes instantly on focus, CPU cost near zero",
  "set.diag": "Diagnostic log",
  "set.diag.d": "Writes debug detail (alert pipeline, self-checks) to data\\logs\\diag.log; off by default — no file is created unless enabled, and disabling deletes the existing file",
  "set.sysmon": "Sysmon threat monitor",
  "set.sysmon.d": "High-risk behaviour alerts via Windows event subscription (event-driven, zero polling, no temp files); requires Sysmon, rules configurable on the Threat Alerts page",
  "set.retention": "Alert retention",
  "set.retention.d": "How long to keep alerts.json data (same folder as the tool); changes apply and purge immediately",
  "set.ret.7": "older than a week", "set.ret.30": "older than a month", "set.ret.90": "older than 3 months",
  "set.res.body": "· No background service, driver or resident process — closing the window (unless tray/dock residency is on) exits fully\n· The tray is a message pump only, no polling threads; network monitoring uses iphlpapi kernel APIs (milliseconds, no PowerShell overhead)\n· Auto-refresh timers run only while their page is visible; they idle when unfocused (policy can be disabled)\n· IP geolocation is an offline database lookup; data never leaves the machine\n· On start the tool enables SeDebug / SeTakeOwnership / SeBackup / SeRestore privilege groups (highest user-mode combination)",
  "set.loading": "Loading…",
  "set.toast.saved": "Saved",
  "set.toast.auto.on": "Auto-start enabled (current user, no admin needed)", "set.toast.auto.off": "Auto-start disabled",
  "set.toast.diag.on": "Diagnostic log enabled (data\\logs\\diag.log)", "set.toast.diag.off": "Diagnostic log disabled, file deleted",
  "set.toast.amon.on": "Threat monitor enabled (requires Sysmon; event-driven, zero polling)", "set.toast.amon.off": "Threat monitor disabled, effective immediately",
  "set.toast.ret": "Retention set to {n}; expired data purged",
  "ret.week": "one week", "ret.month": "one month", "ret.quarter": "three months",
};

const dicts: Record<Lang, Dict> = { zh, en };

export const T = derived(lang, (l) => (key: string, params?: Record<string, string | number>): string => {
  let s = dicts[l][key] ?? dicts.zh[key] ?? key;
  if (params) for (const [k, v] of Object.entries(params)) s = s.split(`{${k}}`).join(String(v));
  return s;
});

export function setLang(l: Lang): void { lang.set(l); }
export function langLabel(l: Lang): string { return l === "zh" ? "中文" : "English"; }
