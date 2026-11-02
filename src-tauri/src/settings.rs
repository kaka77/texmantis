//! 工具设置：开机启动（HKCU Run）+ 偏好持久化（HKCU\Software\TexMantis）
//!
//! 资源占用策略（高级思路）：
//! - 仅设置页可见时才查询；开机启动写入 HKCU Run（不建计划任务、不装服务，零常驻）
//! - 前端"窗口失焦自动暂停轮询"：失焦/最小化时所有页面的定时器空转，不产生查询
//! - 托盘常驻时窗口隐藏，WebView 继续存在但不刷新（配合失焦暂停，内存稳定 ~60MB）

use serde::{Deserialize, Serialize};

/// 全局 AppHandle：后台线程向主窗推送通知用
pub static APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

/// 后台线程 → 主窗 toast（事件 app-toast）
pub fn app_notify(msg: &str) {
    if let Some(app) = APP.get() {
        use tauri::Emitter;
        let _ = app.emit("app-toast", msg.to_string());
    }
}

// ---------------------------------------------------------------------------
// config.json 持久化（exe 同目录，随工具走，与 UAC 提权账户无关）
// ---------------------------------------------------------------------------
#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "dft_alert_monitor")]
    pub close_to_tray: bool,
    pub pause_on_blur: bool,
    pub float_dock: bool,
    pub diag_log: bool,
    pub ctrl_window_ms: u32,
    // 开放推送接口密钥（空 = 接口关闭；用户在社交设置生成 10 位随机密钥）
    #[serde(default)]
    pub api_key: String,
    // 系统优化勾选偏好（一键优化时记录；启动时按此恢复可恢复项）
    pub opt_ctrl_dbl: bool,
    pub opt_ctx_ps: bool,
    pub opt_ctx_cmd: bool,
    pub opt_unlock_ctx: bool,
    // 威胁预警（Sysmon 监控）
    #[serde(default = "dft_alert_monitor")]
    pub alert_monitor: bool,
    #[serde(default = "dft_alert_retention")]
    pub alert_retention_days: u32,
    #[serde(default = "dft_disabled_rules")]
    pub disabled_rules: Vec<String>,
    // 气泡通知免打扰（持久化；引擎与告警记录不受影响）
    #[serde(default = "dft_alert_mute")]
    pub alert_mute: bool,
    // 威胁预警白名单：系统来源过滤开关 + 自定义程序/目标加白（记录页右键加白也写这里）
    #[serde(default)]
    pub wl_sys_src: bool,
    #[serde(default)]
    pub wl_procs: Vec<String>,
    #[serde(default)]
    pub wl_targets: Vec<String>,
    // 社交模块「青刃·知己」：随工具启动（默认开）、本机身份、收发偏好
    #[serde(default = "dft_social_enabled")]
    pub social_enabled: bool,
    #[serde(default)]
    pub social_uid: String,        // 本机唯一标识（首次生成后持久化）
    #[serde(default)]
    pub social_nickname: String,   // 空 = 用系统用户名
    #[serde(default = "dft_social_gender")]
    pub social_gender: String,     // m / f / x（中性）
    #[serde(default = "dft_social_avatar")]
    pub social_avatar: u8,         // 0..=19
    #[serde(default)]
    pub social_port: u16,          // 上次成功绑定的端口（0 = 自动）
    #[serde(default)]
    pub social_favs: Vec<String>,  // 收藏（好友）uid 列表
    #[serde(default)]
    pub social_idkey: String,      // X25519 身份私钥（hex64，首次生成持久化；指纹核验防中间人）
    #[serde(default)]
    pub social_named: bool,        // 是否已生成/设置过昵称（首次进入社交页自动生成一次）
    #[serde(default = "dft_true")]
    pub social_chat_left: bool,   // 社交两栏布局：聊天在左（true）/在右（false）——记住用户习惯
    #[serde(default)]
    pub social_remarks: std::collections::BTreeMap<String, String>, // v0.9.0 好友备注：uid → 备注（空串即删除）
    #[serde(default)]
    pub social_recv_dir: String, // v0.9.2 接收文件根目录（空 = 默认「下载」文件夹下的 TexMantis 子目录）
    #[serde(default)]
    pub social_removed: Vec<String>, // v0.9.3 手动移除的联系人（对方再来消息会自动恢复）
    #[serde(default = "dft_true")]
    pub social_zombie: bool, // v0.9.3 同机多身份（mid 相同）自动清理僵尸号并迁移备注
    #[serde(default)]
    pub social_font_size: u8, // v0.9.5 聊天字号（px，12-17；0 = 默认 13）
}

