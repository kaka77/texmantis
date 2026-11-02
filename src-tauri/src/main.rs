#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod network;
mod monitor;
mod optimize;
mod settings;
mod diag;
mod social;
mod pushapi;

use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};

// ---------------------------------------------------------------------------
// 高阶权限：进程启动即启用全部管理特权组（SeDebug/SeTakeOwnership/SeBackup/
// SeRestore/SeSecurity/SeLoadDriver）—— 应急工具的最高用户态权限组合
// ---------------------------------------------------------------------------
#[cfg(target_os = "windows")]
fn enable_privileges() {
    unsafe {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::Security::{
            AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES,
            SE_BACKUP_NAME, SE_DEBUG_NAME, SE_PRIVILEGE_ENABLED, SE_RESTORE_NAME,
            SE_SECURITY_NAME, SE_TAKE_OWNERSHIP_NAME, TOKEN_PRIVILEGES,
        };
        use windows::Win32::Security::{TOKEN_ADJUST_PRIVILEGES, TOKEN_QUERY};
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut token).is_err() {
            return;
        }
        for name in [SE_DEBUG_NAME, SE_TAKE_OWNERSHIP_NAME, SE_BACKUP_NAME, SE_RESTORE_NAME, SE_SECURITY_NAME] {
            let mut luid = windows::Win32::Foundation::LUID::default();
            if LookupPrivilegeValueW(None, name, &mut luid).is_ok() {
                let mut tp = TOKEN_PRIVILEGES {
                    PrivilegeCount: 1,
                    Privileges: [LUID_AND_ATTRIBUTES { Luid: luid, Attributes: SE_PRIVILEGE_ENABLED }],
                };
                let _ = AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None);
            }
        }
        let _ = CloseHandle(token);
    }
}

// 未提权时通过 UAC 自动重启自身（拒绝提权则继续以普通权限运行）
#[cfg(target_os = "windows")]
/// 从命令行参数提取 --unlock 目标路径
fn extract_unlock_arg(args: &[&str]) -> Option<String> {
    let i = args.iter().position(|a| *a == "--unlock")?;
    args.get(i + 1).map(|s| s.trim_matches('"').to_string())
}

fn auto_elevate() {
    unsafe {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        let mut token = HANDLE::default();
        let elevated = OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_ok() && {
            let mut elev = TOKEN_ELEVATION::default();
            let mut ret = 0u32;
            let ok = GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elev as *mut _ as *mut core::ffi::c_void),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut ret,
            )
            .is_ok()
                && elev.TokenIsElevated != 0;
            let _ = windows::Win32::Foundation::CloseHandle(token);
            ok
        };
        if elevated {
            return;
        }
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_DEFAULT, SHELLEXECUTEINFOW};
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let exe = match std::env::current_exe() {
            Ok(e) => e,
            Err(_) => return,
        };
        use std::os::windows::ffi::OsStrExt;
        let file: Vec<u16> = exe.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let verb: Vec<u16> = "runas\0".encode_utf16().collect();
        // 透传命令行参数（如 --unlock <path>），否则提权重启后直达意图丢失
        let params_str = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
        let params: Vec<u16> = if params_str.is_empty() {
            Vec::new()
        } else {
            params_str
                .as_str()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect()
        };
        let mut sei = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_DEFAULT,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: if params.is_empty() { PCWSTR::null() } else { PCWSTR(params.as_ptr()) },
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        if ShellExecuteExW(&mut sei).is_ok() {
            std::process::exit(0); // 提权实例已启动，本实例退出
        }
        // 用户拒绝 UAC → 继续普通权限运行（界面横幅会提示）
    }
}

