// ---------------------------------------------------------------------------
// Sysmon 威胁预警引擎（事件驱动 · 零轮询）
// 通过 EvtSubscribe 订阅 Microsoft-Windows-Sysmon/Operational 通道，
// 源头 XPath 过滤 9 类高风险事件；规则命中 → 按天分文件记录 data/alarm/alarm-YYYYMMDD.json
// + 高风险右下角方框气泡告警。空闲时零 CPU、无临时文件。
// ---------------------------------------------------------------------------
use serde::{Deserialize, Serialize};

#[cfg(windows)]
#[derive(Serialize, Deserialize, Clone)]
pub struct AlertItem {
    #[serde(default)]
    pub id: String, // 全局唯一键（入库时分配）：前端 keyed-each 依赖，杜绝同毫秒同源记录键碰撞
    pub ts_ms: u64,
    pub level: u32, // 3 高 2 中 1 记录
    pub event_id: u32,
    pub rule: String,
    pub title: String,
    pub mitre: String,
    pub desc: String,
    pub src: String,
    pub target: String,
    pub detail: String,
    #[serde(default)]
    pub kind: String, // ""=威胁预警 / "social"=社交消息（气泡通道共用，样式与跳转不同）
    #[serde(default)]
    pub ctx: String,  // 社交跳转上下文："peer:UID" / "group:GID"
}

#[cfg(windows)]
const CHANNEL: &str = "Microsoft-Windows-Sysmon/Operational";
#[cfg(windows)]
const XPATH: &str = "*[System[(EventID=1 or EventID=3 or EventID=7 or EventID=8 or EventID=10 or EventID=11 or EventID=13 or EventID=19 or EventID=20 or EventID=21 or EventID=22 or EventID=25)]]";

#[cfg(windows)]
const CH_SYSMON: u8 = 0;
#[cfg(windows)]
const CH_SYSTEM: u8 = 1;
#[cfg(windows)]
const CH_TASK: u8 = 2;
#[cfg(windows)]
const CH_SEC: u8 = 3;

/// 多通道订阅（均为系统推送，事件日志服务端 XPath 过滤，未命中事件不会唤醒进程）
#[cfg(windows)]
struct ChanDef {
    name: &'static str,
    query: &'static str,
    ch: u8,
}
#[cfg(windows)]
const CHANS: [ChanDef; 4] = [
    ChanDef { name: "Microsoft-Windows-Sysmon/Operational", query: XPATH, ch: CH_SYSMON },
    ChanDef { name: "System", query: "Event[System[(EventID=7045)]]", ch: CH_SYSTEM },
    ChanDef { name: "Microsoft-Windows-TaskScheduler/Operational", query: "Event[System[(EventID=106)]]", ch: CH_TASK },
    ChanDef { name: "Security", query: "Event[System[(EventID=4625) or (EventID=4624) or (EventID=4720) or (EventID=4732)]]", ch: CH_SEC },
];

#[cfg(windows)]
struct Ctx {
    app: tauri::AppHandle,
    ch: u8,
}

#[cfg(windows)]
static SUBSCRIBED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(windows)]
static SUBS: std::sync::Mutex<Vec<(u8, windows::Win32::System::EventLog::EVT_HANDLE)>> =
    std::sync::Mutex::new(Vec::new());
#[cfg(windows)]
static RUN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(windows)]
static PENDING: std::sync::Mutex<Vec<AlertItem>> = std::sync::Mutex::new(Vec::new());
#[cfg(windows)]
static WAKE_LOCK: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
#[cfg(windows)]
static WAKE_CV: std::sync::Condvar = std::sync::Condvar::new();
#[cfg(windows)]
static STARTED_CHECK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[cfg(windows)]
static STARTED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
#[cfg(windows)]
static CACHE: std::sync::OnceLock<std::sync::Mutex<Vec<AlertItem>>> = std::sync::OnceLock::new();
#[cfg(windows)]
static DEDUPE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, u64>>> =
    std::sync::OnceLock::new();

/// 端到端自检探针状态：金丝雀事件流经真实链路时逐段点亮
#[cfg(windows)]
struct SelfTestState {
    armed_ms: u64,
    event_seen: bool, // 订阅→渲染 已到达引擎
    matched: bool,    // 规则命中
    stored: bool,     // 已落盘
    bubbled: bool,    // 气泡发送已执行
}
#[cfg(windows)]
static SELFTEST: std::sync::OnceLock<std::sync::Mutex<SelfTestState>> = std::sync::OnceLock::new();

/// Sysmon 采集健康快照缓存（后台维护线程：启动 8 秒首检，每 5 分钟巡检）
#[cfg(windows)]
static HEALTH: std::sync::OnceLock<std::sync::Mutex<Option<SysmonHealth>>> = std::sync::OnceLock::new();
#[cfg(windows)]
static HEALTH_GEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static WATCH_GEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static CB_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// ---------------------------------------------------------------------------
// 启动（幂等）：订阅线程 5 分钟重试直至 Sysmon 通道就绪（期间成本≈0）
// ---------------------------------------------------------------------------
#[cfg(windows)]
pub fn start(app: tauri::AppHandle) {
    if STARTED.swap(1, std::sync::atomic::Ordering::SeqCst) == 1 {
        return;
    }
    STARTED_CHECK.store(1, std::sync::atomic::Ordering::SeqCst);
    RUN.store(true, std::sync::atomic::Ordering::SeqCst);
    crate::commands::sysmon_sync_if_installed(); // 升级后静默重应用 Sysmon 采集配置
    crate::diag::write("I", &format!(
        "引擎启动 v{} exe_dir={:?} sysmon_dir={}",
        env!("CARGO_PKG_VERSION"),
        std::env::current_exe().ok().and_then(|p| p.parent().map(|x| x.to_path_buf())),
        crate::settings::config_path().map(|p| p.with_file_name("sysmon").display().to_string()).unwrap_or_default()
    ));
    env_snapshot();
    // 通道自检自修复：被禁用的关键事件通道自动启用（先于订阅建立）
    ensure_channels_enabled();
    // 审计策略自修复：账户管理审计被关闭则启用（否则 4720/4732 永远不产生）
    ensure_audit_policy();
    // 采集健康巡检：启动 8 秒首检，每 5 分钟复查；异常先自愈（重应用配置）再记录快照
    let gen = HEALTH_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let _ = std::thread::Builder::new().name("alert-health".into()).spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(8));
        loop {
            if !RUN.load(std::sync::atomic::Ordering::SeqCst)
                || HEALTH_GEN.load(std::sync::atomic::Ordering::SeqCst) != gen
            {
                break;
            }
            ensure_channels_enabled(); // 巡检顺带复查通道启用态
            ensure_audit_policy(); // 巡检顺带复查审计策略
            if !crate::commands::cfg_deployed_current() {
                let ok = crate::commands::apply_sysmon_config();
                crate::diag::write("I", &format!("巡检自修复：部署配置漂移 → {}", if ok { "已重写并应用" } else { "修复失败" }));
            }
            let mut h = sysmon_health();
            if !h.responds && crate::commands::apply_sysmon_config() {
                crate::diag::write("I", "健康巡检：服务无响应 → 已自动重应用内置配置");
                h = sysmon_health(); // 自愈后复测
            }
            health_store(h);
            if !RUN.load(std::sync::atomic::Ordering::SeqCst)
                || HEALTH_GEN.load(std::sync::atomic::Ordering::SeqCst) != gen
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(300));
        }
    });
    let gen = WATCH_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    let _ = std::thread::Builder::new()
        .name("sysmon-watch".into())
        .spawn(move || unsafe { watch(app, gen) });
}

/// 停止引擎并真实释放资源：关闭订阅句柄、终止重试线程、清空去重表
#[cfg(windows)]
pub fn engine_stop() {
    use windows::Win32::System::EventLog::EvtClose;
    RUN.store(false, std::sync::atomic::Ordering::SeqCst);
    SUBSCRIBED.store(false, std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut g) = SUBS.lock() {
        for (_, h) in g.drain(..) {
            unsafe {
                let _ = EvtClose(h);
            }
        }
    }
    if let Ok(mut g) = BFMAP.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new())).lock() {
        g.clear();
    }
    if let Some(m) = DEDUPE.get() {
        if let Ok(mut g) = m.lock() {
            g.clear();
        }
    }
    STARTED.swap(0, std::sync::atomic::Ordering::SeqCst);
    STARTED_CHECK.store(0, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(windows)]
unsafe fn watch(app: tauri::AppHandle, gen: usize) {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::EventLog::{
        EvtSubscribe, EVT_HANDLE, EvtSubscribeToFutureEvents,
    };
    loop {
        if WATCH_GEN.load(std::sync::atomic::Ordering::SeqCst) != gen {
            return; // 已被更新的订阅线程取代
        }
        // 逐通道订阅（已成功的跳过）；失败通道随重试循环自动补订
        for cd in CHANS.iter() {
            if SUBS.lock().map(|g| g.iter().any(|(c, _)| *c == cd.ch)).unwrap_or(true) {
                continue;
            }
            let channel: Vec<u16> = format!("{}\0", cd.name).encode_utf16().collect();
            let query: Vec<u16> = format!("{}\0", cd.query).encode_utf16().collect();
            let ctx = Box::into_raw(Box::new(Ctx { app: app.clone(), ch: cd.ch })) as *const core::ffi::c_void;
            let h = EvtSubscribe(
                EVT_HANDLE::default(),
                HANDLE::default(),
                PCWSTR(channel.as_ptr()),
                PCWSTR(query.as_ptr()),
                EVT_HANDLE::default(),
                Some(ctx),
                Some(evt_cb),
                EvtSubscribeToFutureEvents.0,
            );
            if let Ok(h) = h {
                if !h.is_invalid() {
                    if let Ok(mut g) = SUBS.lock() {
                        g.push((cd.ch, h));
                    }
                    crate::diag::write("I", &format!("订阅成功 ch={} {}", cd.ch, cd.name));
                    if cd.ch == CH_SYSMON {
                        SUBSCRIBED.store(true, std::sync::atomic::Ordering::SeqCst);
                        backfill(); // 启动回溯：补录工具未运行期间的事件
                    }
                }
            } else {
                crate::diag::write("W", &format!("订阅失败 ch={} {}（每分钟自动重试）", cd.ch, cd.name));
            }
        }
        if SUBS.lock().map(|g| g.len() >= CHANS.len()).unwrap_or(false) {
            return; // 四通道全部就绪
        }
        if !RUN.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        // 等待重试：基线 60 秒；Sysmon 安装完成会立即唤醒重连（Condvar 零轮询）
        {
            let mut g = WAKE_LOCK.lock().unwrap();
            if !*g {
                let (g2, _) = WAKE_CV
                    .wait_timeout(g, std::time::Duration::from_secs(60))
                    .unwrap();
                g = g2;
            }
            *g = false;
        }
        if !RUN.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
    }
}

/// 启动回溯：EvtQuery 逆序读取近期历史事件 → 规则引擎 → 去重补录
/// 覆盖窗口：上次最新告警之后；从未运行过则近 72 小时；最多扫描 5000 条
#[cfg(windows)]
unsafe fn backfill() {
    use windows::Win32::System::EventLog::{
        EvtClose, EvtNext, EvtQuery, EvtQueryChannelPath, EvtQueryReverseDirection, EVT_HANDLE,
    };
    let last_ms = load_all().iter().map(|a| a.ts_ms).max().unwrap_or(0);
    let floor = if last_ms > 0 {
        last_ms
    } else {
        now_ms().saturating_sub(72 * 3_600_000)
    };
    let chan: Vec<u16> = format!("{CHANNEL}\0").encode_utf16().collect();
    let qh = match EvtQuery(
        EVT_HANDLE::default(),
        windows::core::PCWSTR(chan.as_ptr()),
        windows::core::PCWSTR::null(),
        (EvtQueryChannelPath.0 | EvtQueryReverseDirection.0),
    ) {
        Ok(h) => h,
        Err(_) => return,
    };
    let mut new_count = 0usize;
    let mut scanned = 0usize;
    let mut stop = false;
    let mut buf = [0isize; 32];
    while !stop {
        let mut n = 0u32;
        if EvtNext(qh, &mut buf, 1000, 0, &mut n).is_err() || n == 0 {
            break;
        }
        for i in 0..n as usize {
            scanned += 1;
            let h = EVT_HANDLE(buf[i]);
            if let Some(xml) = render_xml(h) {
                if let Some(ev) = parse_event(&xml) {
                    if ev.time_ms != 0 && ev.time_ms < floor {
                        stop = true; // 逆序：已越过补录窗口
                    } else if let Some(a) = evaluate(&xml) {
                        // 白名单与实时回调同语义：回溯补录同样默认不记录（配置前后行为一致）
                        if whitelisted(&a) {
                            crate::diag::write("D", &format!("回溯补录白名单跳过 rule={} src={} 目标={}", a.rule, a.src, a.target));
                        } else if store(&a) {
                            new_count += 1;
                        }
                    }
                }
            }
            let _ = EvtClose(h);
            if stop || scanned >= 5000 {
                stop = true;
                break;
            }
        }
    }
    let _ = EvtClose(qh);
    if new_count > 0 {
        crate::settings::app_notify(&format!("威胁预警回溯完成：补录 {new_count} 条告警记录"));
    }
}