fn dft_alert_monitor() -> bool { true }
fn dft_alert_mute() -> bool { false }
fn dft_alert_retention() -> u32 { 90 }
fn dft_disabled_rules() -> Vec<String> { vec!["E22_DNS".into()] }
fn dft_social_enabled() -> bool { true }
fn dft_true() -> bool { true }
fn dft_social_gender() -> String { "x".into() }
fn dft_social_avatar() -> u8 { ((std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)) % 20) as u8 }

impl Default for Config {
    fn default() -> Self {
        Self {
            close_to_tray: false,
            pause_on_blur: true,
            float_dock: false,
            diag_log: false,
            ctrl_window_ms: 600,
            social_enabled: true,
            social_uid: String::new(),
            social_nickname: String::new(),
            social_gender: "x".into(),
            social_avatar: 0,
            social_port: 0,
            social_favs: Vec::new(),
            social_idkey: String::new(),
            social_named: false,
            social_chat_left: true,
            social_remarks: std::collections::BTreeMap::new(),
            social_recv_dir: String::new(),
            social_removed: Vec::new(),
            social_zombie: true,
            social_font_size: 13,
            api_key: String::new(),
            opt_ctrl_dbl: false,
            opt_ctx_ps: false,
            opt_ctx_cmd: false,
            opt_unlock_ctx: false,
            alert_monitor: true,
            alert_mute: false,
            alert_retention_days: 90,
            disabled_rules: vec!["E22_DNS".into()],
            wl_sys_src: false,
            wl_procs: Vec::new(),
            wl_targets: Vec::new(),
        }
    }
}

/// 数据根目录：exe 同目录 data\（配置直接置于 data\，日志/告警分置于 logs\ 与 alarm\）
pub(crate) fn data_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::env::temp_dir().join("TexMantis"))
        .join("data")
}

pub(crate) fn logs_dir() -> std::path::PathBuf {
    data_dir().join("logs")
}

pub(crate) fn alarm_dir() -> std::path::PathBuf {
    data_dir().join("alarm")
}

pub(crate) fn social_dir() -> std::path::PathBuf {
    data_dir().join("social")
}

pub(crate) fn config_path() -> Option<std::path::PathBuf> {
    let _ = std::fs::create_dir_all(data_dir());
    Some(data_dir().join("config.json"))
}

/// 旧版扁平布局 → data 目录一次性迁移（升级用户配置/日志无缝保留）。
/// alerts.json 的按天拆分迁移在 monitor::load_all 内完成（需解析内容）。
pub(crate) fn migrate_legacy() {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let d = data_dir();
            let _ = std::fs::create_dir_all(d.join("logs"));
            let _ = std::fs::create_dir_all(d.join("alarm"));
            let new_cfg = d.join("config.json");
            let old_cfg = dir.join("config.json");
            if !new_cfg.exists() && old_cfg.exists() {
                let _ = std::fs::rename(&old_cfg, &new_cfg);
            }
            for name in ["diag.log", "diag.log.1"] {
                let old = dir.join(name);
                let new = d.join("logs").join(name);
                if !new.exists() && old.exists() {
                    let _ = std::fs::rename(&old, &new);
                }
            }
        }
    }
}

pub(crate) fn save_config(c: &Config) {
    let Some(p) = config_path() else { return };
    let Ok(txt) = serde_json::to_string_pretty(c) else { return };
    let tmp = p.with_extension(format!("json.{}.tmp", std::process::id()));
    if std::fs::write(&tmp, &txt).is_ok() && std::fs::rename(&tmp, &p).is_ok() {
        let _ = std::fs::copy(&p, p.with_extension("json.bak")); // 最近一次完好像本（损坏自愈源）
        return;
    }
    // 杀软锁定目标文件时 rename 可能失败：回退为直接覆写
    if std::fs::write(&p, &txt).is_ok() {
        let _ = std::fs::remove_file(&tmp);
        return;
    }
    static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !WARNED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        let msg = "config.json 保存失败（可能被安全软件锁定），本次设置仅存于内存".to_string();
        std::thread::spawn(move || crate::diag::write("W", &msg)); // 绝不在调用线程同步回读配置
    }
}