// ---------------------------------------------------------------------------
// 贴边浮漂（迷你 UFO）：独立小窗，趴在屏幕右缘
// ---------------------------------------------------------------------------
#[cfg(target_os = "windows")]
pub fn create_float_window(app: &tauri::AppHandle) -> Result<String, String> {
    use tauri::{LogicalPosition, WebviewUrl, WebviewWindowBuilder};
    if let Some(w) = app.get_webview_window("float") {
        let _ = w.show();
        return Ok("浮标窗口已存在，已重新显示".into());
    }
    let build = WebviewWindowBuilder::new(app, "float", WebviewUrl::App("float.html".into()))
        .title("青刃")
        .decorations(false)
        .transparent(false) // 实心：透明合成在部分驱动/远程桌面场景整窗不可见
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .focused(false)
        .visible(true)
        .inner_size(48.0, 104.0)
        .additional_browser_args("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,CalculateNativeWinOcclusion"); // v0.9.22 禁用遮挡计算：浮标常驻屏幕边缘，最易触发遮挡挂死
    let w = build.build().map_err(|e| format!("创建浮标窗口失败: {e}"))?;
    let _ = w.set_focusable(false); // 无焦点窗口：显示/常驻都不抢当前应用的焦点，点击事件照常接收
    // 趴在屏幕右缘、垂直居中；优先主窗口所在显示器
    let mut info = "已创建（未取得显示器信息，默认位置）".to_string();
    let mon = app
        .get_webview_window("main")
        .and_then(|m| m.current_monitor().ok().flatten())
        .or_else(|| w.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    if let Some(m) = mon {
        let scale = m.scale_factor();
        let sw = m.size().width as f64 / scale;
        let sh = m.size().height as f64 / scale;
        let x = (sw - 12.0).max(0.0); // 静止：只外露 12px 磁吸条
        let y = (sh / 2.0 - 56.0).max(0.0);
        let _ = w.set_position(LogicalPosition::new(x, y));
        info = format!("48×104 @ ({:.0},{:.0})，屏幕 {:.0}×{:.0} 缩放 {:.2}", x, y, sw, sh, scale);
    }
    let _ = w.show();
    let _ = w.set_always_on_top(true);
    let _ = w.unminimize();
    // Win11 圆角（Win10 忽略）
    if let Ok(h) = w.hwnd() {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Dwm::{
            DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWM_WINDOW_CORNER_PREFERENCE,
            DWMWCP_ROUND,
        };
        let pref: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
        unsafe {
            let _ = DwmSetWindowAttribute(
                HWND(h.0),
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &pref as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
            );
        }
    }
    Ok(info)
}

/// WebView2 运行时秒检：缺失时以原生弹窗（不依赖 WebView2 本身）引导微软官方下载后退出。
/// 若不做守卫，缺运行时的机器上工具会呈现空白窗口的"假死"状态。
#[cfg(target_os = "windows")]
fn webview2_guard() {
    if webview2_available() {
        crate::diag::write("I", "WebView2 运行时检测通过");
        return;
    }
    use windows::core::PCWSTR;
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDOK, MB_ICONWARNING, MB_OKCANCEL};
        let text: Vec<u16> = "未检测到 Microsoft WebView2 运行时。\n\n界面显示与威胁预警功能依赖此微软官方组件（安装一次即可）。\n\n「确定」：打开微软官方下载页面（下载 Evergreen Standalone x64 安装包，安装后重新运行本工具）\n「取消」：退出\0".encode_utf16().collect();
        let title: Vec<u16> = "WebView2 运行时缺失\0".encode_utf16().collect();
        let r = MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(title.as_ptr()), MB_OKCANCEL | MB_ICONWARNING);
        if r == IDOK {
            use windows::Win32::UI::Shell::ShellExecuteW;
            use windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD;
            let url: Vec<u16> = "https://developer.microsoft.com/microsoft-edge/webview2/\0".encode_utf16().collect();
            let verb: Vec<u16> = "open\0".encode_utf16().collect();
            ShellExecuteW(None, PCWSTR(verb.as_ptr()), PCWSTR(url.as_ptr()), None, None, SHOW_WINDOW_CMD(5));
        }
    }
    std::process::exit(0);
}