#[cfg(windows)]
unsafe extern "system" fn evt_cb(
    action: windows::Win32::System::EventLog::EVT_SUBSCRIBE_NOTIFY_ACTION,
    ctx: *const core::ffi::c_void,
    event: windows::Win32::System::EventLog::EVT_HANDLE,
) -> u32 {
    use tauri::Emitter;
    use windows::Win32::System::EventLog::{EvtRender, EvtRenderEventXml, EvtSubscribeActionDeliver};
    if action != EvtSubscribeActionDeliver || event.is_invalid() {
        return 1;
    }
    // 引擎启停 + 监控开关实时读取：立即停止记录与弹窗
    if !RUN.load(std::sync::atomic::Ordering::SeqCst) || !crate::settings::config().alert_monitor {
        return 1;
    }
    crate::social::socdbg(&format!("evt-callback: 事件进入 ch={}", unsafe { &*(ctx as *const Ctx) }.ch));
    let c = &*(ctx as *const Ctx);
    let cb_n = CB_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    let verbose = cb_n <= 30 || cb_n % 500 == 0;
    if verbose {
        crate::diag::write("D", &format!("回调触发 #{} ch={}", cb_n, c.ch));
    }
    let Some(xml) = render_xml(event) else {
        if verbose {
            crate::diag::write("W", &format!("事件渲染失败 ch={}（回调 #{cb_n}）", c.ch));
        }
        return 1;
    };
    // 端到端自检打点：金丝雀事件流经真实链路时逐段记录
    let canary = xml.contains("TexMantisSelfTest");
    if canary {
        if let Some(m) = SELFTEST.get() {
            if let Ok(mut g) = m.lock() {
                if g.armed_ms > 0 { g.event_seen = true; }
            }
        }
    }
    // 诊断日志：每事件一行明细；金丝雀事件记录完整 XML（截断）
    {
        let eid = parse_event(&xml).map(|e| e.event_id).unwrap_or(0);
        if canary {
            crate::diag::write("I", &format!(
                "金丝雀事件到达引擎 ch={} eid={} xml={}",
                c.ch, eid, xml.chars().take(700).collect::<String>()
            ));
        } else {
            crate::diag::write("D", &format!("EVT ch={} eid={}", c.ch, eid));
        }
    }
    let a = match c.ch {
        CH_SYSTEM => eval_7045(&xml),
        CH_TASK => eval_106(&xml),
        CH_SEC => bruteforce(&xml),
        _ => evaluate(&xml),
    };
    if let Some(a) = a {
        // 金丝雀自检事件豁免一切屏蔽/白名单（自检必须走通 订阅→解析→命中→落盘 全链路）
        if !canary && whitelisted(&a) {
            crate::diag::write("D", &format!("白名单命中跳过 rule={} src={} 目标={}", a.rule, a.src, a.target));
            return 1;
        }
        crate::diag::write("I", &format!("命中规则 {} level={} 目标={}", a.rule, a.level, a.target));
        if canary && a.rule == "E13_RUNKEY" {
            if let Some(m) = SELFTEST.get() {
                if let Ok(mut g) = m.lock() { g.matched = true; }
            }
        }
        let dup = !dedupe_ok(&a);
        let stored = if dup { false } else { store(&a) };
        if dup {
            crate::diag::write("D", &format!("命中但去重跳过 rule={} 目标={}", a.rule, a.target));
        } else if !stored {
            crate::diag::write("W", &format!("落盘失败 rule={} 目标={}", a.rule, a.target));
        }
        if stored {
            if canary {
                if let Some(m) = SELFTEST.get() {
                    if let Ok(mut g) = m.lock() { g.stored = true; }
                }
            }
            if a.level >= 3 {
                notify_bubble(&c.app, &a); // emit + 待发队列 + 系统右下角气泡（免打扰时静默）
                if canary {
                    if let Some(m) = SELFTEST.get() {
                        if let Ok(mut g) = m.lock() { g.bubbled = true; }
                    }
                }
            }
        }
    }
    1 // TRUE：继续订阅
}

/// 渲染事件 XML（两次调用定型缓冲）
#[cfg(windows)]
unsafe fn render_xml(event: windows::Win32::System::EventLog::EVT_HANDLE) -> Option<String> {
    use windows::Win32::System::EventLog::{EvtRender, EvtRenderEventXml};
    let mut used = 0u32;
    let mut pc = 0u32;
    // 两段式调用：第一段以空缓冲探测所需字节数——API 以 ERROR_INSUFFICIENT_BUFFER
    // 失败属预期（used 同时带回所需大小），绝不能把这次失败当真错误返回
    let probe = EvtRender(
        windows::Win32::System::EventLog::EVT_HANDLE::default(),
        event,
        EvtRenderEventXml.0,
        0,
        None,
        &mut used,
        &mut pc,
    );
    if probe.is_err() && used == 0 {
        return None;
    }
    if used == 0 {
        return None;
    }
    let mut buf = vec![0u8; used as usize];
    EvtRender(
        windows::Win32::System::EventLog::EVT_HANDLE::default(),
        event,
        EvtRenderEventXml.0,
        used,
        Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
        &mut used,
        &mut pc,
    )
    .ok()?;
    let u16s: Vec<u16> = buf
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let end = u16s.iter().position(|&c| c == 0).unwrap_or(u16s.len());
    Some(String::from_utf16_lossy(&u16s[..end]))
}

// ---------------------------------------------------------------------------
// Sysmon XML 极简解析（无第三方依赖）：EventID + <Data Name=...> 值对（兼容单/双引号属性）
// ---------------------------------------------------------------------------
#[cfg(windows)]
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

#[cfg(windows)]
struct RawEvent {
    event_id: u32,
    time_ms: u64,
    data: std::collections::HashMap<String, String>,
}

#[cfg(windows)]
fn parse_event(xml: &str) -> Option<RawEvent> {
    let eid_at = xml.find("<EventID")?;
    let gt = xml.get(eid_at..)?.find('>')? + eid_at; // gt 指向 '>'
    let end = xml.get(gt..)?.find('<')? + gt;
    let event_id: u32 = xml.get(gt + 1..end)?.trim().parse().ok()?;
    // <TimeCreated SystemTime="2026-09-09T13:45:30.1234567Z" />
    // SystemTime='2026-09-09T13:45:30.1234567Z'（单/双引号均兼容）
    let time_ms = xml
        .find("SystemTime=")
        .and_then(|i| {
            // i+11 跳过 "SystemTime="，i+12 再跳过开头引号 → 从值首字符起找第一个引号即闭合
            let rest = xml.get(i + 12..)?;
            let close = rest.find(['\'', '"'])?;
            Some(iso_to_ms(rest.get(..close)?))
        })
        .unwrap_or(0);
    let mut data = std::collections::HashMap::new();
    let mut rest = xml;
    // 属性引号兼容：EvtRender 渲染的事件 XML 属性为单引号（<Data Name='X'>），
    // 部分工具链/系统组件输出双引号——两种都必须识别，否则 Data 映射恒空、规则全盲
    while let Some(i) = rest.find("<Data Name=") {
        rest = &rest[i + 11..];
        let q = match rest.chars().next() {
            Some(c @ ('\'' | '"')) => c,
            _ => { rest = &rest[1..]; continue; } // 非预期格式：前进一位避免死循环
        };
        let e = rest.get(1..)?.find(q as char)? + 1;
        let name = rest.get(1..e)?.to_string();
        rest = &rest[e + 1..];
        let gt = rest.find('>')?;
        let (val, consumed);
        if rest.as_bytes().get(gt.wrapping_sub(1)) == Some(&b'/') {
            val = "";
            consumed = gt + 1;
        } else {
            let close = rest.get(gt..)?.find("</Data>")?;
            val = rest.get(gt + 1..gt + close)?;
            consumed = gt + close + 7;
        }
        data.insert(name, unescape(val));
        rest = &rest[consumed..];
    }
    Some(RawEvent { event_id, time_ms, data })
}

#[cfg(windows)]
fn iso_to_ms(s: &str) -> u64 {
    fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
        let y = if m <= 2 { y - 1 } else { y };
        let era = if y >= 0 { y } else { y - 399 } / 400;
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146097 + doe - 719468
    }
    let b = s.as_bytes();
    if b.len() < 19 {
        return 0;
    }
    let num = |a: usize, z: usize| -> i64 { s.get(a..z).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0) };
    let secs = days_from_civil(num(0, 4), num(5, 7), num(8, 10)) * 86400
        + num(11, 13) * 3600
        + num(14, 16) * 60
        + num(17, 19);
    secs.max(0) as u64 * 1000
}

#[cfg(windows)]
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// 内置规则引擎（防噪音组合逻辑，SIEM 思路）
// ---------------------------------------------------------------------------
#[cfg(windows)]
const fn ends_ci(s: &str, suf: &str) -> bool {
    // const fn 不便使用，改用普通函数
    let _ = (s, suf);
    false
}

#[cfg(windows)]
fn ends_with_ci(s: &str, suf: &str) -> bool {
    s.len() >= suf.len() && s[s.len() - suf.len()..].to_lowercase() == suf.to_lowercase()
}

#[cfg(windows)]
fn contains_ci(s: &str, needle: &str) -> bool {
    s.to_lowercase().contains(&needle.to_lowercase())
}

#[cfg(windows)]
const WINDOWS_DIR: &str = "\\windows\\";

#[cfg(windows)]
const COMMON_PORTS: &[u32] = &[
    21, 22, 23, 25, 53, 67, 68, 69, 80, 88, 110, 123, 135, 137, 138, 139, 143, 161, 162, 389, 443,
    445, 464, 514, 587, 636, 873, 853, 993, 995, 1900, 3268, 3269, 3389, 5228, 5353, 5985, 5986,
    8080, 8443,
];

#[cfg(windows)]
fn mk(level: u32, eid: u32, rule: &str, title: &str, mitre: &str, desc: &str, src: &str, target: &str, detail: &str, t: u64) -> AlertItem {
    AlertItem {
        id: String::new(),
        ts_ms: t,
        level,
        event_id: eid,
        rule: rule.into(),
        title: title.into(),
        mitre: mitre.into(),
        desc: desc.into(),
        src: src.into(),
        target: target.into(),
        detail: detail.into(),
        kind: String::new(),
        ctx: String::new(),
    }
}

#[cfg(windows)]
fn rule_enabled(id: &str) -> bool {
    !crate::settings::config().disabled_rules.iter().any(|x| x == id)
}