/// 配置保存专用线程：update_config 只投递（微秒级），磁盘写入全部在此线程完成并
/// 合并到最新 —— 杀软拖延写盘时任何命令路径都不等待
static SAVE_TX: std::sync::LazyLock<std::sync::mpsc::Sender<Config>> = std::sync::LazyLock::new(|| {
    let (tx, rx) = std::sync::mpsc::channel::<Config>();
    std::thread::spawn(move || loop {
        let Ok(mut c) = rx.recv() else { break };
        while let Ok(nc) = rx.try_recv() {
            c = nc; // 合并到最新
        }
        save_config(&c);
    });
    tx
});

/// 配置就位后的统一收尾：同步诊断开关镜像（在任何 diag 调用前完成）+ 生成社交身份
fn apply_config(c: &mut Config) {
    DIAG_ON.store(
        if c.diag_log { 1 } else { 2 },
        std::sync::atomic::Ordering::SeqCst,
    );
    sync_mirrors(c);
    if c.social_uid.is_empty() {
        c.social_uid = gen_social_uid();
        save_config(c);
    }
}

/// v0.8.0 启动备份：每次启动先把当前 config.json 复制到 config-backups/（保留最近 5 份）。
/// 无论文件好坏都备份——损坏也能找回原文，用户数据绝不静默丢失。
fn backup_on_launch(p: &std::path::Path) {
    if !p.exists() { return; }
    let dir = match p.parent() { Some(d) => d.join("config-backups"), None => return };
    if std::fs::create_dir_all(&dir).is_err() { return; }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = std::fs::copy(p, dir.join(format!("config-{ts}.json")));
    // 只保留最近 5 份（文件名含秒级时间戳，字典序即时间序）
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| {
                    let n = e.file_name().to_string_lossy().to_string();
                    n.starts_with("config-") && n.ends_with(".json")
                })
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    while files.len() > 5 {
        let _ = std::fs::remove_file(files.remove(0));
    }
}

fn load_config() -> Config {
    if let Some(p) = config_path() {
        backup_on_launch(&p); // v0.8.0：启动先备份原文件（含损坏文件），再走恢复链
        if let Ok(txt) = std::fs::read_to_string(&p) {
            let parsed = serde_json::from_str::<Config>(&txt);
            if let Err(e) = &parsed {
                // 注意：此处处于配置 OnceLock 初始化内，严禁任何会回读 config 的调用
                let msg = format!("config.json 解析失败（{}），尝试备份恢复", e);
                let bad = p.with_extension("json.bad");
                let _ = std::fs::rename(&p, &bad);
                std::thread::spawn(move || crate::diag::write("W", &msg));
                // 恢复链：完好像本 .bak → 损坏前一份 .bad（用户原设置找回）
                for src in [p.with_extension("json.bak"), bad.clone()] {
                    if let Ok(bak) = std::fs::read_to_string(&src) {
                        if let Ok(mut c2) = serde_json::from_str::<Config>(&bak) {
                            apply_config(&mut c2);
                            save_config(&c2); // 立即写回主文件，完成自愈
                            return c2;
                        }
                    }
                }
            } else if let Ok(mut c) = parsed {
                apply_config(&mut c);
                return c;
            }
        }
    }
    // 首次迁移：旧注册表值 → config.json（老用户设置无缝保留）
    let mut c = Config::default();
    #[cfg(windows)]
    {
        c.close_to_tray = reg::get_dword(CFG_KEY, "CloseToTray") == Some(1);
        c.pause_on_blur = reg::get_dword(CFG_KEY, "PauseOnBlur").unwrap_or(1) == 1;
        c.float_dock = reg::get_dword(CFG_KEY, "FloatDock") == Some(1);
        c.diag_log = reg::get_dword(CFG_KEY, "DiagLog").unwrap_or(0) == 1;
        c.ctrl_window_ms = reg::get_dword(CFG_KEY, "CtrlDblWindowMs").unwrap_or(600).clamp(100, 2000);
    }
    apply_config(&mut c);
    c
}

/// 本机社交身份：主机名 + 时间 + 进程号 哈希，生成一次后持久化
fn gen_social_uid() -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into()).hash(&mut h);
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0).hash(&mut h);
    std::process::id().hash(&mut h);
    format!("U{:012x}", h.finish())
}

