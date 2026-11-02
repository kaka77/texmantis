<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import {
    alertsGet, alertsClear, alertRuleSet, alertRulesState, alertMonitorStatus, alertEngineToggle,
    alertSysmonInstall, alertImportConfig, alertSelftest,
  alertSysmonResync, diagRead, diagClear, alertWlGet, alertWlSet, deepSelfcheck,
  alertTest, alertMuteSet,
  } from "./api";
  import type { AlertInfo, RuleDef, AlertStatus, AlertWl, CheckItem } from "./api";
  import { toast, askConfirm, rowCtx, openCtx, copyText, pendingAlertDetail, appPaused } from "./stores";
  import { get } from "svelte/store";
  import DetailModal from "./DetailModal.svelte";
  import { listen } from "@tauri-apps/api/event";
  import type { DetailSection } from "./DetailModal.svelte";

  // ---------------------------------------------------------------------------
  // 内置规则表（与后端引擎一一对应；勾选即时生效，状态持久化于 config.json）
  // ---------------------------------------------------------------------------
  const RULES: RuleDef[] = [
    { id: "E1_ENC_PS", event_id: 1, name: "进程创建 · 编码 PowerShell 执行", level: 3, mitre: "T1059.001",
      desc: "PowerShell 使用 -enc/-EncodedCommand 编码参数运行命令，无文件攻击与载荷投递的常用手法",
      note: "命中即时弹窗", def_on: true },
    { id: "E1_OFFICE_CHILD", event_id: 1, name: "进程创建 · Office 派生可疑子进程", level: 3, mitre: "T1566.001",
      desc: "Word/Excel/PPT/邮件客户端派生 cmd/powershell/wscript 等，钓鱼附件载荷执行的典型标志",
      note: "命中即时弹窗", def_on: true },
    { id: "E3_BADPORT", event_id: 3, name: "网络连接 · 非常规端口外联", level: 2, mitre: "T1071.001 / T1571",
      desc: "进程向非常用端口发起外联，可能是 C2 回连、数据外排或横向移动",
      note: "记录不弹窗（防噪音）", def_on: true },
    { id: "E7_TEMP_DLL", event_id: 7, name: "模块加载 · 临时目录 DLL", level: 2, mitre: "T1574.001 / T1574.002",
      desc: "从 Temp 临时目录加载 DLL/OCX 模块，常见于 DLL 劫持与恶意模块加载",
      note: "记录不弹窗", def_on: true },
    { id: "E8_REMOTE_THREAD", event_id: 8, name: "跨进程远程线程注入", level: 3, mitre: "T1055.001 / T1055.002",
      desc: "进程在其他进程内存空间创建远程线程，代码注入与隐蔽执行的强指标（系统目录来源已过滤）",
      note: "命中即时弹窗", def_on: true },
    { id: "E10_LSASS", event_id: 10, name: "进程访问 · LSASS 内存读取", level: 3, mitre: "T1003.001",
      desc: "以内存读取权限访问 lsass.exe，典型 Mimikatz/ProcDump 凭据转储；系统目录来源自动降级为记录",
      note: "第三方来源即时弹窗", def_on: true },
    { id: "E11_STARTUP", event_id: 11, name: "文件创建 · 启动目录写入", level: 3, mitre: "T1105 / T1547.001",
      desc: "向启动（Startup）目录写入文件，开机自启持久化的直接信号",
      note: "命中即时弹窗", def_on: true },
    { id: "E11_TEMP_DROP", event_id: 11, name: "文件创建 · 临时目录落盘", level: 2, mitre: "T1105",
      desc: "Temp 目录创建可执行/脚本文件（载荷投递指标）；安装器/更新器也会高频产生同类事件，噪音大，默认关闭按需开启",
      note: "默认关闭 · 按需观察", def_on: false },
    { id: "E13_RUNKEY", event_id: 13, name: "注册表 · Run 键持久化", level: 3, mitre: "T1547.001",
      desc: "写入 CurrentVersion\\Run(-Once) 自启键值，注册表持久化最常见位置",
      note: "命中即时弹窗", def_on: true },
    { id: "E13_DEFENSE", event_id: 13, name: "注册表 · 安全防御削弱", level: 3, mitre: "T1562.001",
      desc: "修改 DisableAntiSpyware 或 Defender 策略键值，试图关闭系统防护",
      note: "命中即时弹窗", def_on: true },
    { id: "AC_NEW", event_id: 4720, name: "新建本地账户", level: 3, mitre: "T1136.001",
      desc: "Security 4720：系统新增用户账户（后门账户持久化头号手法）；家用机极少出现，命中请立即核查", note: "命中即时弹窗", def_on: true },
    { id: "AC_GROUP", event_id: 4732, name: "本地组成员变更", level: 2, mitre: "T1098",
      desc: "Security 4732：本地组成员变更；加入管理员组（提权后门）时升级弹窗", note: "记录 · 入管理员组弹窗", def_on: true },
    { id: "E22_DNS", event_id: 22, name: "DNS 查询 · 全量记录", level: 2, mitre: "T1071.004 / T1568.002",
      desc: "进程发起 DNS 解析（DNS 隧道/恶意域名），全量记录量大",
      note: "默认关闭，按需勾选观察", def_on: false },
    { id: "E25_TAMPER", event_id: 25, name: "进程篡改 · Hollowing", level: 3, mitre: "T1055.012",
      desc: "进程映像被替换/篡改，Process Hollowing、Herpaderping 等高级规避技术指标",
      note: "命中即时弹窗", def_on: true },
    // ---------- 红队规则库（TTP 行为） ----------
    { id: "RT_CRED_DUMP", event_id: 1, name: "凭据转储工具执行", level: 3, mitre: "T1003.001 / T1003.003",
      desc: "命令行命中 mimikatz/sekurlsa/lsadump 等凭据转储特征", note: "命中即时弹窗", def_on: true },
    { id: "RT_COMSVCS", event_id: 1, name: "comsvcs.dll 转储 LSASS", level: 3, mitre: "T1003.001",
      desc: "rundll32 调用 comsvcs.dll MiniDump 导出 LSASS 内存", note: "命中即时弹窗", def_on: true },
    { id: "RT_POTATO", event_id: 1, name: "Potato 家族提权利用", level: 3, mitre: "T1068",
      desc: "JuicyPotato/GodPotato/PrintSpoofer 等系统级本地提权工具", note: "命中即时弹窗", def_on: true },
    { id: "RT_PRIVESC", event_id: 1, name: "提权枚举工具", level: 2, mitre: "T1068",
      desc: "PowerUp/Sherlock/BeRoot/SharpUp 等提权枚举", note: "记录不弹窗", def_on: true },
    { id: "RT_USER_ADD", event_id: 1, name: "新建本地账户", level: 3, mitre: "T1136.001",
      desc: "net user … /add 创建账户（持久化/后门前置动作）", note: "命中即时弹窗", def_on: true },
    { id: "RT_GROUP_ADD", event_id: 1, name: "账户加入管理员组", level: 3, mitre: "T1078.002",
      desc: "net localgroup administrators … /add 提权", note: "命中即时弹窗", def_on: true },
    { id: "RT_TOKEN", event_id: 1, name: "令牌窃取/模拟", level: 3, mitre: "T1134.001",
      desc: "Invoke-TokenManipulation/incognito 等令牌操控", note: "命中即时弹窗", def_on: true },
    { id: "RT_NTDS", event_id: 1, name: "AD 数据库转储", level: 3, mitre: "T1003.003",
      desc: "ntdsutil / ntds.dit 相关命令（域凭据大规模窃取）", note: "命中即时弹窗", def_on: true },
    { id: "RT_RECOV", event_id: 1, name: "破坏系统恢复能力", level: 3, mitre: "T1490",
      desc: "删卷影副本/禁用恢复（勒索与毁痕前置）", note: "命中即时弹窗", def_on: true },
    { id: "RT_PS_CRADLE", event_id: 1, name: "PowerShell 远程载荷执行", level: 3, mitre: "T1059.001 / T1105",
      desc: "IEX/DownloadString 下载擂（cradle）内存执行远程代码", note: "命中即时弹窗", def_on: true },
    { id: "RT_CERTUTIL", event_id: 1, name: "certutil 远程下载", level: 2, mitre: "T1105",
      desc: "certutil -urlcache/-verifyctl 下载（LOLBin 滥用）", note: "记录不弹窗", def_on: true },
    { id: "RT_BITSADMIN", event_id: 1, name: "BITS 后台传输", level: 2, mitre: "T1197",
      desc: "bitsadmin /transfer 后台下载（绕常规监控）", note: "记录不弹窗", def_on: true },
    { id: "RT_MSHTA", event_id: 1, name: "mshta 脚本载荷", level: 3, mitre: "T1218.005",
      desc: "mshta 执行 http/vbscript/javascript 载荷", note: "命中即时弹窗", def_on: true },
    { id: "RT_REGSVR32", event_id: 1, name: "regsvr32 远程脚本", level: 2, mitre: "T1218.010",
      desc: "regsvr32 /i:http 加载 scrobj（squiblydoo）", note: "记录不弹窗", def_on: true },
    { id: "RT_HIDDEN_JS", event_id: 1, name: "隐藏窗口脚本执行", level: 2, mitre: "T1059.005",
      desc: "wscript/cscript //b 无窗口静默执行", note: "记录不弹窗", def_on: true },
    { id: "RT_DEF_PS", event_id: 1, name: "PowerShell 关闭 Defender/加排除", level: 3, mitre: "T1562.001",
      desc: "Set-MpPreference 关实时防护或添加查杀排除", note: "命中即时弹窗", def_on: true },
    { id: "RT_FW_OFF", event_id: 1, name: "关闭防火墙", level: 2, mitre: "T1562.004",
      desc: "netsh advfirewall state off", note: "记录不弹窗", def_on: true },
    { id: "RT_SVC_SC", event_id: 1, name: "服务创建命令", level: 2, mitre: "T1543.003",
      desc: "sc create 注册服务（实际安装另由 7045 捕获）", note: "记录不弹窗", def_on: true },
    { id: "RT_SVC_CFG", event_id: 1, name: "服务 binPath 修改", level: 2, mitre: "T1543.003",
      desc: "sc config 修改服务二进制路径（服务劫持提权）", note: "记录不弹窗", def_on: true },
    { id: "RT_SCHTASKS", event_id: 1, name: "计划任务创建命令", level: 2, mitre: "T1053.005",
      desc: "schtasks /create；/ru SYSTEM 时升级弹窗", note: "记录 · SYSTEM 弹窗", def_on: true },
    { id: "RT_IFEO", event_id: 13, name: "IFEO 镜像劫持", level: 3, mitre: "T1546.012",
      desc: "Image File Execution Options 写入 Debugger（劫持任意程序启动）", note: "命中即时弹窗", def_on: true },
    { id: "RT_WINLOGON", event_id: 13, name: "Winlogon 登录劫持", level: 3, mitre: "T1547.004",
      desc: "Winlogon Shell/Userinit/Notify 被修改", note: "命中即时弹窗", def_on: true },
    { id: "RT_APPINIT", event_id: 13, name: "AppInit_DLLs 全局注入", level: 3, mitre: "T1546.010",
      desc: "AppInit_DLLs / AppInit_X64_DLLs 被写入", note: "命中即时弹窗", def_on: true },
    { id: "RT_BROWSER_FORCE", event_id: 13, name: "浏览器扩展强制安装", level: 3, mitre: "T1176",
      desc: "ExtensionInstallForcelist 被写入（强推扩展/劫持）", note: "命中即时弹窗", def_on: true },
    { id: "RT_BROWSER_HIJACK", event_id: 13, name: "浏览器主页/搜索劫持", level: 2, mitre: "T1176",
      desc: "主页/启动页/默认搜索提供程序被修改", note: "记录不弹窗", def_on: true },
    { id: "RT_WMI", event_id: 19, name: "WMI 无文件持久化", level: 3, mitre: "T1546.005",
      desc: "WMI 筛选器/消费者/绑定注册（无文件落地持久化，Sysmon 19/20/21）", note: "命中即时弹窗", def_on: true },
    { id: "RT_SVC7045", event_id: 7045, name: "系统服务安装", level: 2, mitre: "T1543.003",
      desc: "System 通道 7045：新服务注册；路径可疑（Temp/AppData/脚本）弹窗", note: "记录 · 可疑弹窗", def_on: true },
    { id: "RT_TASK106", event_id: 106, name: "计划任务注册", level: 2, mitre: "T1053.005",
      desc: "TaskScheduler 通道 106：新任务注册（无前台安装文件的持久化常用手法）；含脚本/临时目录/下载等可疑特征时升级弹窗", note: "记录 · 可疑升级弹窗", def_on: true },
    // ---------- 外部 IP 口令爆破（安全日志关联） ----------
    { id: "BR_NET", event_id: 4625, name: "外部 IP 网络口令爆破", level: 2, mitre: "T1110.001",
      desc: "SMB/WMI/WinRM（LogonType 3/8）远程口令尝试：10 分钟 5 次记录、20 次弹窗、持续每 50 次提醒", note: "5 次记录 · 20 次弹窗", def_on: true },
    { id: "BR_RDP", event_id: 4625, name: "外部 IP RDP 口令爆破", level: 3, mitre: "T1110.001",
      desc: "RDP（LogonType 10）远程口令尝试：10 分钟 5 次记录、20 次弹窗、持续每 50 次提醒", note: "5 次记录 · 20 次弹窗", def_on: true },
    { id: "BR_NET_SUCCESS", event_id: 4624, name: "爆破后成功登录（高危）", level: 3, mitre: "T1110 / T1078",
      desc: "外部 IP 多次失败后成功登录——口令可能已被猜出或泄露", note: "命中即时弹窗", def_on: true },
  ];

  let st: AlertStatus | null = null;
  let testing = false;
  let records: AlertInfo[] = [];
  let disabled: string[] = [];
  let loading = false;
  let wasPaused = false;            // appPaused 跃迁检测（false→true→false = 一轮失焦）
  let unPausedWatch: (() => void) | null = null;
  let levelSel = 0; // 0 全部
  let q = "";
  let detail: AlertInfo | null = null;
  // 气泡点击直达：接收待展示告警并打开详情（消费后清空）
  pendingAlertDetail.subscribe((v) => {
    if (v) {
      detail = v;
      pendingAlertDetail.set(null);
    }
  });
  let showRules = false; // 规则库弹窗
  let showDiag = false;  // 诊断日志弹窗
  let showCheck = false; // 深度自检弹窗
  let checkItems: CheckItem[] | null = null;
  let showWl = false;    // 白名单配置弹窗
  let diagText = "";
  let wl: AlertWl | null = null;      // 白名单配置
  let showRecordLevel =
    typeof localStorage !== "undefined" && localStorage.getItem("tm_show_record_level") === "1";
  function setShowRecordLevel(v: boolean) {
    showRecordLevel = v;
    try { localStorage.setItem("tm_show_record_level", v ? "1" : "0"); } catch {}
  }
  let wlProcInput = "";
  let wlTargetInput = "";
  let listTimer: ReturnType<typeof setInterval> | null = null;
  let unAlertsUpdated: (() => void) | null = null;
  let installing = false; // Sysmon 一键安装进行中
  let fileInput: HTMLInputElement | null = null;

  $: ruleOn = (id: string, def: boolean) => (disabled.includes(id) ? false : def);
  $: enabledCount = RULES.filter((r) => ruleOn(r.id, r.def_on)).length;
  $: kw = q.trim().toLowerCase();
  // 显示层白名单过滤：与引擎判定同语义 —— 配置变更即时作用于历史记录
  // （新事件在引擎层已不落盘；历史数据仍存档 alerts.json，列表中即时隐藏）
  function wlMatch(r: AlertInfo, w: AlertWl | null): boolean {
    if (!w) return false;
    const src = (r.src || "").trim();
    if (src) {
      const sl = src.toLowerCase();
      if (w.sys_src && (sl.startsWith("c:\\windows\\") || sl.startsWith("\\??\\c:\\windows\\"))) return true;
      if (w.procs.some((x) => {
        const k = x.trim().toLowerCase();
        return !!k && (sl === k || sl.endsWith("\\" + k));
      })) return true;
    }
    const tgt = (r.target || "").trim().toLowerCase();
    if (tgt && w.targets.some((x) => {
      const k = x.trim().toLowerCase();
      return !!k && tgt.includes(k);
    })) return true;
    return false;
  }
  // 状态栏派生：通道订阅汇总（明细进悬停提示，保持一行精简）
  $: chanStat = st?.chans?.length
    ? { ok: st.chans.filter(([, ok]) => ok).length, total: st.chans.length }
    : null;
  $: chanDetail = st?.chans?.map(([n, ok]) => `${n}${ok ? "✓" : "✗"}`).join(" · ") ?? "";

  // 内置屏蔽（显示层同步）：目标/关键信息含 WebView2 或工具自身标识的历史记录即时隐藏
  function builtinSuppress(r: AlertInfo): boolean {
    const t = (r.target || "").toLowerCase();
    const d = (r.detail || "").toLowerCase();
    return (
      t.includes("webview2") || d.includes("webview2") ||
      t.includes("texmantis") || d.includes("texmantis")
    );
  }
  $: filtered = records
    .filter((r) => (levelSel === 0 || r.level === levelSel))
    .filter((r) => showRecordLevel || r.level !== 1)
    .filter((r) => !builtinSuppress(r))
    .filter((r) => !wlMatch(r, wl))
    .filter((r) => !kw || `${r.title} ${r.mitre} ${r.src} ${r.target} ${r.detail}`.toLowerCase().includes(kw));
  // 分页：默认每页 30 条（DOM 行数受控 —— 拖动窗口与滚动保持流畅的关键）
  const PAGE_SIZE = 30;
  let page = 1;
  let lastFilterKey = "";
  $: {
    const key = `${q}|${levelSel}`;
    if (key !== lastFilterKey) {
      lastFilterKey = key;
      page = 1;
    }
  }
  $: totalPages = Math.max(1, Math.ceil(filtered.length / PAGE_SIZE));
  $: if (page > totalPages) page = totalPages;
  $: paged = filtered.slice((page - 1) * PAGE_SIZE, page * PAGE_SIZE);

  async function refresh() {
    loading = true;
    try {
      st = await alertMonitorStatus();
      disabled = await alertRulesState();
      records = await alertsGet();
    } catch (e) {
      toast(String(e), "err");
    } finally {
      loading = false;
    }
  }
  let statusTimer: ReturnType<typeof setInterval> | null = null;
  let installPct = -1;
  let installMsg = "";
  let unProgress: (() => void) | null = null;
  onMount(() => {
    refresh();
    // 状态轻量轮询：安装 Sysmon / 引擎重连后界面自动跟进（仅页面打开期间，开销一次通道探测）
    statusTimer = setInterval(async () => {
      try { st = await alertMonitorStatus(); } catch {}
    }, 5000);
    // 白名单配置加载
    alertWlGet().then((w) => (wl = w)).catch(() => {});
    // 预警列表自动刷新（仅本页挂载期间）：引擎落盘即推送 alerts-updated 即时刷新（事件驱动为主），
    // 10 秒定时器仅兜底（补录/清空等非推送场景）；窗口失焦（appPaused）自动暂停
    listen("alerts-updated", () => {
      if (!get(appPaused)) loadRecords();
    }).then((un) => (unAlertsUpdated = un)).catch(() => {});
    // 回焦补偿：失焦期间的推送被门控跳过（事件照常落盘），回焦瞬间立即补拉一次——
    // 否则"点开工具界面是空的、手动刷新才显示"
    wasPaused = get(appPaused);
    unPausedWatch = appPaused.subscribe((v) => {
      if (wasPaused && !v) loadRecords();
      wasPaused = v;
    });
    listTimer = setInterval(() => {
      if (!get(appPaused)) loadRecords();
    }, 10000);
    // Sysmon 安装进度（真实下载百分比 + 阶段文案）
    listen<{ pct: number; msg: string }>("sysmon-progress", (e) => {
      installPct = e.payload.pct;
      installMsg = e.payload.msg;
    }).then((un) => (unProgress = un)).catch(() => {});
  });
  onDestroy(() => {
    if (statusTimer) clearInterval(statusTimer);
    if (listTimer) clearInterval(listTimer);
    unProgress?.();
    unAlertsUpdated?.();
    unPausedWatch?.();
  });

  // 一键启停：开 = 引擎启动 + 回溯补录；关 = 引擎资源真实释放
  async function toggleEngine() {
    const on = !(st?.running);
    if (!on && !(await askConfirm("停止后威胁预警将暂停，引擎资源立即释放；再次启动会自动回溯补录近期告警。确定停止？"))) return;
    try {
      const msg = await alertEngineToggle(on);
      toast(msg, "ok");
      await refresh();
    } catch (e) {
      toast(String(e), "err");
    }
  }

  async function toggleMute() {
    const next = !(st?.muted);
    try {
      await alertMuteSet(next);
      toast(next ? "气泡通知已免打扰：告警仍正常记录，可随时开启" : "气泡通知已开启", "ok");
      await refresh();
    } catch (e) {
      toast(String(e), "err");
    }
  }

  // 一键下载并安装 Sysmon（微软官方源 + 内置最小化采集配置）
  async function installSysmon() {
    if (installing) return;
    installing = true;
    toast("正在下载并安装 Sysmon（约 1-2 分钟，取决于网速）…", "info");
    try {
      const msg = await alertSysmonInstall();
      toast(msg, "ok");
    } catch (e) {
      toast(String(e), "err");
    } finally {
      installing = false;
      installPct = -1;
      installMsg = "";
      await refresh();
    }
  }

  // 导入自定义 Sysmon 配置（XML 双重校验：.NET 解析 + Sysmon 引擎自身校验）
  async function importConfig(e: Event) {
    const f = (e.currentTarget as HTMLInputElement).files?.[0];
    (e.currentTarget as HTMLInputElement).value = "";
    if (!f) return;
    let xml = "";
    try { xml = await f.text(); } catch (err) { toast(`读取文件失败：${err}`, "err"); return; }
    toast("正在校验并应用自定义配置…", "info");
    try {
      const msg = await alertImportConfig(xml);
      toast(msg, "ok");
    } catch (err) {
      toast(String(err), "err");
    }
  }

  // 一键全开/全关：仅翻转状态不同的规则（已到位的不重复请求）
  async function rulesSetAll(on: boolean) {
    const targets = RULES.filter((r) => ruleOn(r.id, r.def_on) !== on);
    if (!targets.length) {
      toast(on ? "所有规则均已启用" : "所有规则均已停用", "info");
      return;
    }
    for (const r of targets) {
      await toggleRule(r, on);
    }
  }

  async function toggleRule(r: RuleDef, on: boolean) {
    try {
      await alertRuleSet(r.id, on);
      disabled = on ? disabled.filter((x) => x !== r.id) : [...disabled, r.id];
      toast(`规则「${r.name}」已${on ? "启用" : "停用"}，立即生效`, "ok");
    } catch (e) {
      toast(String(e), "err");
    }
  }

  async function clearAll() {
    if (!(await askConfirm(`确认清空全部 ${records.length} 条告警记录？此操作不可恢复。`))) return;
    try {
      await alertsClear();
      records = [];
      toast("告警记录已清空（分日告警文件已删除）", "ok");
    } catch (e) {
      toast(String(e), "err");
    }
  }

  // 端到端自检：注入金丝雀事件走真实链路，逐段报告断点
  async function selfTest() {
    testing = true;
    try {
      toast(await alertSelftest(), "info");
    } catch (e) {
      toast(String(e), "err");
    } finally {
      testing = false;
    }
  }

  // 重新同步内置采集配置到 Sysmon（不重装，立即生效）
  async function resync() {
    try {
      toast(await alertSysmonResync(), "ok");
      await refresh();
    } catch (e) {
      toast(String(e), "err");
    }
  }

  // 轻量刷新：仅拉取告警记录（alerts-updated 推送与 3 秒兜底共用；不转圈不打扰）
  async function loadRecords() {
    try { records = await alertsGet(); } catch {}
  }

  // ----- 白名单 -----
  async function saveWl(next?: Partial<AlertWl>) {
    if (!wl) return;
    const merged: AlertWl = { sys_src: wl.sys_src, procs: wl.procs, targets: wl.targets, ...next };
    wl = merged;
    try {
      await alertWlSet(merged.sys_src, merged.procs, merged.targets);
      toast("白名单已保存：过滤即时生效（含列表中的历史命中）", "ok");
      loadRecords();
    } catch (e) {
      toast(String(e), "err");
    }
  }
  function addWlProc(v: string) {
    const x = v.trim();
    if (!x) return;
    if (wl?.procs.some((p) => p.toLowerCase() === x.toLowerCase())) { toast("该程序已在白名单中", "info"); return; }
    saveWl({ procs: [...(wl?.procs ?? []), x] });
    wlProcInput = "";
  }
  function addWlTarget(v: string) {
    const x = v.trim();
    if (!x) return;
    if (wl?.targets.some((t) => t.toLowerCase() === x.toLowerCase())) { toast("该目标已在白名单中", "info"); return; }
    saveWl({ targets: [...(wl?.targets ?? []), x] });
    wlTargetInput = "";
  }

  // ----- 顶部收纳：次级动作收进「更多」菜单（复用全局菜单组件，零新增资源） -----
  function moreMenu(e: MouseEvent) {
    openCtx(e.clientX, e.clientY, [
      { label: st?.muted ? "✓ 气泡免打扰（点击关闭）" : "□ 气泡免打扰（点击开启）", action: () => toggleMute() },
      { label: "端到端自检", action: () => { toast("自检运行中（约 7 秒，完成后弹结果）…", "info"); selfTest(); } },
      { label: "深度自检（采集链路）", action: () => runDeepCheck() },
      { label: "演示预警", action: () => testAlert() },
      { label: "导入自定义采集配置…", action: () => fileInput?.click() },
      { label: installing ? "重装 Sysmon 中…" : "重装 Sysmon（修复用）", action: () => installSysmon() },
      { label: "刷新", action: () => refresh() },
    ]);
  }

  // 深度自检：逐层核查并自动修复可修复项（结果面板展示，动作写入诊断日志）
  async function runDeepCheck() {
    showCheck = true;
    checkItems = null;
    try {
      checkItems = await deepSelfcheck();
    } catch (e) {
      showCheck = false;
      toast(String(e), "err");
    }
  }

  function openWl() {
    showWl = true;
    if (!wl) alertWlGet().then((w) => (wl = w)).catch(() => {});
  }

  // 诊断日志：查看 / 复制 / 清空
  async function loadDiag() {
    try { diagText = await diagRead(); } catch (e) { toast(String(e), "err"); }
  }
  function openDiag() {
    showDiag = true;
    loadDiag();
  }
  async function clearDiag() {
    if (!(await askConfirm("清空诊断日志？（不影响告警记录与引擎运行）"))) return;
    try {
      await diagClear();
      diagText = "";
      toast("已清空", "ok");
    } catch (e) {
      toast(String(e), "err");
    }
  }
  async function copyDiag() {
    try {
      await navigator.clipboard.writeText(diagText);
      toast("已复制到剪贴板", "ok");
    } catch {
      toast("复制失败，请手动选取文本", "err");
    }
  }

  // 演示预警：单按钮随机演示（池内 3 种不重复），走与真实告警完全相同的气泡链路（不落盘）
  async function testAlert() {
    try {
      toast(await alertTest(), "info");
    } catch (e) {
      toast(String(e), "err");
    }
  }

  const lvClass = (l: number) => (l >= 3 ? "sig bad" : l === 2 ? "sig warn" : "sig dim");
  const lvText = (l: number) => (l >= 3 ? "高风险" : l === 2 ? "中风险" : "记录");
  const fmtT = (ms: number) => new Date(ms).toLocaleString("zh-CN", { hour12: false });

  function sections(a: AlertInfo): DetailSection[] {
    return [
      { title: "告警标识", items: [
        { k: "时间", v: fmtT(a.ts_ms) }, { k: "级别", v: lvText(a.level) },
        { k: "规则", v: a.rule }, { k: "Sysmon 事件", v: `Event ID ${a.event_id}` },
        { k: "MITRE ATT&CK", v: a.mitre },
      ]},
      { title: "风险解读", items: [
        { k: "说明", v: a.desc, wide: true },
      ]},
      { title: "来源与目标", items: [
        { k: "来源进程", v: a.src || "（无）", wide: true },
        { k: "目标", v: a.target || "（无）", wide: true },
        { k: "关键信息", v: a.detail, wide: true },
      ]},
    ];
  }