#[cfg(windows)]
fn evaluate(xml: &str) -> Option<AlertItem> {
    let ev = parse_event(xml)?;
    let d = &ev.data;
    let get = |k: &str| d.get(k).cloned().unwrap_or_default();
    let trunc = |s: &str| if s.chars().count() > 220 { s.chars().take(220).collect::<String>() + "…" } else { s.to_string() };
    let t = if ev.time_ms > 0 { ev.time_ms } else { now_ms() };
    let a = match ev.event_id {
        1 => {
            let image = get("Image");
            let cmd = get("CommandLine");
            let parent = get("ParentImage");
            let exe = image.rsplit('\\').next().unwrap_or("").to_lowercase();
            if (exe == "powershell.exe" || exe == "pwsh.exe")
                && (contains_ci(&cmd, "-encodedcommand")
                    || contains_ci(&cmd, "-enc ")
                    || cmd.to_lowercase().contains(" -e ")
                    || contains_ci(&cmd, "frombase64string"))
                && rule_enabled("E1_ENC_PS")
            {
                Some(mk(3, 1, "E1_ENC_PS", "编码 PowerShell 执行", "T1059.001",
                    "PowerShell 使用编码参数运行命令，是无文件攻击与载荷投递的常用手法",
                    &image, "powershell", &format!("命令行：{}", trunc(&cmd)), t))
            } else {
                const PARENTS: &[&str] = &["winword.exe", "excel.exe", "powerpnt.exe", "outlook.exe", "msaccess.exe", "wps.exe", "wpp.exe", "et.exe"];
                const BAD_KIDS: &[&str] = &["cmd.exe", "powershell.exe", "pwsh.exe", "wscript.exe", "cscript.exe", "mshta.exe", "rundll32.exe", "regsvr32.exe", "certutil.exe", "bitsadmin.exe", "msiexec.exe"];
                let pe = parent.rsplit('\\').next().unwrap_or("").to_lowercase();
                if PARENTS.iter().any(|p| pe == *p)
                    && BAD_KIDS.iter().any(|k| exe == *k)
                    && rule_enabled("E1_OFFICE_CHILD")
                {
                    Some(mk(3, 1, "E1_OFFICE_CHILD", "Office 派生可疑子进程", "T1566.001",
                        "办公软件派生命令行/脚本解释器，是钓鱼附件载荷执行的典型标志",
                        &parent, &image, &format!("命令行：{}", trunc(&cmd)), t))
                } else {
                    None
                }
            }
        }
        3 => {
            let port: u32 = get("DestinationPort").parse().unwrap_or(0);
            let ip = get("DestinationIp");
            let image = get("Image");
            if port > 0 && !COMMON_PORTS.contains(&port) && !ip.is_empty() && rule_enabled("E3_BADPORT") {
                Some(mk(2, 3, "E3_BADPORT", "非常规端口外联", "T1071.001 / T1571",
                    "进程向非常用端口发起外联，可能是 C2 回连、数据外排或横向移动",
                    &image, &format!("{ip}:{port}"), &format!("目的 {}:{} · 协议 {}", ip, port, get("Protocol")), t))
            } else {
                None
            }
        }
        7 => {
            let dll = get("ImageLoaded");
            if contains_ci(&dll, "\\temp\\")
                && (ends_with_ci(&dll, ".dll") || ends_with_ci(&dll, ".ocx"))
                && rule_enabled("E7_TEMP_DLL")
            {
                Some(mk(2, 7, "E7_TEMP_DLL", "临时目录加载 DLL", "T1574.001 / T1574.002",
                    "从 Temp 临时目录加载 DLL/OCX 模块，常见于 DLL 劫持与恶意模块加载",
                    &get("Image"), &dll, &format!("签名状态：{}", if get("Signed").to_lowercase() == "true" { format!("已签名（{}）", get("Signature")) } else { "未签名".to_string() }), t))
            } else {
                None
            }
        }
        8 => {
            let src = get("SourceImage");
            let tgt = get("TargetImage");
            if contains_ci(&src, WINDOWS_DIR) {
                None // 系统目录来源：噪音，忽略
            } else if rule_enabled("E8_REMOTE_THREAD") {
                Some(mk(3, 8, "E8_REMOTE_THREAD", "跨进程远程线程注入", "T1055.001 / T1055.002",
                    "进程在其他进程内存空间创建远程线程，是代码注入与隐蔽执行的强指标",
                    &src, &tgt, &format!("{} → {}（入口 {}）", src, tgt, if get("StartFunction").is_empty() { get("StartModule") } else { get("StartFunction") }), t))
            } else {
                None
            }
        }
        10 => {
            let tgt = get("TargetImage");
            if !ends_with_ci(&tgt, "\\lsass.exe") {
                None
            } else {
                let access = get("GrantedAccess").to_lowercase();
                const MASKS: &[&str] = &["0x1410", "0x1010", "0x143a", "0x1430", "0x1f0fff", "0x1fffff", "0x1410"];
                let src = get("SourceImage");
                if MASKS.contains(&access.as_str()) && rule_enabled("E10_LSASS") {
                    if contains_ci(&src, WINDOWS_DIR) {
                        Some(mk(1, 10, "E10_LSASS", "LSASS 内存读取（系统来源）", "T1003.001",
                            "系统目录进程读取 LSASS 内存，通常为合法组件，已降级记录",
                            &src, &tgt, &format!("GrantedAccess {access} · 来源 {}", src), t))
                    } else {
                        Some(mk(3, 10, "E10_LSASS", "LSASS 内存读取（凭据转储）", "T1003.001",
                            "非系统进程以内存读取权限访问 LSASS，典型 Mimikatz/ProcDump 凭据窃取行为",
                            &src, &tgt, &format!("GrantedAccess {access} · 来源 {}", src), t))
                    }
                } else {
                    None
                }
            }
        }
        11 => {
            let f = get("TargetFilename");
            if contains_ci(&f, "\\startup\\") && rule_enabled("E11_STARTUP") {
                Some(mk(3, 11, "E11_STARTUP", "启动目录写入（持久化）", "T1105 / T1547.001",
                    "向启动目录写入文件，开机自启持久化的直接信号",
                    &get("Image"), &f, &format!("落盘文件：{f}"), t))
            } else if contains_ci(&f, "\\temp\\")
                && [".exe", ".dll", ".ps1", ".scr", ".vbs", ".js", ".bat", ".cmd", ".hta", ".jar"]
                    .iter()
                    .any(|e| ends_with_ci(&f, e))
                && rule_enabled("E11_TEMP_DROP")
            {
                Some(mk(2, 11, "E11_TEMP_DROP", "临时目录可执行文件落盘", "T1105",
                    "Temp 目录创建可执行/脚本文件，常见于载荷释放与勒索前期投递",
                    &get("Image"), &f, &format!("落盘文件：{f}"), t))
            } else {
                None
            }
        }
        13 => {
            let obj = get("TargetObject");
            if contains_ci(&obj, "currentversion\\run") && rule_enabled("E13_RUNKEY") {
                Some(mk(3, 13, "E13_RUNKEY", "注册表 Run 键持久化", "T1547.001",
                    "写入 CurrentVersion\\Run(-Once) 自启键值，注册表持久化最常见位置",
                    &get("Image"), &obj, &format!("目标：{obj} · 值：{}", get("Details")), t))
            } else if (contains_ci(&obj, "disableantispyware") || contains_ci(&obj, "microsoft\\windows defender")) && rule_enabled("E13_DEFENSE") {
                Some(mk(3, 13, "E13_DEFENSE", "安全防御削弱", "T1562.001",
                    "修改 Defender 相关策略键值，试图关闭系统防护",
                    &get("Image"), &obj, &format!("目标：{obj} · 值：{}", get("Details")), t))
            } else {
                None
            }
        }
        22 => {
            let q = get("QueryName");
            if !q.is_empty() && rule_enabled("E22_DNS") {
                Some(mk(2, 22, "E22_DNS", "DNS 查询记录", "T1071.004 / T1568.002",
                    "进程发起 DNS 解析（全量记录，量大，默认关闭，按需观察）",
                    &get("Image"), &q, &format!("查询 {q} · 结果：{}", trunc(&get("QueryResults"))), t))
            } else {
                None
            }
        }
        25 => {
            if rule_enabled("E25_TAMPER") {
                Some(mk(3, 25, "E25_TAMPER", "进程映像篡改（Hollowing）", "T1055.012",
                    "检测到进程映像被替换/篡改，Process Hollowing 等高级规避技术指标",
                    &get("Image"), &get("TargetObject"), &format!("进程 {}", get("Image")), t))
            } else {
                None
            }
        }
        _ => None,
    };
    if a.is_some() { a } else { red_evaluate(&ev, t) }
}

// ---------------------------------------------------------------------------
// 红队规则库：来自常见渗透 TTP 的命令行/注册表行为（数据驱动，精确低噪）
// ---------------------------------------------------------------------------
#[cfg(windows)]
struct RedRule {
    id: &'static str,
    eid: u32,
    level: u32,
    title: &'static str,
    mitre: &'static str,
    desc: &'static str,
    all: &'static [&'static str], // AND 条件（全含）
    any: &'static [&'static str], // OR 条件（任含，空 = 不限）
    tgt: bool,                    // false: E1 CommandLine / true: E13 TargetObject
}

#[cfg(windows)]
static RED_RULES: &[RedRule] = &[
    RedRule { id: "RT_CRED_DUMP", eid: 1, level: 3, title: "凭据转储工具执行", mitre: "T1003.001 / T1003.003",
        desc: "命令行命中 mimikatz/sekurlsa/lsadump 等凭据转储特征",
        all: &[], any: &["mimikatz", "sekurlsa::", "lsadump::"], tgt: false },
    RedRule { id: "RT_COMSVCS", eid: 1, level: 3, title: "comsvcs.dll 转储 LSASS", mitre: "T1003.001",
        desc: "rundll32 调用 comsvcs.dll MiniDump 导出 LSASS 内存（免杀常用转储手法）",
        all: &["comsvcs", "minidump"], any: &[], tgt: false },
    RedRule { id: "RT_POTATO", eid: 1, level: 3, title: "Potato 家族提权利用", mitre: "T1068",
        desc: "JuicyPotato/GodPotato/PrintSpoofer 等系统级本地提权工具执行",
        all: &[], any: &["juicypotato", "godpotato", "printspoofer", "roguepotato", "sweetpotato", "efspotato", "coercedpotato", "genericpotato"], tgt: false },
    RedRule { id: "RT_PRIVESC", eid: 1, level: 2, title: "提权枚举工具", mitre: "T1068",
        desc: "PowerUp/Sherlock/BeRoot/SharpUp 等提权枚举脚本或工具",
        all: &[], any: &["powerup.ps", "sherlock.ps", "beroot", "sharpup", "watson.exe"], tgt: false },
    RedRule { id: "RT_USER_ADD", eid: 1, level: 3, title: "新建本地账户", mitre: "T1136.001",
        desc: "net user ... /add 创建账户（持久化/后门的常见前置动作）",
        all: &["net user", "/add"], any: &[], tgt: false },
    RedRule { id: "RT_GROUP_ADD", eid: 1, level: 3, title: "账户加入管理员组", mitre: "T1078.002",
        desc: "net localgroup administrators ... /add 提升账户权限",
        all: &["net localgroup", "/add"], any: &[], tgt: false },
    RedRule { id: "RT_TOKEN", eid: 1, level: 3, title: "令牌窃取/模拟", mitre: "T1134.001",
        desc: "Invoke-TokenManipulation/incognito 等令牌操控工具",
        all: &[], any: &["invoketokenmanipulation", "tokenmanipulation", "incognito"], tgt: false },
    RedRule { id: "RT_NTDS", eid: 1, level: 3, title: "AD 数据库转储", mitre: "T1003.003",
        desc: "ntdsutil/ntds.dit 相关命令（域内凭据大规模窃取）",
        all: &[], any: &["ntdsutil", "ntds.dit"], tgt: false },
    RedRule { id: "RT_RECOV", eid: 1, level: 3, title: "破坏系统恢复能力", mitre: "T1490",
        desc: "删除卷影副本/禁用系统恢复（勒索与毁痕的典型前置）",
        all: &[], any: &["vssadmin delete shadows", "wmic shadowcopy delete", "wbadmin delete catalog", "recoveryenabled no"], tgt: false },
    RedRule { id: "RT_PS_CRADLE", eid: 1, level: 3, title: "PowerShell 远程载荷执行", mitre: "T1059.001 / T1105",
        desc: "IEX/DownloadString 等下载擂（cradle）直接内存执行远程代码",
        all: &["http"], any: &["iex", "invoke-expression", "downloadstring", "downloadfile", "invoke-webrequest", "invoke-restmethod"], tgt: false },
    RedRule { id: "RT_CERTUTIL", eid: 1, level: 2, title: "certutil 远程下载", mitre: "T1105",
        desc: "certutil -urlcache/-verifyctl 下载文件（LOBin 滥用）",
        all: &["certutil"], any: &["urlcache", "verifyctl"], tgt: false },
    RedRule { id: "RT_BITSADMIN", eid: 1, level: 2, title: "BITS 后台传输", mitre: "T1197",
        desc: "bitsadmin /transfer 等后台下载（绕过常规下载监控）",
        all: &["bitsadmin"], any: &["/transfer", "/addfile", "setnotifycmdline"], tgt: false },
    RedRule { id: "RT_MSHTA", eid: 1, level: 3, title: "mshta 脚本载荷", mitre: "T1218.005",
        desc: "mshta 执行 http/vbscript/javascript 载荷（白程序滥用）",
        all: &["mshta"], any: &["http", "vbscript:", "javascript:"], tgt: false },
    RedRule { id: "RT_REGSVR32", eid: 1, level: 2, title: "regsvr32 远程脚本", mitre: "T1218.010",
        desc: "regsvr32 /i:http 加载 scrobj（squiblydoo 白程序绕过）",
        all: &["regsvr32"], any: &["scrobj", "/i:http"], tgt: false },
    RedRule { id: "RT_HIDDEN_JS", eid: 1, level: 2, title: "隐藏窗口脚本执行", mitre: "T1059.005",
        desc: "wscript/cscript //b 无窗口静默执行脚本",
        all: &[], any: &["wscript //b", "wscript.exe //b", "cscript //b", "cscript.exe //b"], tgt: false },
    RedRule { id: "RT_DEF_PS", eid: 1, level: 3, title: "PowerShell 关闭 Defender/加排除", mitre: "T1562.001",
        desc: "Set-MpPreference 关闭实时防护或添加查杀排除项",
        all: &["mppreference"], any: &["-disablerealtimemonitoring", "-disablebehaviormonitoring", "-disableioavprotection", "-disableintrusionpreventionsystem", "-exclusionpath", "-exclusionprocess", "-exclusionextension"], tgt: false },
    RedRule { id: "RT_FW_OFF", eid: 1, level: 2, title: "关闭防火墙", mitre: "T1562.004",
        desc: "netsh advfirewall 关闭防火墙配置文件",
        all: &["netsh advfirewall"], any: &["state off"], tgt: false },
    RedRule { id: "RT_SVC_SC", eid: 1, level: 2, title: "服务创建命令", mitre: "T1543.003",
        desc: "sc create 注册系统服务（持久化/提权载体，实际安装另由 7045 捕获）",
        all: &["sc create"], any: &[], tgt: false },
    RedRule { id: "RT_SVC_CFG", eid: 1, level: 2, title: "服务 binPath 修改", mitre: "T1543.003",
        desc: "sc config 修改服务二进制路径（服务劫持提权）",
        all: &["sc config", "binpath"], any: &[], tgt: false },
    RedRule { id: "RT_SCHTASKS", eid: 1, level: 2, title: "计划任务创建命令", mitre: "T1053.005",
        desc: "schtasks /create；指定 /ru SYSTEM 时升级为高风险弹窗",
        all: &["schtasks"], any: &["/create"], tgt: false },
    RedRule { id: "RT_IFEO", eid: 13, level: 3, title: "IFEO 镜像劫持", mitre: "T1546.012",
        desc: "Image File Execution Options 写入 Debugger（劫持任意程序启动）",
        all: &["image file execution options"], any: &["debugger"], tgt: true },
    RedRule { id: "RT_WINLOGON", eid: 13, level: 3, title: "Winlogon 登录劫持", mitre: "T1547.004",
        desc: "Winlogon 的 Shell/Userinit/Notify 被修改（登录链持久化）",
        all: &["\\winlogon\\"], any: &["shell", "userinit", "notify"], tgt: true },
    RedRule { id: "RT_APPINIT", eid: 13, level: 3, title: "AppInit_DLLs 全局注入", mitre: "T1546.010",
        desc: "AppInit_DLLs/AppInit_X64_DLLs 被写入（全局 DLL 注入）",
        all: &[], any: &["appinit_dlls", "appinit_x64_dlls"], tgt: true },
    RedRule { id: "RT_BROWSER_FORCE", eid: 13, level: 3, title: "浏览器扩展强制安装", mitre: "T1176",
        desc: "ExtensionInstallForcelist 被写入（强推浏览器扩展/劫持）",
        all: &["extensioninstallforcelist"], any: &[], tgt: true },
    RedRule { id: "RT_BROWSER_HIJACK", eid: 13, level: 2, title: "浏览器主页/搜索劫持", mitre: "T1176",
        desc: "浏览器主页/启动页/默认搜索提供程序被修改",
        all: &[], any: &["defaultsearchprovider", "restoreonstartup", "startuppages", "homepage", "searchscopes", "start page"], tgt: true },
];