static CONFIG: std::sync::OnceLock<std::sync::Mutex<Config>> = std::sync::OnceLock::new();

/// 全局配置句柄（首次访问时加载/迁移）
/// 持锁者追踪：每次成功获取都记录（线程号@绝对毫秒）。锁被卡住时，
/// "最近一次成功获取者" = 当前持锁者（其后不可能再有新的成功获取）
pub static CFG_OWNER: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

pub fn config() -> std::sync::MutexGuard<'static, Config> {
    let m = CONFIG.get_or_init(|| std::sync::Mutex::new(load_config()));
    let t0 = std::time::Instant::now();
    let g = m.lock().unwrap_or_else(|e| e.into_inner());
    let waited = t0.elapsed().as_millis();
    {
        let mut o = CFG_OWNER.lock().unwrap_or_else(|e| e.into_inner());
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        *o = format!("{:?}@{}", std::thread::current().id(), now);
    }
    if waited > 50 {
        // 配置锁饥饿监测：某线程持锁过久时取证（load_config 初始化期间绝不走到这里）
        crate::social::socdbg(&format!("config lock: 等待 {waited}ms —— 持锁线程疑似停滞"));
    }
    g
}

/// 配置锁看门狗：每秒 try_lock 一次；连续失败即记录"已被持续持有 N 秒 + 持锁者"。
/// 取证专用，开销为零（try_lock 成功立即释放）。
pub fn start_lock_watchdog() {
    std::thread::Builder::new().name("cfg-watchdog".into()).spawn(|| {
        let mut stuck: u64 = 0;
        loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            let m = CONFIG.get_or_init(|| std::sync::Mutex::new(load_config()));
            match m.try_lock() {
                Ok(_g) => {
                    if stuck >= 2 {
                        crate::social::socdbg("config lock: 已恢复可用（此前停滞结束）");
                    }
                    stuck = 0;
                }
                Err(_) => {
                    stuck += 1;
                    if stuck >= 2 {
                        let owner = CFG_OWNER.lock().unwrap_or_else(|e| e.into_inner()).clone();
                        crate::social::socdbg(&format!(
                            "config lock: 已被持续持有 ≥{stuck}s —— 持锁者: {owner}"
                        ));
                    }
                }
            }
        }
    }).ok();
}

/// 原子修改并保存 —— 磁盘写入在【锁外】执行（杀软拦截文件写入时可能拖延数百毫秒
/// 到数秒，锁若跨越写盘会让所有配置读取者冻结；与社交防火墙同类教训）
pub fn update_config(f: impl FnOnce(&mut Config)) {
    let snapshot = {
        let mut c = config();
        f(&mut c);
        DIAG_ON.store(if c.diag_log { 1 } else { 2 }, std::sync::atomic::Ordering::SeqCst);
        sync_mirrors(&c);
        c.clone()
    };
    let _ = SAVE_TX.send(snapshot); // 无界通道，永不阻塞
}

/// --unlock 右键直达请求
pub static UNLOCK_REQ: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

#[derive(Serialize, Clone)]
pub struct AppSettings {
    pub auto_start: bool,       // 开机启动
    pub close_to_tray: bool,    // 关闭按钮隐藏到托盘
    pub pause_on_blur: bool,    // 失焦时暂停自动刷新
    pub float_dock: bool,       // 贴边浮漂（迷你 UFO）
    pub diag_log: bool,         // 诊断日志（默认关）
    pub alert_monitor: bool,    // Sysmon 威胁预警
    pub alert_retention_days: u32, // 告警保留期（天）
}

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const CFG_KEY: &str = r"Software\TexMantis";

#[cfg(windows)]
fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// 复用 optimize 的注册表原子操作（模块内 pub fn）
#[cfg(windows)]
use crate::optimize::reg;

/// 诊断日志是否开启（默认关）
/// 诊断开关的原子镜像：diag::write 绝不回读配置（回读会在配置初始化期间重入
/// OnceLock 造成永久死锁，也能避免任何 diag↔config 锁序交叉）
pub static DIAG_ON: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0); // 0=未知 1=开 2=关

pub fn diag_log_enabled() -> bool {
    DIAG_ON.load(std::sync::atomic::Ordering::SeqCst) == 1
}