/// WebView2 可用性：官方加载器查询（与 Tauri 实际加载路径一致）+ 注册表 Evergreen 常驻 GUID 兜底
#[cfg(target_os = "windows")]
fn webview2_available() -> bool {
    use windows::core::PCWSTR;
    unsafe {
        use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        let dll = std::env::current_exe().ok().map(|p| p.with_file_name("WebView2Loader.dll"));
        let loaded = match dll {
            Some(p) => {
                let path: Vec<u16> = format!("{}\0", p.display()).encode_utf16().collect();
                LoadLibraryW(PCWSTR(path.as_ptr()))
            }
            None => {
                let name: Vec<u16> = "WebView2Loader.dll\0".encode_utf16().collect();
                LoadLibraryW(PCWSTR(name.as_ptr()))
            }
        };
        if let Ok(h) = loaded {
            if let Some(far) = GetProcAddress(h, windows::core::s!("GetAvailableCoreWebView2BrowserVersionString")) {
                type GetVer = unsafe extern "system" fn(PCWSTR, *mut PCWSTR) -> i32;
                let f: GetVer = std::mem::transmute(far);
                let mut ver = PCWSTR::null();
                if f(PCWSTR::null(), &mut ver) == 0 && !ver.is_null() {
                    return true; // 运行时在位（S_OK 且返回版本串）
                }
                return false;
            }
            // 加载器在但导出异常：保守放行，交给实际加载验证
            return true;
        }
        // 兜底：注册表（HKLM 原生/32 位视图 + HKCU）
        const GUID: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
        use windows::Win32::Foundation::WIN32_ERROR;
        use windows::Win32::System::Registry::{
            RegGetValueW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ,
        };
        let pv_name: Vec<u16> = "pv\0".encode_utf16().collect();
        let pv_name: &'static [u16] = Box::leak(pv_name.into_boxed_slice());
        let PV: &[u16] = pv_name;
        for base in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            for sub in [
                format!("SOFTWARE\\Microsoft\\EdgeUpdate\\Clients\\{GUID}"),
                format!("SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{GUID}"),
            ] {
                let key: Vec<u16> = format!("{sub}\0").encode_utf16().collect();
                let mut buf = [0u16; 64];
                let mut len = (buf.len() * 2) as u32;
                let st = RegGetValueW(
                    HKEY(base.0),
                    PCWSTR(key.as_ptr()),
                    PCWSTR(PV.as_ptr()),
                    RRF_RT_REG_SZ,
                    None,
                    Some(buf.as_mut_ptr() as *mut _),
                    Some(&mut len),
                );
                if st == WIN32_ERROR(0)
                    && buf.iter().take_while(|&&c| c != 0).count() > 0
                {
                    return true;
                }
            }
        }
        false
    }
}

/// v0.9.2：社交激活后的受控退出（前端确认弹窗确认后调用）
#[tauri::command]
fn app_exit_now(app: tauri::AppHandle) {
    app.exit(0);
}

// ---------------------------------------------------------------------------
// v0.9.3 新消息托盘提醒：失焦/驻留时收到社交消息 → 托盘图标闪烁（应用图标 ⇄ 黄色气泡徽章）
// + 任务栏窗口闪烁 + 提示音；点击托盘或主窗回焦即停，并直达对应会话
// ---------------------------------------------------------------------------
struct TrayStore(std::sync::Mutex<tauri::tray::TrayIcon>);