#[cfg(windows)]
fn red_evaluate(ev: &RawEvent, t: u64) -> Option<AlertItem> {
    let get = |k: &str| ev.data.get(k).cloned().unwrap_or_default();
    let trunc = |s: &str| if s.chars().count() > 220 { s.chars().take(220).collect::<String>() + "…" } else { s.to_string() };
    match ev.event_id {
        1 => {
            let cmd = get("CommandLine");
            let image = get("Image");
            for r in RED_RULES.iter().filter(|r| r.eid == 1) {
                if !rule_enabled(r.id) { continue; }
                if r.all.iter().all(|p| contains_ci(&cmd, p)) && (r.any.is_empty() || r.any.iter().any(|p| contains_ci(&cmd, p))) {
                    let (level, title) = if r.id == "RT_SCHTASKS" && contains_ci(&cmd, "/ru system") {
                        (3, "计划任务创建（SYSTEM 权限）")
                    } else {
                        (r.level, r.title)
                    };
                    // 风险解读嵌入实体：账户/组/任务类命令行告警一眼可研判
                    let desc = match r.id {
                        "RT_USER_ADD" => format!("{}——本次新建的账户见下方命令行", r.desc),
                        "RT_GROUP_ADD" => format!("{}——被加入的组与账户见下方命令行", r.desc),
                        "RT_SCHTASKS" => format!("{}——任务与动作见下方命令行", r.desc),
                        _ => r.desc.to_string(),
                    };
                    return Some(mk(level, 1, r.id, title, r.mitre, &desc, &image, &get("ParentImage"), &trunc(&cmd), t));
                }
            }
            None
        }
        13 => {
            let obj = get("TargetObject");
            let image = get("Image");
            for r in RED_RULES.iter().filter(|r| r.eid == 13) {
                if !rule_enabled(r.id) { continue; }
                if r.all.iter().all(|p| contains_ci(&obj, p)) && (r.any.is_empty() || r.any.iter().any(|p| contains_ci(&obj, p))) {
                    return Some(mk(r.level, 13, r.id, r.title, r.mitre, r.desc, &image, &obj, &trunc(&format!("目标：{obj} · 值：{}", get("Details"))), t));
                }
            }
            None
        }
        19 | 20 | 21 => {
            if !rule_enabled("RT_WMI") { return None; }
            let name = get("Name");
            let extra = ["Query", "Destination", "Consumer", "Filter", "EventNamespace", "Type"]
                .iter().map(|k| get(k)).filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ");
            let (kind, image) = match ev.event_id {
                19 => ("WMI 事件筛选器", get("Image")),
                20 => ("WMI 事件消费者", get("Image")),
                _ => ("WMI 筛选器-消费者绑定", get("Image")),
            };
            Some(mk(3, ev.event_id, "RT_WMI", "WMI 无文件持久化", "T1546.005",
                &format!("注册 {kind}（无文件落地、常规查杀不可见的持久化手法）"),
                &image, &name, &trunc(&format!("{name} · {extra}")), t))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 系统服务安装（7045）/ 计划任务注册（106）
// ---------------------------------------------------------------------------
#[cfg(windows)]
fn eval_7045(xml: &str) -> Option<AlertItem> {
    if !rule_enabled("RT_SVC7045") { return None; }
    let ev = parse_event(xml)?;
    if ev.event_id != 7045 { return None; }
    let t = if ev.time_ms > 0 { ev.time_ms } else { now_ms() };
    let get = |k: &str| ev.data.get(k).cloned().unwrap_or_default();
    let name = get("ServiceName");
    let img = get("ImagePath");
    let lower = img.to_lowercase();
    let sus = ["\\temp\\", "\\appdata\\", "\\users\\", "powershell", "cmd.exe", "/c ", "wscript", "mshta", "certutil", "-enc"]
        .iter().any(|p| lower.contains(p));
    let trunc = |s: &str| if s.chars().count() > 220 { s.chars().take(220).collect::<String>() + "…" } else { s.to_string() };
    Some(mk(if sus { 3 } else { 2 }, 7045, "RT_SVC7045", "系统服务安装", "T1543.003",
        &format!("新服务「{name}」注册到系统（持久化/提权常用落点）· 路径 {img}"),
        &get("AccountName"), &name,
        &trunc(&format!("{name} · {} · 路径 {img}", if sus { "⚠ 可疑路径" } else { "常规路径" })), t))
}

#[cfg(windows)]
fn eval_106(xml: &str) -> Option<AlertItem> {
    if !rule_enabled("RT_TASK106") { return None; }
    let ev = parse_event(xml)?;
    if ev.event_id != 106 { return None; }
    let t = if ev.time_ms > 0 { ev.time_ms } else { now_ms() };
    let get = |k: &str| ev.data.get(k).cloned().unwrap_or_default();
    let name = get("TaskName");
    let content = get("TaskContent");
    let user = get("UserContext");
    let lower = format!("{name} {content}").to_lowercase();
    let sus = ["\\temp\\", "\\appdata\\", "\\users\\", "powershell", "cmd.exe", "cmd ", "cmd/",
        "wscript", "cscript", "mshta", "certutil", "bitsadmin", "-enc", "http://", "https://",
        "reg add", "rundll32", "regsvr32", "msbuild", "installutil"]
        .iter().any(|p| lower.contains(p));
    let trunc = |s: &str| if s.chars().count() > 220 { s.chars().take(220).collect::<String>() + "…" } else { s.to_string() };
    Some(mk(if sus { 3 } else { 2 }, 106, "RT_TASK106", "计划任务注册", "T1053.005",
        &format!("新计划任务「{name}」已注册（无前台安装文件的持久化常用手法）· 运行上下文 {user}"),
        &user, &name,
        &trunc(&format!("{name} · {} · {}", if sus { "⚠ 可疑动作" } else { "常规任务" }, if content.is_empty() { user.clone() } else { content })), t))
}

// ---------------------------------------------------------------------------
// 外部 IP 口令爆破关联（SMB/WMI/WinRM/RDP）：4625/4624 滑动窗口，纯内存 O(1)
// ---------------------------------------------------------------------------
#[cfg(windows)]
#[derive(Clone)]
struct BfState {
    count: u32,
    first: u64,
    last: u64,
    med: bool,
    high: bool,
}

#[cfg(windows)]
static BFMAP: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, BfState>>> =
    std::sync::OnceLock::new();

/// 仅外部来源：过滤私网/环回/链路本地（内网横向不在本规则范围）
#[cfg(windows)]
fn is_external_ip(ip: &str) -> bool {
    let ip = ip.trim();
    if ip.is_empty() || ip == "-" || ip == "127.0.0.1" || ip == "::1" {
        return false;
    }
    if ip.contains(':') {
        let l = ip.to_lowercase();
        return !(l.starts_with("fe80") || l.starts_with("fc") || l.starts_with("fd"));
    }
    let p: Vec<&str> = ip.split('.').collect();
    if p.len() != 4 { return false; }
    let a: u32 = p[0].parse().unwrap_or(0);
    let b: u32 = p[1].parse().unwrap_or(0);
    !(a == 0 || a == 10 || a == 127 || (a == 192 && b == 168)
        || (a == 172 && (16..=31).contains(&b)) || (a == 169 && b == 254) || a >= 224)
}

#[cfg(windows)]
fn bruteforce(xml: &str) -> Option<AlertItem> {
    let ev = parse_event(xml)?;
    let get = |k: &str| ev.data.get(k).cloned().unwrap_or_default();
    let t = if ev.time_ms > 0 { ev.time_ms } else { now_ms() };
    const NET: &str = "网络登录（SMB/WMI/WinRM）";
    match ev.event_id {
        4625 => {
            let ip = get("IpAddress");
            if !is_external_ip(&ip) { return None; }
            let lt: u32 = get("LogonType").parse().unwrap_or(0);
            let (rule, proto) = match lt {
                10 => ("BR_RDP", "RDP"),
                3 | 8 => ("BR_NET", NET),
                _ => return None, // 本地交互/服务/解锁等非远程爆破
            };
            if !rule_enabled(rule) { return None; }
            let user = get("TargetUserName");
            let mut map = BFMAP.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new())).lock().unwrap();
            // 容量治理：超 512 条先清 30 分钟未活跃，仍超则保留最近 400
            if map.len() > 512 {
                let cutoff = now_ms().saturating_sub(1_800_000);
                map.retain(|_, v| v.last >= cutoff);
                if map.len() > 512 {
                    let mut v: Vec<(String, u64)> = map.iter().map(|(k, x)| (k.clone(), x.last)).collect();
                    v.sort_by_key(|x| std::cmp::Reverse(x.1));
                    let keep: std::collections::HashSet<String> = v.into_iter().take(400).map(|x| x.0).collect();
                    map.retain(|k, _| keep.contains(k));
                }
            }
            let st = map.entry(format!("{proto}|{ip}")).or_insert(BfState { count: 0, first: t, last: t, med: false, high: false });
            if t.saturating_sub(st.first) > 600_000 { // 10 分钟滑动窗口
                st.count = 0; st.first = t; st.med = false; st.high = false;
            }
            st.count += 1;
            st.last = t;
            let n = st.count;
            if n == 5 && !st.med {
                st.med = true;
                return Some(mk(2, 4625, rule, "口令爆破进行中", "T1110.001",
                    &format!("外部 IP {ip} 正在爆破账户「{user}」（10 分钟内失败 {n} 次，20 次升级弹窗）"),
                    &ip, &user, &format!("外部 IP {ip} · 协议 {proto} · 账户 {user} · 失败 {n} 次/10 分钟"), t));
            }
            if n == 20 && !st.high {
                st.high = true;
                return Some(mk(3, 4625, rule, "口令爆破（高频）", "T1110.001",
                    &format!("外部 IP {ip} 正在高频爆破账户「{user}」（已失败 {n} 次，此后每 +50 次再提醒）"),
                    &ip, &user, &format!("外部 IP {ip} · 协议 {proto} · 账户 {user} · 失败 {n} 次/10 分钟"), t));
            }
            if n > 20 && n % 50 == 0 {
                return Some(mk(3, 4625, rule, "口令爆破（持续）", "T1110.001",
                    &format!("来自 {ip} 的爆破仍在针对账户「{user}」继续——建议封禁来源 IP / 收紧暴露面"),
                    &ip, &user, &format!("外部 IP {ip} · 协议 {proto} · 账户 {user} · 累计失败 {n} 次"), t));
            }
            None
        }
        4624 => {
            // 爆破后成功登录：最高危信号（口令已被猜出/泄露）
            let ip = get("IpAddress");
            if !is_external_ip(&ip) { return None; }
            let user = get("TargetUserName");
            let lt: u32 = get("LogonType").parse().unwrap_or(0);
            let proto = if lt == 10 { "RDP" } else { NET };
            let mut map = BFMAP.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new())).lock().unwrap();
            let prev = map.get(&format!("{proto}|{ip}")).map(|x| x.count).unwrap_or(0);
            if prev >= 5 && t.saturating_sub(map.get(&format!("{proto}|{ip}")).map(|x| x.last).unwrap_or(0)) <= 600_000 {
                if let Some(st) = map.get_mut(&format!("{proto}|{ip}")) {
                    st.count = 0; st.med = false; st.high = false;
                }
                return Some(mk(3, 4624, "BR_NET_SUCCESS", "爆破后成功登录（高危）", "T1110 / T1078",
                    &format!("外部 IP {ip} 在 {prev} 次失败后成功登录账户「{user}」：口令可能已被猜出或泄露，请立即核查"),
                    &ip, &user, &format!("IP {ip} · 协议 {proto} · 账户 {user} · 此前失败 {prev} 次"), t));
            }
            None
        }
        4720 => {
            // 新建本地账户：家用机极罕见，后门账户是持久化头号手法 → 即时弹窗
            let user = get("TargetUserName");
            let creator = get("SubjectUserName");
            Some(mk(3, 4720, "AC_NEW", "新建本地账户", "T1136.001",
                &format!("系统新增用户账户「{user}」（操作者 {creator}）——后门账户是持久化头号手法，请确认是否本人操作"),
                &creator, &user,
                &format!("新账户 {user} · 操作者 {creator} · SID {}", get("TargetSid")), t))
        }
        4732 => {
            // 本地组成员变更：加入管理员组 = 提权后门（弹窗）；其他组仅记录
            let group = get("TargetUserName");
            let actor = get("SubjectUserName");
            let gl = group.to_lowercase();
            let admin = gl.contains("administrator") || gl.contains("administrators");
            let member = get("MemberName");
            let member_disp = if member.is_empty() || member.starts_with('%') { get("MemberSid") } else { member };
            let desc_group = if admin {
                format!("「{member_disp}」被加入管理员组「{group}」（操作者 {actor}）——等同于提权后门，请立即确认是否本人操作")
            } else {
                format!("本地组「{group}」新增成员「{member_disp}」（操作者 {actor}）——常规组变更，仅记录")
            };
            Some(mk(if admin { 3 } else { 2 }, 4732, "AC_GROUP", "本地组成员变更", "T1098",
                &desc_group,
                &actor, &group,
                &format!("组 {group} · 新成员 {member_disp} · 操作者 {actor}"), t))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 去重（同规则+同主体 60 秒内只记一次）、存储、保留期清除
// ---------------------------------------------------------------------------
#[cfg(windows)]
fn dedupe_ok(a: &AlertItem) -> bool {
    let m = DEDUPE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut g = m.lock().unwrap();
    if g.len() > 512 {
        g.clear();
    }
    let key = format!("{}|{}|{}", a.rule, a.src, a.target);
    let now = now_ms();
    if let Some(last) = g.get(&key) {
        if now.saturating_sub(*last) < 60_000 {
            return false;
        }
    }
    g.insert(key, now);
    true
}

#[cfg(windows)]
/// epoch ms → 本地日期 YYYYMMDD（FileTime → 本地文件时区，分日文件键）
fn day_key(ms: u64) -> String {
    use windows::Win32::Foundation::FILETIME;
    use windows::Win32::Storage::FileSystem::FileTimeToLocalFileTime;
    use windows::Win32::System::Time::FileTimeToSystemTime;
    let ft: i64 = (ms as i64).saturating_mul(10_000) + 116_444_736_000_000_000;
    let f = FILETIME {
        dwLowDateTime: (ft as u64 & 0xFFFF_FFFF) as u32,
        dwHighDateTime: ((ft as u64 >> 32) & 0xFFFF_FFFF) as u32,
    };
    let mut local = FILETIME::default();
    let mut st = windows::Win32::Foundation::SYSTEMTIME::default();
    unsafe {
        if FileTimeToLocalFileTime(&f, &mut local).is_err() {
            return String::new();
        }
        if FileTimeToSystemTime(&local, &mut st).is_err() {
            return String::new();
        }
    }
    format!("{:04}{:02}{:02}", st.wYear, st.wMonth, st.wDay)
}

#[cfg(windows)]
/// 旧版单文件 alerts.json → data/alarm/ 分日文件（一次性；数据无损迁移）
fn migrate_legacy_alerts() {
    let Some(old) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.join("alerts.json")))
    else {
        return;
    };
    if !old.exists() {
        return;
    }
    let Ok(txt) = std::fs::read_to_string(&old) else { return };
    if let Ok(list) = serde_json::from_str::<Vec<AlertItem>>(&txt) {
        let mut days: Vec<(String, Vec<AlertItem>)> = Vec::new();
        for a in list {
            let k = day_key(a.ts_ms);
            if k.is_empty() {
                continue;
            }
            match days.iter_mut().find(|(d, _)| *d == k) {
                Some((_, v)) => v.push(a),
                None => days.push((k, vec![a])),
            }
        }
        for (k, mut v) in days {
            v.sort_by_key(|a| a.ts_ms);
            let dir = crate::settings::alarm_dir();
            let _ = std::fs::create_dir_all(&dir);
            let pf = dir.join(format!("alarm-{k}.json"));
            let mut merged: Vec<AlertItem> = std::fs::read_to_string(&pf)
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default();
            merged.extend(v);
            merged.dedup_by(|a, b| {
                a.ts_ms == b.ts_ms && a.rule == b.rule && a.src == b.src && a.target == b.target
            });
            let tmp = pf.with_extension("json.tmp");
            if std::fs::write(&tmp, serde_json::to_string(&merged).unwrap_or_default()).is_ok() {
                let _ = std::fs::rename(&tmp, &pf);
            }
        }
    }
    let _ = std::fs::rename(&old, old.with_extension("json.migrated"));
}