/// 双击 Ctrl 触发时限（ms）：两次独立 Ctrl 按下的最大间隔，默认 600
pub fn ctrl_dbl_window_ms() -> u32 {
    config().ctrl_window_ms
}

pub fn set_ctrl_dbl_window_ms(ms: u32) -> Result<(), String> {
    update_config(|c| c.ctrl_window_ms = ms.clamp(100, 2000));
    Ok(())
}

/// 主线程镜像：float_dock / close_to_tray 的原子副本（窗口事件处理器零锁读取，
/// 即使配置锁被卡死，窗口焦点/关闭事件也绝不在主线程上等待配置锁）
pub static FLOAT_DOCK_MIRROR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static CLOSE_TO_TRAY_MIRROR: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn sync_mirrors(c: &Config) {
    use std::sync::atomic::Ordering::SeqCst;
    FLOAT_DOCK_MIRROR.store(c.float_dock, SeqCst);
    CLOSE_TO_TRAY_MIRROR.store(c.close_to_tray, SeqCst);
}

pub fn get_settings() -> AppSettings {
    // 注册表读取在配置锁外（注册表 I/O 可能被安全软件拖延，绝不持锁跨系统调用）
    #[cfg(windows)]
    let auto_start = reg::key_exists_run(RUN_KEY, "TexMantis");
    #[cfg(not(windows))]
    let auto_start = false;
    let c = config();
    #[cfg(windows)]
    {
        AppSettings {
            auto_start,
            close_to_tray: c.close_to_tray,
            pause_on_blur: c.pause_on_blur,
            float_dock: c.float_dock,
            diag_log: c.diag_log,
            alert_monitor: c.alert_monitor,
            alert_retention_days: c.alert_retention_days,
        }
    }
    #[cfg(not(windows))]
    {
        AppSettings {
            auto_start,
            close_to_tray: c.close_to_tray,
            pause_on_blur: c.pause_on_blur,
            float_dock: c.float_dock,
            diag_log: c.diag_log,
            alert_monitor: c.alert_monitor,
            alert_retention_days: c.alert_retention_days,
        }
    }
}

pub fn set_auto_start(on: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        if on {
            let exe = std::env::current_exe()
                .map_err(|e| format!("无法定位自身程序：{e}"))?;
            let v = format!("\"{}\"", exe.display());
            reg::set_sz(RUN_KEY, "TexMantis", &v)
        } else {
            reg::delete_value(RUN_KEY, "TexMantis")
        }
    }
    #[cfg(not(windows))]
    {
        let _ = on;
        Ok(())
    }
}

pub fn get_flag(name: &str) -> bool {
    #[cfg(windows)]
    {
        reg::get_dword(CFG_KEY, name).unwrap_or(0) == 1
    }
    #[cfg(not(windows))]
    {
        let _ = name;
        false
    }
}