</script>

<div class="page">
  <div class="toolbar">
    <h2>威胁预警</h2>
    <div class="trow">
    <span class="chip" class:ok={st?.running} class:warn={st?.mode === "waiting"}>{st?.text ?? "…"}</span>
    {#if chanStat}
      <span class="chip" class:warn={chanStat.ok !== chanStat.total}
        title="四通道推送订阅：{chanDetail}（✓ = 事件就绪；✗ = 连接中，每分钟自动重试）">
        通道 {chanStat.ok}/{chanStat.total}{chanStat.ok === chanStat.total ? " ✓" : " ⚠"}
      </span>
    {/if}
    {#if st}
      <span class="chip" class:ok={st.health === "正常"} class:warn={st.health !== "正常"} title="Sysmon 采集健康（启动 8 秒后首检，每 5 分钟巡检；异常自动尝试重应用配置）">采集：{st.health}</span>
      <span class="chip" title="运行版本 {st.version}；{st.last_alert_min < 0 ? "暂无告警记录" : st.last_alert_min === 0 ? "最近告警 1 分钟内" : `最近告警 ${st.last_alert_min} 分钟前`}">v{st.version}</span>
      {#if !st.cfg_current}<span class="chip warn" title="Sysmon 采集配置与内置规则所需不一致，重启工具自动同步">⚠ 配置待同步</span>{/if}
    {/if}
    <span class="chip" title="内置检测规则启用情况（点右侧规则库配置）">规则 {enabledCount}/{RULES.length}</span>
    {#if wl && (wl.sys_src || wl.procs.length || wl.targets.length)}
      <button type="button" class="chip wl-chip" on:click={openWl}
        title="策略生效中：命中项已在列表隐藏；引擎落盘与开机回溯补录均默认跳过。点击打开策略配置">策略生效中</button>
    {/if}
    </div>
    <div class="trow btns">
    <button class="btn" class:stop={st?.running} on:click={toggleEngine} disabled={loading}
      title={st?.running ? "停止监控：引擎资源立即释放" : "启动监控：事件驱动 · 自动回溯补录"}>
      {st?.running ? "■ 停止监控" : "▶ 启动监控"}
    </button>
    <button class="btn diag" class:open={showDiag}
      class:warn={!!st && st.health !== "正常" && st.health !== "检测中…"}
      on:click={openDiag}
      title="预警链路 debug 明细：环境快照 / 逐事件 / 命中与落盘 / 自检分段 / 健康巡检">
      {showDiag ? "● 诊断日志" : "诊断日志"}
    </button>
    <button class="btn" on:click={() => (showRules = true)}>规则库</button>
    <button class="btn" on:click={openWl} title="显示偏好与静默策略：系统来源 / 程序 / 目标加白">策略配置</button>
    <button class="btn" on:click|stopPropagation={moreMenu} title="免打扰 / 端到端自检 / 演示预警 / 导入配置 / 重装 Sysmon / 刷新">更多 ▾</button>
    </div>
  </div>

  {#if st && !st.sysmon}
    <div class="banner">
      <span style="flex:1;min-width:240px">
        本机未检测到 <b>Sysmon（微软官方 Sysinternals 组件）</b>。点击后工具将自动完成
        <b>下载 → 安装 → 按内置规则配置采集</b> 全流程（安装会在系统内注册微软官方 Sysmon 服务；
        如需彻底卸载可执行 <span class="mono">Sysmon64.exe -u force</span>）。
      </span>
      <span style="flex:1"></span>
      <button class="btn" on:click={installSysmon} disabled={installing}>
        {installing ? (installMsg || "下载安装中…（请稍候）") : "一键下载并安装 Sysmon"}
      </button>
      <button class="mini" on:click={() => fileInput?.click()} disabled={installing}>导入自定义配置</button>
    </div>
  {:else if st?.health && st.health.startsWith("异常")}
    <div class="banner">
      <span style="flex:1;min-width:240px">⚠ <b>Sysmon 采集异常</b>：{st.health}。先尝试「重新同步配置」；无效则一键重装 Sysmon（微软官方组件，重装不影响系统数据）。</span>
      <span style="flex:1"></span>
      <button class="btn" on:click={resync} disabled={loading}>重新同步配置</button>
      <button class="btn" on:click={installSysmon} disabled={installing}>{installing ? "重装中…" : "一键重装 Sysmon"}</button>
      <button class="mini" on:click={() => fileInput?.click()} disabled={installing}>导入自定义配置</button>
    </div>
  {/if}
  <input type="file" accept=".xml,text/xml" style="display:none" bind:this={fileInput} on:change={importConfig} />
  {#if installing && installPct >= 0}
    <div class="pwrap">
      <div class="pbar"><div class="pfill" style="width:{installPct}%"></div></div>
      <span class="pmsg">{installMsg}</span>
    </div>
  {/if}

  <div class="card records-card">
    <div class="card-head">
      <b>告警记录</b>
      <span class="dim" title={filtered.length !== records.length ? `列表显示 ${filtered.length} 条；总存量 ${records.length} 条（其余已被过滤）` : `共 ${records.length} 条`}>共 {filtered.length} 条</span>
      <span style="flex:1"></span>
      <button class="mini" on:click={() => (page = Math.max(1, page - 1))} disabled={page <= 1}>‹ 上一页</button>
      <span class="pager-txt">{page} / {totalPages} 页 · 命中 {filtered.length} 条</span>
      <button class="mini" on:click={() => (page = Math.min(totalPages, page + 1))} disabled={page >= totalPages}>下一页 ›</button>
      <input class="q" type="search" placeholder="搜索 规则 / MITRE / 进程 / 目标…" bind:value={q} />
      <select bind:value={levelSel}>
        <option value={0}>全部级别</option>
        <option value={3}>高风险</option>
        <option value={2}>中风险</option>
        <option value={1}>记录</option>
      </select>
      <button class="mini" on:click={clearAll} disabled={!records.length}>清空</button>
    </div>
    <div class="tbl-scroll">
    <table>
      <thead>
        <tr>
          <th style="width:13%">时间</th>
          <th style="width:6%">级别</th>
          <th style="width:8%">事件</th>
          <th style="width:30%">告警（MITRE）</th>
          <th style="width:20%">来源进程</th>
          <th>目标</th>
        </tr>
      </thead>
      <tbody>
        {#each paged as a (a.id || `${a.ts_ms}-${a.rule}-${a.src}`)}
          {@const items = [
            { label: "查看详细信息", action: () => (detail = a) },
            ...(a.src ? [{ label: `加白来源程序（不再告警）`, action: () => addWlProc(a.src) }] : []),
            ...(a.target ? [{ label: "加白目标（不再告警）", action: () => addWlTarget(a.target) }] : []),
            { label: "复制 MITRE 编号", action: () => copyText(a.mitre) },
            { label: "复制来源进程", action: () => copyText(a.src) },
            { label: "复制关键信息", action: () => copyText(a.detail) },
          ]}
          <tr on:contextmenu={(e) => rowCtx(e, items)} on:dblclick={() => (detail = a)}>
            <td class="mono">{fmtT(a.ts_ms)}</td>
            <td><span class={lvClass(a.level)}>{lvText(a.level)}</span></td>
            <td class="mono">Event {a.event_id}</td>
            <td><b>{a.title}</b> <span class="mono mitre">{a.mitre}</span></td>
            <td class="mono" title={a.src}>{a.src || "—"}</td>
            <td class="mono dim" title={a.target}>{a.target || "—"}</td>
          </tr>
        {:else}
          <tr><td colspan="6" class="dim" style="text-align:center;padding:18px 0">
            {loading ? "加载中…" : "暂无告警记录：引擎运行中，命中规则的事件会即时推送并落盘"}
          </td></tr>
        {/each}
      </tbody>
    </table>
    </div>
  </div>

  <div class="hint">
    引擎基于 <b>Windows 事件订阅</b>（事件驱动 · 零轮询），命中规则即时推送并落盘；
    数据按天存于 <span class="mono">data\alarm\</span>，保留期见「设置 → 威胁预警」，过期自动清除。
  </div>
</div>

{#if showRules}
  <div class="rules-mask" role="presentation" on:click|self={() => (showRules = false)}>
    <div class="rules-modal">
      <div class="rules-head">
        <b>规则库</b>
        <span class="dim">{enabledCount}/{RULES.length} 启用 · 勾选即时生效 · 状态保存于本机配置 · 事件驱动零轮询</span>
        <span style="flex:1"></span>
        <button class="mini" on:click={() => rulesSetAll(true)}>全部开启</button>
        <button class="mini" on:click={() => rulesSetAll(false)}>全部停用</button>
        <button class="mini" on:click={() => (showRules = false)}>关闭</button>
      </div>
      <div class="rules-legend">
        <span class="lg"><i class="lgdot" style="background:#e11d48"></i><b>命中即时弹窗</b>（高风险：气泡 + 提示音 + 落盘）</span>
        <span class="lg"><i class="lgdot" style="background:#f59e0b"></i><b>记录不弹窗</b>（中风险：仅落盘防噪音）</span>
        <span class="lg"><i class="lgdot" style="background:#94a3b8"></i><b>N次记录·M次弹窗</b>（分级升级）</span>
      </div>
      <div class="rules-body">
        <table>
          <thead>
            <tr>
              <th style="width:52px">启用</th>
              <th style="width:80px">事件 ID</th>
              <th>规则</th>
              <th style="width:76px">风险</th>
              <th style="width:150px">MITRE ATT&CK</th>
              <th style="width:150px">监控行为</th>
            </tr>
          </thead>
          <tbody>
            {#each RULES as r (r.id)}
              <tr title={r.desc}>
                <td><input type="checkbox" class="ios-sw" checked={ruleOn(r.id, r.def_on)} on:change={(e) => toggleRule(r, e.currentTarget.checked)} /></td>
                <td class="mono">Event {r.event_id}</td>
                <td><b>{r.name}</b><div class="dim sub">{r.desc}</div></td>
                <td><span class={lvClass(r.level)}>{lvText(r.level)}</span></td>
                <td class="mono mitre">{r.mitre}</td>
                <td class="dim">{r.note}</td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
      <div class="rules-foot dim">
        采集说明：引擎通过 EvtSubscribe 订阅 Sysmon 事件（事件驱动、零轮询）；Sysmon 采集范围由工具内置配置自动管理，
        也可在顶部导入自定义 Sysmon 配置（引擎规则开关仍独立生效）。
      </div>
    </div>
  </div>
{/if}

  {#if showCheck}
    <div class="rules-mask" role="presentation" on:click|self={() => (showCheck = false)}>
      <div class="rules-modal check-modal">
        <div class="rules-head">
          <b>深度自检 · 采集链路</b>
          <span class="dim">逐层核查 · 可修复项当场修复</span>
          <span style="flex:1"></span>
          <button class="mini" on:click={() => (showCheck = false)}>关闭</button>
        </div>
        <div class="rules-body check-list">
          {#if !checkItems}
            <div class="dim" style="padding: 18px 6px">自检运行中…（约 3-8 秒）</div>
          {:else}
            {#each checkItems as c (c.name)}
              <div class="check-row">
                <span class={c.fixed ? "ck-fix" : c.ok ? "ck-ok" : "ck-bad"}>{c.fixed ? "🔧 已修复" : c.ok ? "✓" : "✗"}</span>
                <div class="ck-body">
                  <b>{c.name}</b>
                  <div class="dim sub">{c.detail}</div>
                </div>
              </div>
            {/each}
            <div class="rules-foot dim">修复动作同时写入诊断日志。若「采集活性」长期为 0，说明 Sysmon 未在产出事件，请用「重装 Sysmon（修复用）」；若活性正常但仍无预警，请先确认版本（关于页）是否为最新。</div>
          {/if}
        </div>
      </div>
    </div>
  {/if}

{#if showDiag}
  <div class="rules-mask" role="presentation" on:click|self={() => (showDiag = false)}>
    <div class="rules-modal">
      <div class="rules-head">
        <b>诊断日志</b>
        <span class="dim">data\logs\diag.log · 超 1MB 自动轮转 · 复制后可直接发给开发者</span>
        <span style="flex:1"></span>
        <button class="mini" on:click={loadDiag}>刷新</button>
        <button class="mini" on:click={copyDiag}>复制全部</button>
        <button class="mini" on:click={clearDiag}>清空</button>
        <button class="mini" on:click={() => (showDiag = false)}>关闭</button>
      </div>
      <div class="rules-body">
        <pre class="diagpre">{diagText || "（暂无日志：点击「刷新」，或先运行一次「端到端自检」/ 等待健康巡检产生明细）"}</pre>
      </div>
    </div>
  </div>
{/if}
{#if showWl}
  <div class="rules-mask" role="presentation" on:click|self={() => (showWl = false)}>
    <div class="rules-modal wl-modal-box">
      <div class="rules-head">
        <b>策略配置</b>
        <span class="dim">显示偏好与静默策略 · 保存即时生效</span>
        <span style="flex:1"></span>
        <button class="mini" on:click={() => (showWl = false)}>关闭</button>
      </div>
      <div class="wl-modal">
        {#if wl}
          <!-- 分区一：列表显示（紧凑卡片行：名称与开关在同一视觉组，说明在名称下方小字） -->
          <div class="pol-sec">
            <div class="pol-head"><span class="pol-bar"></span><b>列表显示</b><span class="dim">仅影响展示，不影响落盘</span></div>
            <div class="pol-item">
              <div class="pol-txt">
                <span class="pol-name">显示低风险记录</span>
                <span class="pol-desc">「记录」级事件命中时仅落盘、不弹窗，默认隐藏；开启后在列表中显示</span>
              </div>
              <input type="checkbox" class="ios-sw" checked={showRecordLevel} on:change={(e) => setShowRecordLevel(e.currentTarget.checked)} />
            </div>
          </div>
          <div class="pol-div"></div>
          <!-- 分区二：静默策略 -->
          <div class="pol-sec">
            <div class="pol-head"><span class="pol-bar"></span><b>静默策略（白名单）</b><span class="dim">命中即静默：不告警 / 不弹气泡 / 不进记录，历史命中即时隐藏</span></div>
            <div class="pol-item">
              <div class="pol-txt">
                <span class="pol-name">系统来源加白</span>
                <span class="pol-desc">过滤来源位于 <span class="mono">C:\Windows\</span> 的进程触发的事件（系统自带工具的正常操作不再告警）</span>
              </div>
              <input type="checkbox" class="ios-sw" bind:checked={wl.sys_src} on:change={() => saveWl()} />
            </div>
            <div class="pol-sub">
              <div class="pol-subhead">程序白名单<span class="dim">完整路径或文件名 · 注册表/文件类事件不含进程信息，无法按程序加白</span></div>
              <div class="wlrow">
                <input class="wl-input" type="text" placeholder="如 D:\Tools\myapp.exe 或 myapp.exe" bind:value={wlProcInput} on:keydown={(e) => e.key === "Enter" && addWlProc(wlProcInput)} />
                <button class="mini" on:click={() => addWlProc(wlProcInput)} disabled={!wlProcInput.trim()}>添加</button>
              </div>
              <div class="wchips">
                {#each wl.procs as p (p)}
                  <span class="wchip mono" title={p}>{p.split("\\").pop() || p}<button class="wx" title="移除" on:click={() => saveWl({ procs: (wl?.procs ?? []).filter((x) => x !== p) })}>×</button></span>
                {:else}
                  <span class="dim">暂无 —— 在记录页右键「加白来源程序」可快速添加</span>
                {/each}
              </div>
            </div>
            <div class="pol-sub">
              <div class="pol-subhead">目标白名单<span class="dim">路径 / 注册表键 / 主机等关键字 · 包含匹配</span></div>
              <div class="wlrow">
                <input class="wl-input" type="text" placeholder="如 HKCU\...\Run\MyApp 或 D:\SafeFolder" bind:value={wlTargetInput} on:keydown={(e) => e.key === "Enter" && addWlTarget(wlTargetInput)} />
                <button class="mini" on:click={() => addWlTarget(wlTargetInput)} disabled={!wlTargetInput.trim()}>添加</button>
              </div>
              <div class="wchips">
                {#each wl.targets as t (t)}
                  <span class="wchip mono" title={t}>{t}<button class="wx" title="移除" on:click={() => saveWl({ targets: (wl?.targets ?? []).filter((x) => x !== t) })}>×</button></span>
                {:else}
                  <span class="dim">暂无 —— 在记录页右键「加白目标」可快速添加</span>
                {/each}
              </div>
            </div>
          </div>
        {:else}
          <div class="dim" style="padding:24px;text-align:center">加载中…</div>
        {/if}
      </div>
      <div class="rules-foot dim">内置屏蔽（固定生效，不可移除）：TexMantis.exe 自身触发的事件；目标 / 关键信息含 "WebView2" 或 "TexMantis" 的事件（运行时组件与工具自身的正常读写）。</div>
    </div>
  </div>
{/if}
<DetailModal
  open={!!detail}
  title={`告警详情${detail ? ` · ${detail.title}` : ""}`}
  sections={detail ? sections(detail) : []}
  on:close={() => (detail = null)}
/>

<style>
  .page { display: flex; flex-direction: column; gap: 12px; flex: 1; min-height: 0; }
  /* 记录卡填满剩余视口：表格内部滚动 + 吸顶表头（与其他页面 .view 模式一致，
     根除"页面下方一大段空白"——空白被收纳进卡片内部而非散落在页面上） */
  .records-card { flex: 1; min-height: 0; display: flex; flex-direction: column; }
  .tbl-scroll { flex: 1; min-height: 0; overflow: auto; }
  .records-card table { table-layout: fixed; }
  .records-card th { position: sticky; top: 0; z-index: 1; }
  .records-card th, .records-card td { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .toolbar { display: block; }
  /* 全局 .banner 有 max-height:64px 裁剪（日志横幅防爆炸）；预警页横幅含操作按钮，
     裁剪会把按钮藏进内部滚动区——解除并改为可换行 flex */
  .page .banner { max-height: none; overflow: visible; display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .pwrap { display: flex; align-items: center; gap: 10px; }
  .pbar { flex: 1; height: 8px; border-radius: 999px; background: var(--accent-bg); overflow: hidden; }
  .pfill { height: 100%; border-radius: 999px; background: var(--accent); transition: width .25s ease; }
  .pmsg { font-size: 12px; color: var(--text-dim); min-width: 120px; text-align: right; }
  .diagpre { margin: 0; padding: 12px 14px; font: 11.5px/1.55 Consolas, "Courier New", monospace; white-space: pre-wrap; word-break: break-all; user-select: text; -webkit-user-select: text; }
  /* 诊断日志按钮三态：灰=默认关闭 · 主题色=查看中 · 琥珀=采集健康异常提醒（与启停按钮同族尺寸） */
  .btn.diag { background: transparent; color: var(--text-dim); border: 1px solid var(--stroke); }
  .btn.diag:hover { color: var(--text); border-color: var(--text-dim); }
  .btn.diag.open { background: var(--accent); color: #fff; border-color: transparent; }
  .btn.diag.warn:not(.open) { color: var(--warn); border-color: var(--warn); background: var(--warn-bg); }
  /* 白名单（模态配置面板，与规则库同族） */
  .wl-modal-box { width: min(560px, 94vw); }
  .wlrow { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .wl-sec { display: flex; flex-direction: column; gap: 8px; }
  .wl-title { display: flex; align-items: baseline; gap: 10px; flex-wrap: wrap; }
  .wl-input { flex: 1; min-width: 220px; padding: 5px 9px; border: 1px solid var(--stroke); border-radius: 8px; background: var(--panel-strong); font-size: 12.5px; }
  .wchips { display: flex; flex-wrap: wrap; gap: 6px; }
  .wchip { display: inline-flex; align-items: center; gap: 4px; padding: 3px 8px; border: 1px solid var(--stroke); border-radius: 999px; font-size: 11.5px; max-width: 480px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .wx { border: none; background: transparent; color: var(--text-dim); cursor: pointer; font-size: 13px; padding: 0 2px; }
  .wx:hover { color: var(--bad); }
  /* 告警记录紧凑行距 + 分页 */
  .card table td { padding: 5px 12px; }
  .pager-txt { font-size: 11.5px; color: var(--text-dim); white-space: nowrap; }
  .wl-chip { cursor: pointer; border: none; font: inherit; }
  /* 策略配置行 */
  /* 策略配置：紧凑卡片行 + 分区节标（解决"凌乱"与"名称离开关太远"） */
  .pol-sec { display: flex; flex-direction: column; gap: 10px; }
  .pol-head { display: flex; align-items: center; gap: 8px; padding-bottom: 6px; border-bottom: 1px solid var(--stroke-soft); }
  .pol-bar { width: 4px; height: 14px; border-radius: 2px; background: var(--accent); flex: none; }
  .pol-head b { font-size: 13px; }
  .pol-item { display: flex; align-items: center; gap: 14px; padding: 10px 12px; border: 1px solid var(--stroke-soft); border-radius: 10px; background: rgba(255, 255, 255, 0.45); }
  .pol-txt { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
  .pol-name { font-weight: 650; font-size: 12.5px; color: var(--text); }
  .pol-desc { font-size: 11.5px; color: var(--text-dim); line-height: 1.55; }
  .pol-div { height: 1px; background: var(--stroke-soft); margin: 2px 0; }
  .pol-sub { display: flex; flex-direction: column; gap: 8px; padding: 10px 12px; border: 1px dashed var(--stroke-soft); border-radius: 10px; }
  .pol-subhead { font-weight: 650; font-size: 12px; display: flex; align-items: baseline; gap: 10px; flex-wrap: wrap; }
  .pol-subhead .dim { font-weight: 400; font-size: 11px; }
  .wl-modal { padding: 14px 16px; display: flex; flex-direction: column; gap: 14px; overflow: auto; font-size: 12.5px; }
  /* 深度自检结果 */
  .check-modal { max-width: 560px; }
  .check-list { display: flex; flex-direction: column; gap: 8px; }
  .check-row { display: flex; gap: 10px; align-items: flex-start; padding: 9px 12px; border: 1px solid var(--stroke-soft); border-radius: 10px; background: rgba(255, 255, 255, 0.45); }
  .ck-ok { color: var(--ok); font-weight: 700; flex: none; }
  .ck-bad { color: #b91c1c; font-weight: 700; flex: none; }
  .ck-fix { color: #b45309; font-weight: 700; flex: none; }
  .ck-body { flex: 1; }
  .ck-body .sub { margin-top: 2px; line-height: 1.5; }
  /* 规则库图例条 */
  .rules-legend { display: flex; flex-wrap: wrap; gap: 6px 18px; padding: 8px 14px; border-bottom: 1px solid var(--stroke-soft); font-size: 11.5px; color: var(--text-dim); }
  .lgdot { display: inline-block; width: 8px; height: 8px; border-radius: 50%; margin-right: 6px; vertical-align: middle; }
  .lg b { color: var(--text); font-weight: 600; }
  .trow { display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .trow.btns { justify-content: flex-end; margin-top: 8px; }
  .toolbar h2 { margin: 0 0 6px 2px; font-size: 16px; }
  .chip { font-size: 12px; padding: 3px 10px; border-radius: 999px; background: var(--accent-bg); color: var(--accent); }
  .chip.ok { background: var(--ok-bg); color: var(--ok); }
  .chip.warn { background: var(--warn-bg); color: var(--warn); }
  .btn { padding: 6px 14px; border: none; border-radius: 8px; background: var(--accent); color: #fff; cursor: pointer; font-size: 13px; white-space: nowrap; }
  .btn:disabled { opacity: 0.6; cursor: default; }
  .btn.test { background: var(--bad); }
  .btn.ghost { background: var(--warn); }
  .btn.start { background: var(--ok); }
  .btn.stop { background: #64748b; }
  .mini { padding: 4px 10px; border: 1px solid var(--stroke); border-radius: 8px; background: var(--panel-strong); cursor: pointer; font-size: 12.5px; white-space: nowrap; }
  .mini:disabled { opacity: 0.5; cursor: default; }
  .banner { font-size: 12.5px; color: var(--warn); background: var(--warn-bg); border: 1px solid transparent; border-radius: 10px; padding: 8px 12px; display: flex; align-items: center; gap: 10px; flex-wrap: wrap; }
  .banner-ok { color: var(--ok); background: var(--ok-bg); }
  .rules-mask { position: fixed; inset: 0; background: rgba(15, 23, 42, 0.42); z-index: 120; display: flex; align-items: center; justify-content: center; }
  .rules-modal { width: min(880px, 94vw); max-height: 86vh; display: flex; flex-direction: column; background: var(--panel-strong); border: 1px solid var(--stroke); border-radius: 14px; box-shadow: 0 24px 64px rgba(15, 23, 42, 0.28); overflow: hidden; }
  .rules-head { display: flex; align-items: center; gap: 10px; padding: 12px 14px; border-bottom: 1px solid var(--stroke-soft); font-size: 13.5px; }
  .rules-body { overflow: auto; }
  .rules-foot { padding: 8px 14px; border-top: 1px solid var(--stroke-soft); font-size: 11.5px; line-height: 1.6; }
  .card { flex: none; border: 1px solid var(--stroke-soft); border-radius: 12px; background: var(--panel); overflow: hidden; }
  .card-head { display: flex; align-items: center; gap: 10px; padding: 9px 12px; border-bottom: 1px solid var(--stroke-soft); font-size: 13px; }
  .card-head .q { padding: 4px 8px; border: 1px solid var(--stroke); border-radius: 8px; background: var(--panel-strong); font-size: 12.5px; width: 230px; }
  .card-head select { padding: 4px 8px; border: 1px solid var(--stroke); border-radius: 8px; background: var(--panel-strong); font-size: 12.5px; }
  table { width: 100%; border-collapse: collapse; font-size: 12.5px; }
  th { text-align: left; padding: 7px 10px; border-bottom: 1px solid var(--stroke); background: var(--panel-strong); }
  td { padding: 7px 10px; border-bottom: 1px solid var(--stroke-soft); vertical-align: top; }
  tbody tr:hover { background: var(--accent-bg); }
  .sub { font-size: 11.5px; margin-top: 2px; }
  .mitre { color: var(--accent); }
  .hint { font-size: 12px; color: var(--text-dim); line-height: 1.6; margin-top: auto; }
</style>