#[cfg(windows)]
fn load_all() -> Vec<AlertItem> {
    migrate_legacy_alerts();
    let mut out = Vec::new();
    let mut seq = 0u64;
    let dir = crate::settings::alarm_dir();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut files: Vec<std::path::PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map_or(false, |n| n.starts_with("alarm-") && n.ends_with(".json"))
            })
            .collect();
        files.sort(); // 文件名即日期序
        for f in files {
            if let Ok(txt) = std::fs::read_to_string(&f) {
                if let Ok(v) = serde_json::from_str::<Vec<AlertItem>>(&txt) {
                    for mut rec in v {
                        if rec.id.is_empty() {
                            seq += 1;
                            rec.id = format!("a{:x}-{:x}", rec.ts_ms, seq);
                        }
                        out.push(rec);
                    }
                }
            }
        }
    }
    out
}

#[cfg(windows)]
/// 仅重写某一天的分日文件（低资源关键：每条告警只触碰当天小文件，不重写全量历史）
fn save_day(list: &[AlertItem], ymd: &str) {
    if ymd.is_empty() {
        return;
    }
    let dir = crate::settings::alarm_dir();
    let _ = std::fs::create_dir_all(&dir);
    let pf = dir.join(format!("alarm-{ymd}.json"));
    let day: Vec<&AlertItem> = list.iter().filter(|a| day_key(a.ts_ms) == ymd).collect();
    let tmp = pf.with_extension("json.tmp");
    if std::fs::write(&tmp, serde_json::to_string(&day).unwrap_or_default()).is_ok() {
        let _ = std::fs::rename(&tmp, &pf);
    }
}