static TRAY_FLASH: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static TRAY_FLASHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false); // 闪烁线程在跑
static TRAY_CTX: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// 32×32 黄色气泡徽章（纯代码绘制，零资源文件）：明黄圆底 + 白色对话气泡
fn badge_icon() -> tauri::image::Image<'static> {
    const W: u32 = 32;
    const H: u32 = 32;
    let mut rgba = vec![0u8; (W * H * 4) as usize];
    let put = |rgba: &mut [u8], x: i32, y: i32, c: [u8; 4]| {
        if x < 0 || y < 0 || x >= W as i32 || y >= H as i32 { return; }
        let i = ((y as u32 * W + x as u32) * 4) as usize;
        rgba[i..i + 4].copy_from_slice(&c);
    };
    let cx = 15.5f32;
    let cy = 15.5f32;
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            if d <= 14.6 {
                // 抗锯齿边缘：14.6-15.4 渐隐
                let a = if d <= 13.8 { 255u8 } else { (255.0 * (15.4 - d) / 1.6).clamp(0.0, 255.0) as u8 };
                put(&mut rgba, x, y, [255, 201, 20, a]); // iOS 黄 #FFC914
            }
        }
    }
    // 白色气泡（圆角矩形 + 左下小尾巴）
    for y in 9..21 {
        for x in 8..24 {
            let corner = (x == 8 || x == 23) && (y == 9 || y == 20);
            if !corner { put(&mut rgba, x, y, [255, 255, 255, 255]); }
        }
    }
    for t in 0..4 { put(&mut rgba, 10 - t / 2, 21 + t % 2 + t / 2, [255, 255, 255, 255]); }
    // 气泡内三个圆点
    for dx in [-4i32, 0, 4] {
        for dy in -1..=1 {
            put(&mut rgba, 16 + dx, 14 + dy, [255, 201, 20, 255]);
        }
    }
    tauri::image::Image::new_owned(rgba, W, H)
}

#[cfg(windows)]
fn flash_taskbar(hwnd: isize, stop: bool) {
    use windows::Win32::UI::WindowsAndMessaging::{
        FlashWindowEx, FLASHWINFO, FLASHW_ALL, FLASHW_STOP, FLASHW_TIMERNOFG,
    };
    let mut info = FLASHWINFO {
        cbSize: std::mem::size_of::<FLASHWINFO>() as u32,
        hwnd: windows::Win32::Foundation::HWND(hwnd as *mut core::ffi::c_void),
        dwFlags: if stop { FLASHW_STOP } else { FLASHW_ALL | FLASHW_TIMERNOFG },
        uCount: 0,
        dwTimeout: 0,
    };
    unsafe { FlashWindowEx(&mut info); }
}