pub fn set_flag(name: &str, on: bool) -> Result<(), String> {
    update_config(|c| {
        match name {
            "CloseToTray" => c.close_to_tray = on,
            "PauseOnBlur" => c.pause_on_blur = on,
            "FloatDock" => c.float_dock = on,
            "DiagLog" => {
            c.diag_log = on;
            if !on {
                // 关闭即不产生：连带删除已有日志文件（与设置页承诺一致）
                crate::diag::remove_files();
            }
        },
            "AlertMonitor" => c.alert_monitor = on,
            _ => {}
        }
    });
    // 威胁监控开关：开/关即时生效（启动引擎 / 释放引擎资源）
    #[cfg(windows)]
    if name == "AlertMonitor" {
        match (on, APP.get()) {
            (true, Some(app)) => crate::monitor::start(app.clone()),
            (false, _) => crate::monitor::engine_stop(),
            _ => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tauri 命令
// ---------------------------------------------------------------------------
#[tauri::command]
pub async fn settings_get() -> Result<AppSettings, String> {
    Ok(get_settings())
}

/// 右键菜单直达：取出待解锁路径（一次性）
#[tauri::command]
pub async fn unlock_take_request() -> Option<String> {
    UNLOCK_REQ.lock().unwrap().take()
}

#[tauri::command]
pub async fn settings_set_auto_start(on: bool) -> Result<(), String> {
    set_auto_start(on)
}

#[tauri::command]
pub async fn settings_set_flag(name: String, on: bool) -> Result<(), String> {
    match name.as_str() {
        "CloseToTray" | "PauseOnBlur" => set_flag(&name, on),
        "DiagLog" => {
            let r = set_flag("DiagLog", on);
            if on {
                // 开启即写一行标识（文件在日志函数内自动创建）
                crate::optimize::hook_log("诊断日志已开启");
            } else {
                // 关闭即删除日志文件，不留痕迹
                let _ = std::fs::remove_file(crate::optimize::LOG_PATH.get_or_init(crate::optimize::hook_log_path));
            }
            r
        }
        other => Err(format!("未知设置项：{other}")),
    }
}

// ---------------------------------------------------------------------------
// 贴边浮漂（迷你 UFO）
// ---------------------------------------------------------------------------
use std::sync::Mutex;
pub static FLOAT_PAGE: Mutex<Option<(f64, f64)>> = Mutex::new(None);

/// 磁吸滑出/滑回：悬停展开完整浮标，静止只露 12px 发光条
#[tauri::command]
pub async fn float_slide(app: tauri::AppHandle, out: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        if let Some(w) = app.get_webview_window("float") {
            let mon = w
                .current_monitor()
                .ok()
                .flatten()
                .or_else(|| app.primary_monitor().ok().flatten());
            if let Some(m) = mon {
                let scale = m.scale_factor();
                let sw = m.size().width as f64 / scale;
                let sh = m.size().height as f64 / scale;
                let y = (sh / 2.0 - 56.0).max(0.0);
                let dock_x = (sw - 12.0).max(0.0); // 静止：外露 12px
                let open_x = (sw - 44.0).max(0.0); // 展开：露出 44px（4px 压边防悬停抖动）
                let target = if out { open_x } else { dock_x };
                let (cur_x, cur_y) = match w.outer_position() {
                    Ok(p) => (p.x as f64 / scale, p.y as f64 / scale),
                    Err(_) => (dock_x, y),
                };
                // 4 步滑动动画
                for i in 1..=4u32 {
                    let t = i as f64 / 4.0;
                    let x = cur_x + (target - cur_x) * t;
                    let ny = cur_y + (y - cur_y) * t;
                    let _ = w.set_position(tauri::LogicalPosition::new(x, ny));
                    std::thread::sleep(std::time::Duration::from_millis(22));
                }
                let _ = w.set_position(tauri::LogicalPosition::new(target, y));
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        let _ = out;
    }
    Ok(())
}

/// 浮标页面回报自身渲染状态（诊断用）
#[tauri::command]
pub async fn float_page_ready(w: f64, h: f64) -> Result<(), String> {
    if let Ok(mut g) = FLOAT_PAGE.lock() {
        *g = Some((w, h));
    }
    Ok(())
}
use tauri::Manager;

#[tauri::command]
pub async fn float_dock_set(app: tauri::AppHandle, on: bool) -> Result<String, String> {
    set_flag("FloatDock", on)?;
    #[cfg(windows)]
    {
        if on {
            if let Ok(mut g) = FLOAT_PAGE.lock() {
                *g = None;
            }
            let info = crate::create_float_window(&app)?;
            std::thread::sleep(std::time::Duration::from_millis(1200));
            let page = FLOAT_PAGE.lock().ok().and_then(|g| *g);
            let verdict = match page {
                Some((pw, ph)) => format!("页面已渲染 {pw:.0}×{ph:.0} ✓"),
                None => "页面未回报渲染状态".to_string(),
            };
            return Ok(format!("浮标已开启：{info}；{verdict}。关闭主窗后停靠右缘"));
        } else if let Some(w) = app.get_webview_window("float") {
            let _ = w.close();
            // 关闭浮标时若主窗隐藏，拉回主窗避免失联
            if let Some(m) = app.get_webview_window("main") {
                let _ = m.show();
                let _ = m.set_focus();
            }
            return Ok("贴边浮标已关闭，主窗已拉回前台".into());
        }
    }
    #[cfg(not(windows))]
    {
        let _ = app;
    }
    Ok(if on { "已开启" } else { "已关闭" }.to_string())
}

/// 浮漂点击：呼出主窗并收起自己
#[tauri::command]
pub async fn float_summon(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(m) = app.get_webview_window("main") {
        let _ = m.show();
        let _ = m.unminimize();
        let _ = m.set_focus();
    }
    if let Some(f) = app.get_webview_window("float") {
        let _ = f.hide();
    }
    Ok(())
}