#[cfg(windows)]
fn purge(list: &mut Vec<AlertItem>) {
    let days = crate::settings::config().alert_retention_days as u64;
    let cutoff = now_ms().saturating_sub(days * 86_400_000);
    list.retain(|a| a.ts_ms >= cutoff);
    if list.len() > 2000 {
        let drop = list.len() - 2000;
        list.drain(0..drop);
    }
    // 文件级清理：超出保留期的分日文件整体删除（无需逐条扫描）
    let cutoff_ymd = day_key(now_ms().saturating_sub(days * 86_400_000));
    if !cutoff_ymd.is_empty() {
        if let Ok(rd) = std::fs::read_dir(crate::settings::alarm_dir()) {
            for e in rd.filter_map(|e| e.ok()) {
                let pf = e.path();
                if let Some(n) = pf.file_name().and_then(|n| n.to_str()) {
                    if let Some(ymd) = n
                        .strip_prefix("alarm-")
                        .and_then(|x| x.strip_suffix(".json"))
                    {
                        if ymd < cutoff_ymd.as_str() {
                            let _ = std::fs::remove_file(&pf);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(windows)]
/// 白名单判定（true = 加白跳过告警）：
/// 1) 内置：本工具自身触发的事件全局加白（不可移除）
/// 2) 系统来源（C:\Windows\）可开关
/// 3) 自定义程序白名单（完整路径相等，或按 \文件名 结尾匹配）
/// 4) 自定义目标白名单（包含匹配：路径/注册表键/主机等）
#[cfg(windows)]
fn whitelisted(a: &AlertItem) -> bool {
    // 内置屏蔽（默认生效，不可配置）：
    // 1) 目标/关键信息含 WebView2 —— 运行时组件的正常读写不构成威胁
    // 2) 目标/关键信息含工具自身标识 texmantis —— 本工具目录/文件/自身配置（如开机启动 Run 键）
    //    注意：仅匹配目标与关键信息；来源进程不做子串匹配（防伪装程序借名逃逸）
    let tgt_l = a.target.to_ascii_lowercase();
    let det_l = a.detail.to_ascii_lowercase();
    if tgt_l.contains("webview2")
        || det_l.contains("webview2")
        || tgt_l.contains("texmantis")
        || det_l.contains("texmantis")
    {
        return true;
    }
    crate::social::socdbg("whitelist: 取配置锁");
    let cfg = crate::settings::config();
    crate::social::socdbg("whitelist: 配置锁已获取");
    let src = a.src.trim();
    if !src.is_empty() {
        if let Ok(me) = std::env::current_exe() {
            if src.eq_ignore_ascii_case(&me.to_string_lossy()) {
                return true;
            }
        }
        let sl = src.to_ascii_lowercase();
        if cfg.wl_sys_src && (sl.starts_with("c:\\windows\\") || sl.starts_with("\\??\\c:\\windows\\")) {
            return true;
        }
        if cfg.wl_procs.iter().any(|w| {
            let w = w.trim().to_ascii_lowercase();
            !w.is_empty() && (sl == w || sl.ends_with(&format!("\\{w}")))
        }) {
            return true;
        }
    }
    let tgt = a.target.trim();
    if !tgt.is_empty() {
        let tl = tgt.to_ascii_lowercase();
        if cfg
            .wl_targets
            .iter()
            .any(|w| {
                let w = w.trim().to_ascii_lowercase();
                !w.is_empty() && tl.contains(w.as_str())
            })
        {
            return true;
        }
    }
    false
}

static ALERT_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// 全局唯一告警键：时间戳 + 进程内序号（重启后时间戳不同，序号区分同毫秒）
#[cfg(windows)]
fn next_alert_id(ts_ms: u64) -> String {
    format!("a{:x}-{:x}", ts_ms, ALERT_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed))
}

fn store(a: &AlertItem) -> bool {
    crate::social::socdbg("alert store: 进入（取 CACHE 锁）");
    let m = CACHE.get_or_init(|| std::sync::Mutex::new(load_all()));
    let mut g = m.lock().unwrap();
    crate::social::socdbg("alert store: CACHE 锁已获取");
    // 跨会话精确去重：重启后回溯不会与历史记录重复
    if g.iter()
        .any(|x| x.rule == a.rule && x.src == a.src && x.target == a.target && x.ts_ms == a.ts_ms)
    {
        return false;
    }
    let mut rec = a.clone();
    rec.id = next_alert_id(rec.ts_ms);
    g.push(rec);
    purge(&mut g);
    let ymd = day_key(a.ts_ms);
    save_day(&g, &ymd);
    // 事件驱动：新增告警即时推送（预警页监听 alerts-updated 刷新；3 秒轮询仅兜底）
    if let Some(app) = crate::settings::APP.get() {
        use tauri::Emitter;
        let _ = app.emit("alerts-updated", ());
    }
    true
}

#[cfg(windows)]
pub fn status() -> String { alert_status().text }


// ---------------------------------------------------------------------------
// 系统气泡窗口：贴屏幕右下角（系统工作区，任务栏/托盘之上），新告警从底部推入
// ---------------------------------------------------------------------------
#[cfg(windows)]
fn notify_bubble(app: &tauri::AppHandle, a: &AlertItem) {
    // 免打扰：持久化偏好，仅影响气泡展示；引擎与记录不受影响
    if crate::settings::config().alert_mute {
        crate::diag::write("D", &format!("命中 {} 但免打扰中，气泡静默", a.rule));
        return;
    }
    crate::diag::write("I", &format!("推送气泡 rule={} level={} 标题={}", a.rule, a.level, a.title));
    {
        let mut g = PENDING.lock().unwrap();
        g.push(a.clone());
        let l = g.len();
        if l > 3 {
            g.drain(0..l - 3);
        }
    }
    // 实时链路：窗口存活（含隐藏未销毁）即达 —— 真实告警与测试告警同一条路
    {
        use tauri::Emitter;
        let _ = app.emit("threat-alert", a);
    }
    show_notifier();
}

/// 社交消息气泡：与威胁预警同通道（PENDING 兜底 + threat-alert 实时事件 + 气泡窗），
/// kind="social" 让前端按聊天样式渲染并跳转社交板块
#[cfg(windows)]
/// v0.9.23 开放推送接口的消息气泡：与社交消息同通道，独立来源标识
pub fn notify_api(app: &tauri::AppHandle, title: &str, desc: &str) {
    let a = AlertItem {
        id: format!("api-{}", crate::social::now_ms_pub()),
        ts_ms: crate::social::now_ms_pub(),
        level: 1,
        event_id: 0,
        rule: "API 推送".into(),
        title: title.to_string(),
        mitre: String::new(),
        desc: desc.to_string(),
        src: "127.0.0.1".into(),
        target: String::new(),
        detail: String::new(),
        kind: "api".into(),
        ctx: String::new(),
    };
    {
        let mut g = PENDING.lock().unwrap();
        g.push(a.clone());
        let l = g.len();
        if l > 3 {
            g.drain(0..l - 3);
        }
    }
    {
        use tauri::Emitter;
        let _ = app.emit("threat-alert", &a);
    }
    show_notifier();
}

pub fn notify_social(app: &tauri::AppHandle, title: &str, desc: &str, ctx: &str) {
    let a = AlertItem {
        id: format!("soc-{}", now_ms()),
        ts_ms: now_ms(),
        level: 1,
        event_id: 0,
        rule: "社交消息".into(),
        title: title.to_string(),
        mitre: String::new(),
        desc: desc.to_string(),
        src: String::new(),
        target: String::new(),
        detail: String::new(),
        kind: "social".into(),
        ctx: ctx.to_string(),
    };
    {
        let mut g = PENDING.lock().unwrap();
        g.push(a.clone());
        let l = g.len();
        if l > 3 {
            g.drain(0..l - 3);
        }
    }
    {
        use tauri::Emitter;
        let _ = app.emit("threat-alert", &a);
    }
    show_notifier();
}

#[cfg(not(windows))]
pub fn notify_social(_app: &tauri::AppHandle, _title: &str, _desc: &str, _ctx: &str) {}

/// Sysmon 就绪后立即唤醒引擎重连（替代最长 60 秒的基线等待）
#[cfg(windows)]
/// Sysmon 重装会销毁并重建事件通道：既有订阅句柄全部失效，必须关闭并重订
/// （否则句柄看似有效、事件永不再推送——"重装后从此沉默"陷阱）
#[cfg(windows)]
pub(crate) fn reset_subscriptions() {
    use windows::Win32::System::EventLog::EvtClose;
    SUBSCRIBED.store(false, std::sync::atomic::Ordering::SeqCst);
    let mut closed = 0usize;
    if let Ok(mut g) = SUBS.lock() {
        for (_, h) in g.drain(..) {
            unsafe {
                let _ = EvtClose(h);
            }
            closed += 1;
        }
    }
    let gen = WATCH_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
    crate::diag::write("I", &format!("订阅句柄已重置（通道重建，关闭 {closed} 个）→ 重新订阅"));
    let _ = std::thread::Builder::new().name("alert-resub".into()).spawn(move || unsafe {
        if let Some(app) = crate::settings::APP.get() {
            watch(app.clone(), gen);
        }
    });
}

pub(crate) fn wake_engine() {
    if let Ok(mut g) = WAKE_LOCK.lock() {
        *g = true;
        WAKE_CV.notify_all();
    }
}

/// 气泡窗口高度贴合内容（消除固定窗口的空白区域）：底部锚定、向上生长
#[cfg(windows)]
#[tauri::command]
pub async fn notifier_fit(height: f64) -> Result<(), String> {
    use tauri::Manager;
    if let Some(app) = crate::settings::APP.get() {
        let a = app.clone();
        let h = a.clone();
        let hh = height.clamp(96.0, 420.0);
        let _ = a.run_on_main_thread(move || {
            if let Some(w) = h.get_webview_window("notifier") {
                let _ = w.set_size(tauri::LogicalSize::new(356.0_f64, hh));
                position_notifier(&h, &w);
            }
        });
    }
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
pub async fn notifier_fit(_height: f64) -> Result<(), String> { Ok(()) }

#[cfg(windows)]
pub fn show_notifier() {
    use tauri::Manager;
    if let Some(app) = crate::settings::APP.get() {
        let a = app.clone();
        let h = a.clone();
        let _ = a.run_on_main_thread(move || {
            if ensure_notifier_window(&h).is_err() {
                return;
            }
            if let Some(w) = h.get_webview_window("notifier") {
                position_notifier(&h, &w);
                let _ = w.show();
            }
        });
    }
}

#[cfg(windows)]
fn ensure_notifier_window(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
    if app.get_webview_window("notifier").is_some() {
        return Ok(());
    }
    let build = WebviewWindowBuilder::new(app, "notifier", WebviewUrl::App("index.html".into()))
        .title("青刃威胁预警")
        .decorations(false)
        .transparent(true) // 透明气泡窗口：卡片自绘半透明，空白区域全透明，无白底残留
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false) // 透明窗必须禁用 DWM 投影（会露出方形阴影框）
        .focused(false)
        .visible(false)
        .additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,CalculateNativeWinOcclusion") // v0.9.22 禁用遮挡计算：气泡窗长期被遮挡
        .inner_size(356.0, 120.0)
        .additional_browser_args(
            "--autoplay-policy=no-user-gesture-required --disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection",
        );
    build.build().map_err(|e| format!("创建气泡窗口失败: {e}"))?;
    Ok(())
}

/// 贴主屏工作区右下角（工作区已扣除任务栏，即托盘正上方），留 16px 边距
#[cfg(windows)]
fn position_notifier(app: &tauri::AppHandle, w: &tauri::WebviewWindow) {
    use tauri::{Manager, PhysicalPosition};
    let mon = app
        .get_webview_window("main")
        .and_then(|m| m.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(m) = mon else { return };
    let wa = m.work_area();
    let sz = w
        .outer_size()
        .unwrap_or(tauri::PhysicalSize::new(356u32, 336u32));
    let x = wa.position.x + wa.size.width as i32 - sz.width as i32 - 16;
    let y = wa.position.y + wa.size.height as i32 - sz.height as i32 - 16;
    let _ = w.set_position(PhysicalPosition::new(
        x.max(wa.position.x),
        y.max(wa.position.y),
    ));
}

/// Sysmon 通道探测（一次查询级开销）
#[cfg(windows)]
pub(crate) fn sysmon_ok() -> bool {
    use windows::Win32::System::EventLog::{EvtClose, EvtQuery, EvtQueryChannelPath};
    let chan: Vec<u16> = format!("{CHANNEL}\0").encode_utf16().collect();
    match unsafe {
        EvtQuery(
            windows::Win32::System::EventLog::EVT_HANDLE::default(),
            windows::core::PCWSTR(chan.as_ptr()),
            windows::core::PCWSTR::null(),
            EvtQueryChannelPath.0,
        )
    } {
        Ok(h) => {
            let _ = unsafe { EvtClose(h) };
            true
        }
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Tauri 命令
// ---------------------------------------------------------------------------
#[cfg(windows)]
#[tauri::command]
pub async fn alerts_get() -> Result<Vec<AlertItem>, String> {
    let m = CACHE.get_or_init(|| std::sync::Mutex::new(load_all()));
    let g = m.lock().unwrap();
    Ok(g.iter().rev().take(2000).cloned().collect())
}

#[cfg(windows)]
#[tauri::command]
pub async fn alerts_clear() -> Result<(), String> {
    let m = CACHE.get_or_init(|| std::sync::Mutex::new(load_all()));
    let mut g = m.lock().unwrap();
    g.clear();
    // 按天存档：清空即删除全部分日文件
    if let Ok(rd) = std::fs::read_dir(crate::settings::alarm_dir()) {
        for e in rd.filter_map(|e| e.ok()) {
            let pf = e.path();
            if let Some(n) = pf.file_name().and_then(|n| n.to_str()) {
                if n.starts_with("alarm-") && n.ends_with(".json") {
                    let _ = std::fs::remove_file(&pf);
                }
            }
        }
    }
    Ok(())
}

#[cfg(windows)]
#[tauri::command]
pub async fn alert_set_retention(days: u32) -> Result<(), String> {
    crate::settings::update_config(|c| c.alert_retention_days = days.clamp(1, 365));
    let m = CACHE.get_or_init(|| std::sync::Mutex::new(load_all()));
    let mut g = m.lock().unwrap();
    purge(&mut g);
    // 重写仍保留的各天文件（被清掉的天已由 purge 文件级删除）
    let mut days: Vec<String> = g
        .iter()
        .map(|a| day_key(a.ts_ms))
        .filter(|d| !d.is_empty())
        .collect();
    days.sort();
    days.dedup();
    for d in days {
        save_day(&g, &d);
    }
    Ok(())
}

#[cfg(windows)]
#[tauri::command]
pub async fn alert_rule_set(id: String, on: bool) -> Result<(), String> {
    crate::settings::update_config(|c| {
        if on {
            c.disabled_rules.retain(|x| x != &id);
        } else if !c.disabled_rules.contains(&id) {
            c.disabled_rules.push(id.clone());
        }
    });
    Ok(())
}

#[cfg(windows)]
#[tauri::command]
pub async fn alert_rules_state() -> Result<Vec<String>, String> {
    Ok(crate::settings::config().disabled_rules.clone())
}

#[cfg(windows)]
#[tauri::command]
pub async fn alert_monitor_status() -> Result<AlertStatus, String> {
    Ok(alert_status())
}

/// 一键启停：开 = 持久化 + 启动引擎（含回溯补录）；关 = 持久化 + 释放引擎资源
#[cfg(windows)]
#[tauri::command]
pub async fn alert_engine_toggle(on: bool) -> Result<String, String> {
    if on {
        crate::settings::update_config(|c| c.alert_monitor = true);
        match crate::settings::APP.get() {
            Some(app) => {
                start(app.clone());
                Ok("威胁预警已启动：实时监听 + 近期回溯补录".into())
            }
            None => Err("应用尚未就绪，请稍后重试".into()),
        }
    } else {
        crate::settings::update_config(|c| c.alert_monitor = false);
        engine_stop();
        Ok("威胁预警已停止，引擎资源已释放".into())
    }
}

// ---------------------------------------------------------------------------
// 状态结构（全平台）
// ---------------------------------------------------------------------------
#[derive(Serialize, Clone)]
pub struct AlertStatus {
    pub mode: String,   // off | local | waiting
    pub running: bool,
    pub sysmon: bool,
    pub muted: bool,
    pub text: String,
    pub version: String,             // 引擎版本（核对进程是否为新版）
    pub chans: Vec<(String, bool)>,  // 四通道逐个订阅状态
    pub cfg_current: bool,           // Sysmon 采集配置是否为当前版（含浏览器劫持键）
    pub last_alert_min: i64,         // 最近告警距今分钟数（-1 = 暂无记录；演示不计）
    pub health: String,              // 采集健康：正常 / 异常（…） / 停滞?（…） / 检测中…
}

#[cfg(windows)]
pub(crate) fn chan_states() -> Vec<(String, bool)> {
    let have = |ch: u8| SUBS.lock().map(|g| g.iter().any(|(c, _)| *c == ch)).unwrap_or(false);
    vec![
        ("Sysmon行为".into(), have(CH_SYSMON)),
        ("系统服务".into(), have(CH_SYSTEM)),
        ("计划任务".into(), have(CH_TASK)),
        ("安全日志".into(), have(CH_SEC)),
    ]
}

/// 通道自检自修复：关键事件通道被禁用则自动启用。
/// TaskScheduler/Operational 在 Windows 默认禁用——不启用则计划任务注册（E106）永远采集不到，
/// 订阅句柄虽在却永远收不到事件（"看起来在监控，实际全盲"的隐蔽故障）。
#[cfg(windows)]
pub(crate) fn ensure_channels_enabled() -> Vec<(&'static str, bool, bool)> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let chans = [
        "Microsoft-Windows-Sysmon/Operational",
        "Microsoft-Windows-TaskScheduler/Operational",
        "Security",
        "System",
    ];
    let mut repaired: Vec<&str> = Vec::new();
    let mut res: Vec<(&'static str, bool, bool)> = Vec::new();
    for ch in chans {
        let txt = std::process::Command::new("wevtutil")
            .args(["gl", ch])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|o| crate::commands::decode_proc_output(&o.stdout))
            .unwrap_or_default();
        if txt.to_lowercase().contains("enabled: false") {
            let ok = std::process::Command::new("wevtutil")
                .args(["sl", ch, "/e:true"])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            crate::diag::write(
                "I",
                &format!("通道自修复 {ch}: {}", if ok { "已启用" } else { "启用失败" }),
            );
            if ok {
                repaired.push(ch);
            }
            res.push((ch, ok, ok));
        } else {
            res.push((ch, true, false));
        }
    }
    if !repaired.is_empty() {
        crate::settings::app_notify(&format!(
            "威胁预警自修复：已启用被禁用的事件通道（{}）",
            repaired.join("、")
        ));
    }
    res
}

/// 审计策略自检自修复：账户管理成功审计被关闭（部分"优化工具"会关）则自动启用。
/// 用 GUID 而非名称——auditpol 子类别名称随系统语言本地化，GUID 全语言一致。
/// User Account Management = {0CCE9235-69AE-11D9-BED3-505054503030}；Setting Value 位1=成功审计
#[cfg(windows)]
pub(crate) fn ensure_audit_policy() -> (bool, bool) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const SUB: &str = "/subcategory:{0CCE9235-69AE-11D9-BED3-505054503030}";
    let out = std::process::Command::new("auditpol")
        .args(["/get", SUB, "/r"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let txt = out.map(|o| crate::commands::decode_proc_output(&o.stdout)).unwrap_or_default();
    let last = txt.lines().filter(|l| l.contains(',')).last().unwrap_or("");
    let val: u32 = last.rsplit(',').next().unwrap_or("").trim().parse().unwrap_or(3);
    if val & 1 == 0 {
        let ok = std::process::Command::new("auditpol")
            .args(["/set", SUB, "/success:enable"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        crate::diag::write(
            "I",
            &format!("审计自修复 账户管理: {}", if ok { "已启用成功审计" } else { "启用失败" }),
        );
        if ok {
            crate::settings::app_notify("威胁预警自修复：已启用被关闭的账户管理审计");
        }
        return (ok, ok);
    }
    (true, false)
}

/// 采集配置是否为当前版（含浏览器劫持键，0.3.1+ 标记）
#[cfg(windows)]
fn cfg_current() -> bool {
    crate::settings::config_path()
        .map(|p| p.with_file_name("sysmon").join("texmantis-default.xml"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.contains("TexMantis Capture Profile v6"))
        .unwrap_or(false)
}

/// 最近一次落盘告警距今分钟数（-1 = 暂无记录）
#[cfg(windows)]
fn last_alert_min() -> i64 {
    let Some(m) = CACHE.get() else { return -1 };
    let g = m.lock().unwrap();
    let max = g.iter().map(|a| a.ts_ms).max().unwrap_or(0);
    if max == 0 { return -1; }
    (now_ms().saturating_sub(max) / 60_000) as i64
}

#[cfg(windows)]
pub fn alert_status() -> AlertStatus {
    let sysmon = sysmon_ok();
    let muted = crate::settings::config().alert_mute;
    let version = env!("CARGO_PKG_VERSION").into();
    let hs = HEALTH.get().and_then(|m| m.lock().ok()).and_then(|g| g.clone());
    let health = match &hs {
        None => "检测中…".to_string(),
        Some(hh) if !hh.responds => "异常（服务/驱动无响应）".to_string(),
        Some(hh) if hh.drv_running == Some(false) => "异常（内核驱动未加载）".to_string(),
        Some(hh) if hh.hash_match == Some(false) => "异常（生效配置与内置不一致）".to_string(),
        Some(hh) => match hh.newest_age_secs {
            Some(x) if x >= 300 => format!("停滞?（最新事件 {} 分钟前）", x.max(1) / 60),
            Some(_) => "正常".to_string(),
            None => "未知（通道暂无匹配事件）".to_string(),
        },
    };
    // 订阅芯片反映真实可用性：Sysmon 采集不健康时，Sysmon 行为通道标红
    let mut chans = chan_states();
    if let Some(hh) = &hs {
        if !hh.responds {
            for c in chans.iter_mut() {
                if c.0 == "Sysmon行为" {
                    c.1 = false;
                }
            }
        }
    }
    let cfg_current = cfg_current();
    let last_alert_min = last_alert_min();
    if !crate::settings::config().alert_monitor {
        return AlertStatus { mode: "off".into(), running: false, sysmon, muted, version, chans, cfg_current, last_alert_min, health,
            text: "监控已停止 · 引擎资源已释放（启动后自动回溯补录）".into() };
    }
    let started = STARTED_CHECK.load(std::sync::atomic::Ordering::SeqCst) == 1;
    let subscribed = SUBSCRIBED.load(std::sync::atomic::Ordering::SeqCst);
    let missing = chans.iter().filter(|(_, ok)| !ok).map(|(n, _)| n.clone()).collect::<Vec<_>>().join("、");
    let warn = health.starts_with("异常");
    if subscribed {
        let text = if !warn {
            if missing.is_empty() { "运行中 · 四通道已订阅（事件驱动 · 零轮询）".into() }
            else { format!("运行中 · {missing} 通道待就绪（每分钟自动重试）") }
        } else {
            format!("运行中 · ⚠ Sysmon 采集{health}")
        };
        AlertStatus { mode: "local".into(), running: true, sysmon, muted, version, chans, cfg_current, last_alert_min, health, text }
    } else if started {
        AlertStatus { mode: "local".into(), running: true, sysmon, muted, version, chans, cfg_current, last_alert_min, health,
            text: "启动中 · 正在连接事件通道（每分钟自动重试）".into() }
    } else {
        AlertStatus { mode: "waiting".into(), running: false, sysmon, muted, version, chans, cfg_current, last_alert_min, health,
            text: "等待事件通道（自动重试）".into() }
    }
}

#[cfg(not(windows))]
pub fn alert_status() -> AlertStatus {
    AlertStatus { mode: "off".into(), running: false, sysmon: false, muted: false, version: env!("CARGO_PKG_VERSION").into(), chans: Vec::new(), cfg_current: false, last_alert_min: -1, health: "不支持".into(), text: "仅支持 Windows".into() }
}

// 非 Windows 平台桩（保持编译通过）
#[cfg(not(windows))]
#[derive(Serialize, Deserialize, Clone)]
pub struct AlertItem {
    #[serde(default)]
    pub id: String,
    pub ts_ms: u64,
    pub level: u32,
    pub event_id: u32,
    pub rule: String,
    pub title: String,
    pub mitre: String,
    pub desc: String,
    pub src: String,
    pub target: String,
    pub detail: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub ctx: String,
}

#[cfg(not(windows))]
#[tauri::command]
pub async fn alerts_get() -> Result<Vec<AlertItem>, String> {
    Ok(Vec::new())
}

#[cfg(not(windows))]
#[tauri::command]
pub async fn alerts_clear() -> Result<(), String> {}

#[cfg(not(windows))]
#[tauri::command]
pub async fn alert_set_retention(_days: u32) -> Result<(), String> {}

#[cfg(not(windows))]
#[tauri::command]
pub async fn alert_rule_set(_id: String, _on: bool) -> Result<(), String> {}

#[cfg(not(windows))]
#[tauri::command]
pub async fn alert_rules_state() -> Result<Vec<String>, String> {
    Ok(Vec::new())
}

#[cfg(not(windows))]
#[tauri::command]
pub async fn alert_monitor_status() -> Result<AlertStatus, String> {
    Ok(alert_status())
}

#[cfg(not(windows))]
#[tauri::command]
pub async fn alert_engine_toggle(_on: bool) -> Result<String, String> {
    Ok("仅支持 Windows".into())
}

#[cfg(not(windows))]
pub fn status() -> String {
    "仅支持 Windows".into()
}

#[cfg(not(windows))]
pub fn engine_stop() {}

/// 测试演示预警：单按钮随机演示（池内 3 种，不与上次重复），走与真实告警完全相同的气泡链路（不落盘）
#[cfg(windows)]
static LAST_DEMO: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(255);

#[cfg(windows)]
#[tauri::command]
pub async fn alert_test() -> Result<String, String> {
    if crate::settings::config().alert_mute {
        return Ok("气泡通知处于「免打扰」中，本次演示不弹出；点顶部「气泡：免打扰中 · 点击开启」即可恢复".into());
    }
    // 随机挑选：时间熵取模，且不与上次重复（池内 3 种）
    let mut idx = (now_ms() as u8 % 3) as usize;
    let last = LAST_DEMO.load(std::sync::atomic::Ordering::SeqCst);
    if last == idx as u8 {
        idx = (idx + 1) % 3;
    }
    LAST_DEMO.store(idx as u8, std::sync::atomic::Ordering::SeqCst);

    let a = match idx {
        0 => AlertItem {
            id: String::new(),
            ts_ms: now_ms(), level: 3, event_id: 10, rule: "E10_LSASS".into(),
            title: "LSASS 内存读取（凭据转储）".into(), mitre: "T1003.001".into(),
            desc: "非系统进程以内存读取权限访问 LSASS，典型 Mimikatz/ProcDump 凭据窃取行为".into(),
            src: r"C:\Users\demo\AppData\Temp\dump.exe".into(), target: "lsass.exe".into(),
            detail: "GrantedAccess 0x1410 · 演示数据".into(),
            kind: String::new(),
            ctx: String::new(),
        },
        1 => AlertItem {
            id: String::new(),
            ts_ms: now_ms(), level: 2, event_id: 3, rule: "E3_BADPORT".into(),
            title: "非常规端口外联".into(), mitre: "T1071.001 / T1571".into(),
            desc: "进程向非常用端口发起外联，可能是 C2 回连、数据外排或横向移动".into(),
            src: r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe".into(),
            target: "203.0.113.66:4444".into(),
            detail: "协议 tcp · 演示数据".into(),
            kind: String::new(),
            ctx: String::new(),
        },
        _ => AlertItem {
            id: String::new(),
            ts_ms: now_ms(), level: 3, event_id: 106, rule: "RT_TASK106".into(),
            title: "计划任务注册（可疑动作）".into(), mitre: "T1053.005".into(),
            desc: "新计划任务已注册，动作指向 Temp 目录脚本（无前台安装文件的持久化）".into(),
            src: r"C:\Windows\System32\svchost.exe".into(),
            target: r"\Microsoft\Windows\Demo\UpdateCheck".into(),
            detail: r"C:\Users\demo\AppData\Local\Temp\updater.ps1 · 演示数据".into(),
            kind: String::new(),
            ctx: String::new(),
        },
    };
    match crate::settings::APP.get() {
        Some(app) => {
            notify_bubble(app, &a);
            Ok(format!("演示告警已发送（{} · {}，与真实告警同一链路，不落盘）", if a.level >= 3 { "高风险" } else { "中风险" }, a.title))
        }
        None => Err("应用尚未就绪，请稍后重试".into()),
    }
}


// ---------------------------------------------------------------------------
// 端到端自检：注入金丝雀事件（Run 键写入），走完全真实的
// 订阅→渲染→解析→命中→落盘→气泡 链路，逐段报告断点；并用 EvtQuery
// 探查 Sysmon 日志，区分「未采集」与「订阅未推送」
// ---------------------------------------------------------------------------
#[cfg(windows)]
fn reg_quick(args: &[&str]) -> bool {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("reg").args(args).creation_flags(CREATE_NO_WINDOW)
        .output().map(|o| o.status.success()).unwrap_or(false)
}

/// 金丝雀是否被 Sysmon 记录：优先 XPath 精确匹配 TargetObject（结论确定，
/// 不受事件量影响）；查询失败则回退扫描最近 512 条
#[cfg(windows)]
/// Sysmon 通道近 30 分钟事件数（采集活性铁证：独立于引擎订阅，timediff 由服务端过滤）。None=通道查询失败
#[cfg(windows)]
pub(crate) fn sysmon_channel_fresh_events() -> Option<u32> {
    use windows::core::PCWSTR;
    use windows::Win32::System::EventLog::{
        EvtClose, EvtNext, EvtQuery, EvtQueryChannelPath, EvtQueryReverseDirection, EVT_HANDLE,
    };
    let chan: Vec<u16> = format!("{CHANNEL}\0").encode_utf16().collect();
    let query: Vec<u16> =
        "*[System[TimeCreated[timediff(@SystemTime) <= 1800000]]]\0".encode_utf16().collect();
    unsafe {
        let qh = EvtQuery(
            EVT_HANDLE::default(),
            PCWSTR(chan.as_ptr()),
            PCWSTR(query.as_ptr()),
            (EvtQueryChannelPath.0 | EvtQueryReverseDirection.0),
        ).ok()?;
        let mut buf = [0isize; 8];
        let mut n = 0u32;
        let _ = EvtNext(qh, &mut buf, 500, 0, &mut n);
        for i in 0..n as usize {
            let _ = EvtClose(EVT_HANDLE(buf[i]));
        }
        let _ = EvtClose(qh);
        Some(n)
    }
}

fn channel_has_canary(vname: &str) -> Option<bool> {
    use windows::core::PCWSTR;
    use windows::Win32::System::EventLog::{
        EvtClose, EvtNext, EvtQuery, EvtQueryChannelPath, EvtQueryReverseDirection, EVT_HANDLE,
    };
    let chan: Vec<u16> = format!("{CHANNEL}\0").encode_utf16().collect();
    let target = format!(r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run\{vname}");
    let query: Vec<u16> =
        format!("*[EventData[Data[@Name='TargetObject']='{}']]\0", target).encode_utf16().collect();
    unsafe {
        if let Ok(qh) = EvtQuery(
            EVT_HANDLE::default(),
            PCWSTR(chan.as_ptr()),
            PCWSTR(query.as_ptr()),
            (EvtQueryChannelPath.0 | EvtQueryReverseDirection.0),
        ) {
            let mut buf = [0isize; 4];
            let mut n = 0u32;
            let ok = EvtNext(qh, &mut buf, 500, 0, &mut n).is_ok();
            let found = ok && n > 0;
            for i in 0..n as usize {
                let _ = EvtClose(EVT_HANDLE(buf[i]));
            }
            let _ = EvtClose(qh);
            if found {
                return Some(true); // 精确命中：确定已采集
            }
            // 精确未命中 → 继续回退扫描（兜底 TargetObject 渲染差异）
        }
        // 回退：逆序扫描最近 512 条
        let mut found = false;
        let mut buf = [0isize; 32];
        if let Ok(qh) = EvtQuery(
            EVT_HANDLE::default(),
            PCWSTR(chan.as_ptr()),
            PCWSTR::null(),
            (EvtQueryChannelPath.0 | EvtQueryReverseDirection.0),
        ) {
            'outer2: for _ in 0..16 {
                let mut n = 0u32;
                if EvtNext(qh, &mut buf, 500, 0, &mut n).is_err() || n == 0 {
                    break;
                }
                for i in 0..n as usize {
                    let h = EVT_HANDLE(buf[i]);
                    if let Some(xml) = render_xml(h) {
                        if xml.contains("TexMantisSelfTest") {
                            found = true;
                        }
                    }
                    let _ = EvtClose(h);
                    if found {
                        break 'outer2;
                    }
                }
            }
            let _ = EvtClose(qh);
        }
        Some(found)
    }
}

// ---------------------------------------------------------------------------
// Sysmon 深度健康诊断：服务响应 / 生效配置一致性（hash 对比）/ 采集活性（最新事件年龄）
// ---------------------------------------------------------------------------
#[cfg(windows)]
#[derive(Clone)]
pub(crate) struct SysmonHealth {
    responds: bool,             // Sysmon64 -c 是否正常输出
    hash_match: Option<bool>,   // 生效配置 hash 与工具内置配置文件是否一致
    newest_age_secs: Option<u64>, // 通道最新事件距今秒数
    drv_running: Option<bool>,  // SysmonDrv 内核驱动是否运行（无驱动 = 服务在也不产生事件）
    e13_recent: u32,            // 通道内 EventID13 样本数（逆序最多取 10）
    e13_age: Option<u64>,       // 最新 EventID13 距今秒数
}

#[cfg(windows)]
fn newest_event_age() -> Option<u64> {
    use windows::core::PCWSTR;
    use windows::Win32::System::EventLog::{
        EvtClose, EvtNext, EvtQuery, EvtQueryChannelPath, EvtQueryReverseDirection, EVT_HANDLE,
    };
    let chan: Vec<u16> = format!("{CHANNEL}\0").encode_utf16().collect();
    unsafe {
        let qh = EvtQuery(
            EVT_HANDLE::default(),
            PCWSTR(chan.as_ptr()),
            PCWSTR::null(),
            (EvtQueryChannelPath.0 | EvtQueryReverseDirection.0),
        )
        .ok()?;
        let mut buf = [0isize; 5];
        let mut n = 0u32;
        let mut age: Option<u64> = None;
        if EvtNext(qh, &mut buf, 500, 0, &mut n).is_ok() {
            // 依次尝试最新几条，直到有一条能解析出时间（防个别元数据事件解析失败误判）
            for i in 0..n as usize {
                let h = EVT_HANDLE(buf[i]);
                if let Some(xml) = render_xml(h) {
                    if let Some(e) = parse_event(&xml) {
                        if e.time_ms > 0 {
                            age = Some(now_ms().saturating_sub(e.time_ms) / 1000);
                        }
                    }
                }
                let _ = EvtClose(h);
                if age.is_some() {
                    break;
                }
            }
        }
        let _ = EvtClose(qh);
        age
    }
}

#[cfg(windows)]
pub(crate) fn sysmon_health() -> SysmonHealth {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut responds = false;
    let mut hash_match: Option<bool> = None;
    if let Some(dir) = crate::settings::config_path().map(|p| p.with_file_name("sysmon")) {
        let exe = dir.join("Sysmon64.exe");
        let cfgp = dir.join("texmantis-default.xml");
        if exe.exists() {
            if let Ok(o) = std::process::Command::new(&exe).arg("-c").creation_flags(CREATE_NO_WINDOW).output() {
                // Sysmon 重定向到管道时输出 UTF-16LE：必须按启发式解码，否则 SHA256= 永远找不到（假阴性）
                let txt = format!("{}{}", crate::commands::decode_proc_output(&o.stdout), crate::commands::decode_proc_output(&o.stderr));
                if let Some(i) = txt.find("SHA256=") {
                    responds = true;
                    let hex: String = txt[i + 7..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
                    if let Ok(bytes) = std::fs::read(&cfgp) {
                        use sha2::{Digest, Sha256};
                        let mut h = Sha256::new();
                        h.update(&bytes);
                        let ours = format!("{:x}", h.finalize());
                        hash_match = Some(ours.eq_ignore_ascii_case(&hex));
                    }
                }
            }
        }
    }
    let drv_running = (|| -> Option<bool> {
        let o = std::process::Command::new("sc").args(["query", "SysmonDrv"]).creation_flags(CREATE_NO_WINDOW).output().ok()?;
        Some(o.status.success() && crate::commands::decode_proc_output(&o.stdout).contains("RUNNING"))
    })();
    let (e13_recent, e13_age) = e13_probe();
    let newest_age_secs = newest_event_age();
    crate::diag::write("D", &format!(
        "健康探测 服务={} 驱动={:?} hash={:?} 最新事件={:?}秒 E13样本={}条 最新E13={:?}秒",
        if responds { "响应" } else { "无响应" }, drv_running, hash_match, newest_age_secs, e13_recent, e13_age
    ));
    SysmonHealth { responds, hash_match, newest_age_secs, drv_running, e13_recent, e13_age }
}

/// 启动环境快照：Sysmon 服务/驱动状态 + 事件通道配置 → 诊断日志
#[cfg(windows)]
fn env_snapshot() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    for args in [["sc", "query", "Sysmon64"], ["sc", "query", "SysmonDrv"], ["wevtutil", "gl", CHANNEL]] {
        match std::process::Command::new(args[0])
            .args(&args[1..])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
        {
            Ok(o) => {
                let txt = crate::commands::decode_proc_output(&o.stdout);
                let brief: Vec<String> = txt
                    .lines()
                    .filter(|l| {
                        let l = l.trim();
                        ["SERVICE_NAME", "DISPLAY_NAME", "TYPE", "STATE", "enabled", "maxSize", "logFilePath", "已启用", "最大大小", "日志路径"]
                            .iter()
                            .any(|k| l.contains(k))
                    })
                    .take(8)
                    .map(|l| l.trim().to_string())
                    .collect();
                crate::diag::write("I", &format!(
                    "环境快照 {}{}: {}",
                    args.join(" "),
                    if o.status.success() { "" } else { "（命令失败）" },
                    if brief.is_empty() { txt.trim().chars().take(240).collect::<String>() } else { brief.join(" | ") }
                ));
            }
            Err(e) => crate::diag::write("W", &format!("环境快照 {} 执行失败: {e}", args.join(" "))),
        }
    }
}

#[cfg(not(windows))]
fn env_snapshot() {}

/// 通道内 EventID13（注册表值设置）最近样本：逆序取最多 10 条
#[cfg(windows)]
fn e13_probe() -> (u32, Option<u64>) {
    use windows::core::PCWSTR;
    use windows::Win32::System::EventLog::{
        EvtClose, EvtNext, EvtQuery, EvtQueryChannelPath, EvtQueryReverseDirection, EVT_HANDLE,
    };
    let chan: Vec<u16> = format!("{CHANNEL}\0").encode_utf16().collect();
    let query: Vec<u16> = "*[System[(EventID=13)]]\0".encode_utf16().collect();
    let mut count = 0u32;
    let mut age: Option<u64> = None;
    unsafe {
        if let Ok(qh) = EvtQuery(
            EVT_HANDLE::default(),
            PCWSTR(chan.as_ptr()),
            PCWSTR(query.as_ptr()),
            (EvtQueryChannelPath.0 | EvtQueryReverseDirection.0),
        ) {
            let mut buf = [0isize; 10];
            let mut n = 0u32;
            if EvtNext(qh, &mut buf, 500, 0, &mut n).is_ok() {
                count = n;
                if n > 0 {
                    let h = EVT_HANDLE(buf[0]);
                    if let Some(xml) = render_xml(h) {
                        match parse_event(&xml).filter(|e| e.time_ms > 0) {
                            Some(e) => age = Some(now_ms().saturating_sub(e.time_ms) / 1000),
                            None => crate::diag::write("W", &format!(
                                "E13 样本解析失败 xml={}",
                                xml.chars().take(500).collect::<String>()
                            )),
                        }
                    } else {
                        crate::diag::write("W", "E13 样本渲染失败（EvtRender）");
                    }
                    let _ = EvtClose(h);
                }
                for i in 1..n as usize {
                    let _ = EvtClose(EVT_HANDLE(buf[i]));
                }
            }
            let _ = EvtClose(qh);
        }
    }
    (count, age)
}

/// 写入健康快照（巡检线程 / 重同步命令共用）
#[cfg(windows)]
pub(crate) fn health_store(h: SysmonHealth) {
    let m = HEALTH.get_or_init(|| std::sync::Mutex::new(None));
    if let Ok(mut g) = m.lock() {
        *g = Some(h);
    }
}

#[cfg(windows)]
fn selftest_state() -> &'static std::sync::Mutex<SelfTestState> {
    SELFTEST.get_or_init(|| std::sync::Mutex::new(SelfTestState { armed_ms: 0, event_seen: false, matched: false, stored: false, bubbled: false }))
}

/// 一轮金丝雀：重置探针 → 写 Run 键 → 等待流经真实链路（≤6 秒）→ 返回值名
#[cfg(windows)]
fn canary_round() -> String {
    if let Ok(mut g) = selftest_state().lock() {
        *g = SelfTestState { armed_ms: now_ms(), event_seen: false, matched: false, stored: false, bubbled: false };
    }
    let vname = format!("TexMantisSelfTest{}", now_ms() / 1000);
    let wrote = reg_quick(&[
        "add", r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run",
        "/v", &vname, "/t", "REG_SZ", "/d", "cmd /c echo selftest", "/f",
    ]);
    crate::diag::write("I", &format!("自检：写入金丝雀 Run 键 {vname} ok={wrote}"));
    for _ in 0..12 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Ok(g) = selftest_state().lock() {
            if g.bubbled {
                break;
            }
        }
    }
    vname
}

#[cfg(windows)]
fn selftest_report(seen: bool, matched: bool, stored: bool, bubbled: bool, health: &SysmonHealth, probe: Option<bool>, repaired: bool) -> String {
    let prefix = if repaired { "已自动重应用配置并重测｜" } else { "" };
    if seen {
        if !matched {
            return format!("自检结果：✗ {prefix}事件已到达引擎但未命中规则（解析异常）→ 请把本结果反馈给开发者");
        }
        if !stored {
            return format!("自检结果：✗ {prefix}命中 E13_RUNKEY 但未落盘（存储异常）→ 请把本结果反馈给开发者");
        }
        if !bubbled {
            if crate::settings::config().alert_mute {
                return format!("自检结果：✓ {prefix}事件到达 ✓ 规则命中 ✓ 已落盘｜气泡因「免打扰」被静默——点顶部按钮关闭免打扰即可恢复");
            }
            return format!("自检结果：✓ {prefix}事件到达 ✓ 规则命中 ✓ 已落盘｜气泡未显示（气泡窗口异常）→ 请把本结果反馈给开发者");
        }
        return format!("自检结果：✓ {prefix}全链路通过（到达→命中 E13_RUNKEY→落盘→气泡）｜自检告警已入列表，可自行清除");
    }
    let health_txt = if !health.responds {
        "Sysmon 服务/驱动无响应（Sysmon64 -c 无输出）".to_string()
    } else {
        format!(
            "配置{}；最新事件 {}",
            match health.hash_match {
                Some(true) => "一致✓",
                Some(false) => "不一致✗（Sysmon 在用其他配置）",
                None => "hash 校验失败",
            },
            match health.newest_age_secs {
                Some(x) if x < 120 => format!("{} 秒前（采集活跃）", x),
                Some(x) => format!("{} 分钟前（采集停滞?）", x.max(1) / 60),
                None => "年龄未知".into(),
            }
        )
    };
    let probe_txt = match probe {
        Some(true) => "；Sysmon 日志中已找到金丝雀（采集正常，推送链路异常）",
        Some(false) => "；Sysmon 日志精确查询未找到金丝雀（确认未采集）",
        None => "；日志查询失败",
    };
    if !health.responds {
        return format!("自检结果：✗ {prefix}金丝雀未到达引擎（{probe_txt}）｜{health_txt}→ 请执行「一键重装 Sysmon」");
    }
    if health.drv_running == Some(false) {
        return format!("自检结果：✗ {prefix}金丝雀未到达引擎（{probe_txt}）｜Sysmon 服务在但内核驱动（SysmonDrv）未加载，事件不会产生 → 请执行「一键重装 Sysmon」重新注册驱动；详情见「诊断日志」");
    }
    if health.hash_match == Some(false) {
        return format!("自检结果：✗ {prefix}金丝雀未到达引擎（{probe_txt}）｜{health_txt}→ 请重启工具（启动会自动同步配置）后再运行一次自检");
    }
    if health.e13_recent > 0 {
        return format!("自检结果：✗ {prefix}金丝雀未到达引擎（{probe_txt}）｜通道内 EventID13 有样本（{} 条，最新 {:?} 秒前）但无金丝雀 → 请打开「诊断日志」查看明细并反馈开发者", health.e13_recent, health.e13_age);
    }
    if matches!(health.newest_age_secs, Some(x) if x < 120) {
        return format!("自检结果：✗ {prefix}金丝雀未到达引擎（{probe_txt}）｜{health_txt}→ 采集活跃但 Run 键事件缺失，请打开「诊断日志」并反馈开发者");
    }
    format!("自检结果：✗ {prefix}金丝雀未到达引擎（{probe_txt}）｜{health_txt}→ 通道无事件：请打开「诊断日志」查看驱动与通道状态，必要时一键重装 Sysmon")
}

#[cfg(windows)]
fn selftest_blocking() -> Result<String, String> {
    if !RUN.load(std::sync::atomic::Ordering::SeqCst) || !crate::settings::config().alert_monitor {
        return Err("引擎未运行：请先点击「启动监控」".into());
    }
    let health = sysmon_health();
    let vname = canary_round();
    let (mut seen, mut matched, mut stored, mut bubbled) = {
        let g = selftest_state().lock().unwrap();
        (g.event_seen, g.matched, g.stored, g.bubbled)
    };
    let mut repaired = false;
    let mut probe: Option<bool> = None;
    if !seen && health.responds && health.hash_match == Some(false) {
        if crate::commands::apply_sysmon_config() {
            repaired = true;
            let _ = reg_quick(&["delete", r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run", "/v", &vname, "/f"]);
            let v2 = canary_round();
            (seen, matched, stored, bubbled) = {
                let g = selftest_state().lock().unwrap();
                (g.event_seen, g.matched, g.stored, g.bubbled)
            };
            probe = channel_has_canary(&v2);
            let _ = reg_quick(&["delete", r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run", "/v", &v2, "/f"]);
            return Ok(selftest_report(seen, matched, stored, bubbled, &health, probe, repaired));
        }
    }
    probe = channel_has_canary(&vname);
    let _ = reg_quick(&["delete", r"HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\Run", "/v", &vname, "/f"]);
    crate::diag::write("I", &format!(
        "自检完成 seen={seen} matched={matched} stored={stored} bubbled={bubbled} probe={probe:?} 服务={} 驱动={:?} hash={:?} 最新事件={:?}秒 E13={}条/{:?}秒",
        if health.responds { "响应" } else { "无响应" },
        health.drv_running, health.hash_match, health.newest_age_secs, health.e13_recent, health.e13_age
    ));
    Ok(selftest_report(seen, matched, stored, bubbled, &health, probe, repaired))
}

/// 端到端自检入口（阻塞探测放独立线程，不卡异步运行时）
#[cfg(windows)]
#[tauri::command]
pub async fn alert_selftest() -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(selftest_blocking)
        .await
        .map_err(|e| format!("自检任务失败：{e}"))?
}

/// 气泡免打扰开关（持久化；开启时立即收起气泡窗口）
#[cfg(windows)]
#[tauri::command]
pub async fn alert_mute_set(on: bool) -> Result<(), String> {
    use tauri::{Emitter, Manager};
    crate::settings::update_config(|c| c.alert_mute = on);
    if on {
        if let Some(app) = crate::settings::APP.get() {
            let a = app.clone();
            let h = a.clone();
            let _ = a.run_on_main_thread(move || {
                let _ = h.emit("threat-clear", ());
                if let Some(w) = h.get_webview_window("notifier") {
                    let _ = w.hide();
                }
            });
        }
    }
    Ok(())
}

/// 气泡窗口就绪后补拉窗口加载期间到达的告警（仅近 15 秒内的有效）
#[cfg(windows)]
#[tauri::command]
pub async fn notifier_take_pending() -> Result<Vec<AlertItem>, String> {
    let now = now_ms();
    let mut g = PENDING.lock().unwrap();
    g.retain(|a| now.saturating_sub(a.ts_ms) < 15_000);
    Ok(std::mem::take(&mut *g))
}

/// 点击气泡：唤起主窗口并跳转威胁预警中心（携带被点击的告警时直接打开详情）
#[cfg(windows)]
#[tauri::command]
pub async fn notifier_open_main(alert: Option<AlertItem>) -> Result<(), String> {
    use tauri::{Emitter, Manager};
    if let Some(app) = crate::settings::APP.get() {
        let a = app.clone();
        let h = a.clone();
        let _ = a.run_on_main_thread(move || {
            if let Some(w) = h.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
            let _ = h.emit("goto-alert", alert);
        });
    }
    Ok(())
}

#[cfg(not(windows))]
#[tauri::command]
pub async fn alert_test() -> Result<String, String> { Ok("仅支持 Windows".into()) }

#[cfg(not(windows))]
#[tauri::command]
pub async fn alert_selftest() -> Result<String, String> { Ok("仅支持 Windows".into()) }

#[cfg(not(windows))]
#[tauri::command]
pub async fn alert_mute_set(_on: bool) -> Result<(), String> { Ok(()) }

#[cfg(not(windows))]
#[tauri::command]
pub async fn notifier_take_pending() -> Result<Vec<AlertItem>, String> { Ok(Vec::new()) }

#[cfg(not(windows))]
#[tauri::command]
pub async fn notifier_open_main(_alert: Option<AlertItem>) -> Result<(), String> { Ok(()) }