/// 启动闪烁（重复调用幂等）
pub fn social_tray_flash_start(app: &tauri::AppHandle, ctx: String) {
    use std::sync::atomic::Ordering;
    *TRAY_CTX.lock().unwrap_or_else(|e| e.into_inner()) = Some(ctx);
    flash_taskbar(
        app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize).unwrap_or(0),
        false,
    );
    if !TRAY_FLASHING.swap(true, Ordering::SeqCst) {
        // 闪烁线程：图标 ⇄ 徽章 600ms 交替，停止条件命中即还原；
        // v0.9.22 兜底自熄 45 秒 —— 用户离开时（无人触发停止钩子）绝不无限循环耗资源
        std::thread::spawn(move || {
            let badge = badge_icon();
            let t0 = std::time::Instant::now();
            let mut tick = 0u32;
            while TRAY_FLASHING.load(Ordering::SeqCst) {
                if t0.elapsed() > std::time::Duration::from_secs(45) {
                    if let Some(app) = crate::settings::APP.get() { social_tray_flash_stop(&app); }
                    break;
                }
                if let Some(app) = crate::settings::APP.get() {
                    if let Some(store) = app.try_state::<TrayStore>() {
                        let tray = store.0.lock().unwrap_or_else(|e| e.into_inner());
                        if tick % 2 == 0 {
                            let _ = tray.set_icon(Some(badge.clone()));
                            let _ = tray.set_title(Some("💬 新消息"));
                        } else {
                            let _ = tray.set_icon(app.default_window_icon().cloned());
                            let _ = tray.set_title(Option::<&str>::None);
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(600));
                tick += 1;
            }
        });
    }
    TRAY_FLASH.store(true, Ordering::SeqCst);
}

/// 停止闪烁（托盘点击 / 主窗回焦）
pub fn social_tray_flash_stop(app: &tauri::AppHandle) {
    use std::sync::atomic::Ordering;
    if !TRAY_FLASH.swap(false, Ordering::SeqCst) { return; }
    TRAY_FLASHING.store(false, Ordering::SeqCst);
    if let Some(store) = app.try_state::<TrayStore>() {
        let tray = store.0.lock().unwrap_or_else(|e| e.into_inner());
        let _ = tray.set_icon(app.default_window_icon().cloned());
        let _ = tray.set_title(Option::<&str>::None);
    }
    flash_taskbar(
        app.get_webview_window("main").and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize).unwrap_or(0),
        true,
    );
    *TRAY_CTX.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

/// 托盘是否正在闪烁（点击路由用）
pub fn tray_flashing() -> bool {
    TRAY_FLASH.load(std::sync::atomic::Ordering::SeqCst)
}

/// v0.9.10：打开社交面板 = 已开始阅读消息，托盘闪烁即停（不再依赖托盘点击/窗口回焦）
#[tauri::command]
fn social_flash_stop(app: tauri::AppHandle) {
    social_tray_flash_stop(&app);
}

fn main() {
    // 旧版扁平数据布局 → data/ 一次性迁移（必须在任何配置/日志/告警读取之前）
    crate::settings::migrate_legacy();
    // WebView2 运行时秒检（窗口创建之前；缺失则弹窗引导下载并退出）
    #[cfg(target_os = "windows")]
    webview2_guard();
    // 强制 Per-Monitor V2 感知：保证低级钩子物理坐标与 GetWindowRect 同一坐标系
    // （150% 缩放下若被虚拟化，几何判定会整体偏 1.5 倍而永不命中）
    #[cfg(target_os = "windows")]
    unsafe {
        use windows::Win32::UI::HiDpi::{
            SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        };
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    #[cfg(target_os = "windows")]
    {
        auto_elevate();
        enable_privileges();
    }
    tauri::Builder::default()
        // 单实例守护：二次启动 → 唤出主窗、收起浮标，新进程自动退出（防多图标/多进程）
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            use tauri::{Emitter, Manager};
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
            if let Some(f) = app.get_webview_window("float") {
                let _ = f.hide();
            }
            // 二次启动带 --unlock：转发给运行中的实例
            if let Some(path) = extract_unlock_arg(&args.iter().map(|s| s.as_str()).collect::<Vec<_>>()) {
                crate::settings::UNLOCK_REQ.lock().unwrap().replace(path.clone());
                let _ = app.emit("unlock-request", path);
            }
        }))
        .setup(|app| {
            use tauri::Emitter;
            let _ = crate::settings::APP.set(app.handle().clone());
            // 右键菜单直达：首启带 --unlock 参数（存静态，前端挂载时拉取，避免事件早于监听）
            {
                let argv: Vec<String> = std::env::args().skip(1).collect();
                if let Some(path) = extract_unlock_arg(&argv.iter().map(|s| s.as_str()).collect::<Vec<_>>()) {
                    crate::settings::UNLOCK_REQ.lock().unwrap().replace(path.clone());
                    let _ = app.emit("unlock-request", path);
                }
            }
            // v0.9.23 开放推送接口（本地回环 HTTP；密钥开启才接受推送）
            pushapi::start_server();
            // 按已保存偏好恢复可恢复优化项（双击 Ctrl）
            #[cfg(target_os = "windows")]
            optimize::restore_persisted();
            // Sysmon 威胁预警引擎（事件驱动，幂等）
            #[cfg(target_os = "windows")]
            if crate::settings::config().alert_monitor {
                crate::monitor::start(app.handle().clone());
                settings::start_lock_watchdog(); // 配置锁看门狗：卡锁时每秒点名持锁者
            }
            // 分辨率自适应：窗口逻辑尺寸超过所在屏幕 92% 时自动收缩并居中
            // （小屏 / 高缩放笔记本防布局挤压：按钮换行、卡片溢出都是这个根因）
            if let Some(win) = app.get_webview_window("main") {
                if let Ok(Some(mon)) = win.current_monitor() {
                    let scale = mon.scale_factor();
                    let avail_w = (mon.size().width as f64 / scale) * 0.92;
                    let avail_h = (mon.size().height as f64 / scale) * 0.92;
                    if let Ok(sz) = win.inner_size() {
                        let cur_w = sz.width as f64 / scale;
                        let cur_h = sz.height as f64 / scale;
                        if cur_w > avail_w || cur_h > avail_h {
                            use tauri::LogicalSize;
                            let _ = win.set_size(LogicalSize::new(cur_w.min(avail_w), cur_h.min(avail_h)));
                            let _ = win.center();
                        }
                    }
                }
            }
            // 分辨率自适应：窗口逻辑尺寸超过所在屏幕 92% 时自动收缩并居中
            // （小屏 / 高缩放笔记本防布局挤压：按钮换行、卡片溢出都是这个根因）
            if let Some(win) = app.get_webview_window("main") {
                if let Ok(Some(mon)) = win.current_monitor() {
                    let scale = mon.scale_factor();
                    let avail_w = (mon.size().width as f64 / scale) * 0.92;
                    let avail_h = (mon.size().height as f64 / scale) * 0.92;
                    if let Ok(sz) = win.inner_size() {
                        let cur_w = sz.width as f64 / scale;
                        let cur_h = sz.height as f64 / scale;
                        if cur_w > avail_w || cur_h > avail_h {
                            use tauri::LogicalSize;
                            let _ = win.set_size(LogicalSize::new(cur_w.min(avail_w), cur_h.min(avail_h)));
                            let _ = win.center();
                        }
                    }
                }
            }
            // 分辨率自适应：窗口逻辑尺寸超过所在屏幕 92% 时自动收缩并居中
            // （小屏 / 高缩放笔记本防布局挤压：按钮换行、卡片溢出都是这个根因）
            if let Some(win) = app.get_webview_window("main") {
                if let Ok(Some(mon)) = win.current_monitor() {
                    let scale = mon.scale_factor();
                    let avail_w = (mon.size().width as f64 / scale) * 0.92;
                    let avail_h = (mon.size().height as f64 / scale) * 0.92;
                    if let Ok(sz) = win.inner_size() {
                        let cur_w = sz.width as f64 / scale;
                        let cur_h = sz.height as f64 / scale;
                        if cur_w > avail_w || cur_h > avail_h {
                            use tauri::LogicalSize;
                            let _ = win.set_size(LogicalSize::new(cur_w.min(avail_w), cur_h.min(avail_h)));
                            let _ = win.center();
                        }
                    }
                }
            }
            #[cfg(target_os = "windows")]
            {
                use tauri::Manager;
                if let Some(win) = app.get_webview_window("main") {
                    // 浅色主题：Win11 优先 Mica（静态背景，拖动零开销）；
                    // Win10 回退 BlurBehind —— 不要用 Acrylic！Acrylic 在窗口
                    // 拖动时会实时重采样背后内容，造成严重卡顿。
                    if window_vibrancy::apply_mica(&win, Some(false)).is_err() {
                        // BlurBehind：拖动时无重采样，比 Acrylic 流畅得多
                        let _ = window_vibrancy::apply_blur(&win, Some((245, 248, 252, 120)));
                    }
                    // Win11 圆角窗口（Win10 忽略此调用）
                    if let Ok(h) = win.hwnd() {
                        use windows::Win32::Foundation::HWND;
                        use windows::Win32::Graphics::Dwm::{
                            DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE,
                            DWM_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
                        };
                        let pref: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
                        unsafe {
                            let _ = DwmSetWindowAttribute(
                                HWND(h.0),
                                DWMWA_WINDOW_CORNER_PREFERENCE,
                                &pref as *const _ as *const core::ffi::c_void,
                                std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
                            );
                        }
                    }
                }
            }

            // 托盘图标（零后台开销：仅消息泵，无轮询线程）
            {
                let show = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
                let quit = MenuItem::with_id(app, "quit", "退出 青刃 TexMantis", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&show, &quit])?;
                TrayIconBuilder::with_id("main-tray")
                    .icon(app.default_window_icon().unwrap().clone())
                    .tooltip("青刃 TexMantis · 系统精密工具箱")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, ev| match ev.id.as_ref() {
                        "show" => {
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                            if let Some(f) = app.get_webview_window("float") {
                                let _ = f.hide();
                            }
                        }
                        "quit" => {
                            // v0.9.2：社交已激活 → 退出需确认（唤起主窗由前端弹确认框）
                            if social::opened() {
                                if let Some(w) = app.get_webview_window("main") {
                                    let _ = w.show();
                                    let _ = w.unminimize();
                                    let _ = w.set_focus();
                                }
                                use tauri::Emitter;
                                let _ = app.emit("app-exit-request", ());
                            } else {
                                app.exit(0);
                            }
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, ev| {
                        // 左键单击托盘图标 = 显示窗口；闪烁中 = 停止闪烁并直达消息会话
                        if let tauri::tray::TrayIconEvent::Click { button: tauri::tray::MouseButton::Left, button_state: tauri::tray::MouseButtonState::Up, .. } = ev {
                            let app = tray.app_handle();
                            let was_flashing = tray_flashing();
                            if was_flashing {
                                let ctx = TRAY_CTX.lock().map(|g| g.clone()).unwrap_or(None);
                                social_tray_flash_stop(app);
                                use tauri::Emitter;
                                let _ = app.emit("goto-social", ());
                                if let Some(c) = ctx {
                                    let _ = app.emit("social-open-ctx", c);
                                }
                            }
                            if let Some(w) = app.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                            if let Some(f) = app.get_webview_window("float") {
                                let _ = f.hide();
                            }
                        }
                    })
                    .build(app)?;
                // v0.9.3：托盘句柄入库，供"新消息托盘闪烁"切换图标
                if let Some(tray) = app.tray_by_id("main-tray") {
                    let _ = app.manage(TrayStore(std::sync::Mutex::new(tray)));
                }
            }

            // 社交服务随工具启动（默认开；关闭即退，无服务/无驱动/无常驻）
            if settings::config().social_enabled {
                let sh = app.handle().clone();
                std::thread::spawn(move || social::start(sh));
            }

            // 开机即启用贴边浮漂（设置持久化；镜像原子，主线程零锁）
            #[cfg(target_os = "windows")]
            {
                if settings::FLOAT_DOCK_MIRROR.load(std::sync::atomic::Ordering::SeqCst) {
                    let _ = create_float_window(app.handle());
                    // 开机时主窗在前台：浮标创建后立即隐藏，关窗才现身
                    if let Some(f) = app.handle().get_webview_window("float") {
                        let _ = f.hide();
                    }
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // 前台语义统一：主窗失去前台（被其他窗口覆盖 / 最小化 / 隐藏到托盘）→ 浮漂必在场；
            // 主窗回到前台 → 浮漂收起。Focused 事件是三种场景的公共交汇点（show/hide 幂等且轻量）
            if window.label() == "main" {
                if let tauri::WindowEvent::Focused(focused) = event {
                    let focused = *focused;
                    if settings::FLOAT_DOCK_MIRROR.load(std::sync::atomic::Ordering::SeqCst) {
                        if let Some(f) = window.app_handle().get_webview_window("float") {
                            let _ = if focused { f.hide() } else { f.show() };
                        }
                    }
                    // v0.9.3：回焦即视为"开始阅读" → 托盘停止闪烁
                    if focused {
                        social_tray_flash_stop(window.app_handle());
                    }
                }
            }
            // 关闭按钮：优先级 贴边浮漂 > 隐藏到托盘 > 真退出
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let float_dock = settings::FLOAT_DOCK_MIRROR.load(std::sync::atomic::Ordering::SeqCst);
                let close_to_tray = settings::CLOSE_TO_TRAY_MIRROR.load(std::sync::atomic::Ordering::SeqCst);
                // v0.9.2：本会话打开过社交板块 → 关窗默认驻留托盘（社交保持在线），托盘退出需确认
                let social_live = social::opened();
                if float_dock || close_to_tray || social_live {
                    api.prevent_close();
                    let _ = window.hide();
                    if float_dock {
                        let app = window.app_handle();
                        match app.get_webview_window("float") {
                            Some(f) => {
                                let _ = f.show();
                            }
                            None => {
                                // 浮标窗口意外消亡时现场重建（自愈）
                                let _ = create_float_window(app);
                            }
                        }
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_processes,
            commands::kill_process,
            commands::kill_tree,
            commands::get_services,
            commands::service_action,
            commands::get_startup,
            commands::remove_startup,
            commands::get_tasks,
            commands::task_action,
            commands::check_signatures,
            commands::event_query,
            monitor::alerts_get,
            monitor::alerts_clear,
            monitor::alert_set_retention,
            monitor::alert_rule_set,
            monitor::alert_rules_state,
            monitor::alert_monitor_status,
            monitor::alert_engine_toggle,
            monitor::alert_test,
            monitor::alert_selftest,
            commands::alert_sysmon_resync,
            commands::diag_read,
            commands::diag_clear,
            commands::deep_selfcheck,
            commands::main_focused,
            social::social_get_status,
            social::social_scan,
            social::social_set_profile,
            social::social_set_favs,
            social::social_open_session,
            social::social_send_text,
            social::social_history,
            social::social_gen_name,
            social::social_set_layout,
            social::social_fix_firewall,
            // v0.9.0 文件传输 / 群聊 / 广播 / 备注 / 提醒
            social::social_pick_file,
            social::social_send_file,
            social::social_file_accept,
            social::social_file_reject,
            social::social_file_pause,
            social::social_file_resume,
            social::social_file_cancel,
            social::social_file_retry,
            social::social_file_receive_again,
            social::social_broadcast,
            social::social_set_remark,
            social::social_create_group,
            social::social_group_send,
            social::social_group_invite,
            social::social_group_kick,
            social::social_group_set_admin,
            social::social_group_rename,
            social::social_group_leave,
            social::social_group_dissolve,
            social::social_group_nudge,
            social::social_group_files,
            social::social_notify_sound,
            social::social_pick_dir,
            social::social_dirs,
            social::social_set_recv_dir,
            social::social_open_path,
            social::social_remove_peer,
            social::social_set_zombie,
            social::social_set_font_size,
            social::social_clear_history,
            social::social_data_usage,
            social::social_clear_all_history,
            social::social_clear_recv_files,
            social::social_clear_chat_files,
            social::social_list_dir,
            social::social_shake,
            social::social_mark_read,
            social::social_read_all,
            pushapi::api_key_gen,
            pushapi::api_key_clear,
            pushapi::api_state,
            social::social_reveal_path,
            social_flash_stop,
            // v0.9.2 受控退出
            app_exit_now,
            commands::alert_wl_get,
            commands::alert_wl_set,
            monitor::alert_mute_set,
            monitor::notifier_take_pending,
            monitor::notifier_open_main,
            monitor::notifier_fit,
            commands::alert_sysmon_install,
            commands::alert_import_config,
            commands::is_elevated,
            commands::open_folder,
            commands::unlock_scan,
            commands::force_delete,
            optimize::opt_get_all,
            optimize::opt_apply,
            optimize::opt_set_ctrl_window,
            optimize::opt_set_ctx_menu,
            network::net_connections,
            network::geo_get_status,
            network::geo_set_db_path,
            network::geo_lookup_ips,
            settings::settings_get,
            settings::settings_set_auto_start,
            settings::settings_set_flag,
            settings::unlock_take_request,
            settings::float_dock_set,
            settings::float_slide,
            settings::float_page_ready,
            settings::float_summon,
            commands::hash_file,
            commands::open_url
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
