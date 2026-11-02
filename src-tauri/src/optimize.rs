//! 系统优化模块：桌面图标显示 / 任务栏双击显示桌面 / 资源管理器右键终端
//!
//! - 桌面图标：HKCU HideDesktopIcons\NewStartPanel（1=隐藏，值不存在=显示）+ SHChangeNotify 刷新
//! - 双击任务栏显示桌面：WH_MOUSE_LL 低级钩子检测 Shell_TrayWnd 上的双击，
//!   连续两次独立 Ctrl 按下（间隔 ≤ 双击时限、期间无其它按键）经线程消息
//!   异步调用官方 API IShellDispatch4::ToggleDesktop（工具运行期间生效）
//! - 右键终端：HKCU\Software\Classes\{Directory\Background,Directory,Drive}\shell 下
//!   写入独立菜单项（免管理员、可随时开关卸载），%V 由 explorer 展开为当前目录

use serde::Serialize;

#[derive(Serialize, Clone)]
pub struct OptState {
    pub desktop_this_pc: bool,
    pub desktop_recycle_bin: bool,
    pub desktop_control_panel: bool,
    pub desktop_network: bool,
    pub taskbar_dbl_desktop: bool,
    pub ctx_power_shell: bool,
    pub ctx_cmd: bool,
    pub ctx_unlock: bool,
    pub ctrl_window_ms: u32,
}

const NEW_START_PANEL: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\HideDesktopIcons\NewStartPanel";
const CLASSIC_START_MENU: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\HideDesktopIcons\ClassicStartMenu";
const CLSID_THIS_PC: &str = "{20D04FE0-3AEA-1069-A2D8-08002B30309D}";
const CLSID_RECYCLE: &str = "{645FF040-5081-101B-9F08-00AA002F954E}";
const CLSID_CONTROL: &str = "{5399E694-6CE5-4D6C-8FCE-1D8870FDCBA0}";
const CLSID_NETWORK: &str = "{F02C1A0D-BE21-4350-88B0-7367FC96EF3C}";

#[cfg(windows)]
fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ---------------------------------------------------------------------------
// 注册表原子操作（HKCU）
// ---------------------------------------------------------------------------
#[cfg(windows)]
pub(crate) mod reg {
    use super::w;
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegDeleteTreeW, RegGetValueW, RegOpenKeyExW, RegSetKeyValueW, HKEY, HKEY_CURRENT_USER,
        KEY_QUERY_VALUE, RRF_RT_REG_DWORD,
    };
    use windows::Win32::UI::Shell::SHDeleteValueW;

    const REG_SZ: u32 = 1;
    const REG_DWORD: u32 = 4;

    pub fn get_dword(subkey: &str, value: &str) -> Option<u32> {
        let sk = w(subkey);
        let vn = w(value);
        let mut out: u32 = 0;
        let mut cb = 4u32;
        let err = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                PCWSTR(sk.as_ptr()),
                PCWSTR(vn.as_ptr()),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut out as *mut u32 as *mut core::ffi::c_void),
                Some(&mut cb),
            )
        };
        if err == ERROR_SUCCESS && cb == 4 {
            Some(out)
        } else {
            None
        }
    }

    /// 写命名值（REG_SZ）
    pub fn set_sz(subkey: &str, value: &str, data: &str) -> Result<(), String> {
        let sk = w(subkey);
        let vn = w(value);
        let bytes = w(data);
        let err = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(sk.as_ptr()),
                PCWSTR(vn.as_ptr()),
                REG_SZ,
                Some(bytes.as_ptr() as *const core::ffi::c_void),
                (bytes.len() * 2) as u32,
            )
        };
        if err != ERROR_SUCCESS {
            Err(format!("写入注册表失败（错误码 {}）", err.0))
        } else {
            Ok(())
        }
    }

    pub fn set_dword(subkey: &str, value: &str, data: u32) -> Result<(), String> {
        let sk = w(subkey);
        let vn = w(value);
        let err = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(sk.as_ptr()),
                PCWSTR(vn.as_ptr()),
                REG_DWORD,
                Some(&data as *const u32 as *const core::ffi::c_void),
                4,
            )
        };
        if err != ERROR_SUCCESS {
            Err(format!("写入注册表失败（错误码 {}）", err.0))
        } else {
            Ok(())
        }
    }

    pub fn delete_value(subkey: &str, value: &str) -> Result<(), String> {
        let sk = w(subkey);
        let vn = w(value);
        let err = unsafe { SHDeleteValueW(HKEY_CURRENT_USER, PCWSTR(sk.as_ptr()), PCWSTR(vn.as_ptr())) };
        // 值/键不存在（2）视为成功
        if err != ERROR_SUCCESS && err.0 != 2 {
            return Err(format!("删除注册表值失败（错误码 {}）", err.0));
        }
        Ok(())
    }

    pub fn set_default_sz(subkey: &str, data: &str) -> Result<(), String> {
        let sk = w(subkey);
        let bytes = w(data);
        let err = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(sk.as_ptr()),
                PCWSTR::null(), // 默认值
                REG_SZ,
                Some(bytes.as_ptr() as *const core::ffi::c_void),
                (bytes.len() * 2) as u32,
            )
        };
        if err != ERROR_SUCCESS {
            Err(format!("写入注册表失败（错误码 {}）", err.0))
        } else {
            Ok(())
        }
    }

    pub fn delete_tree(subkey: &str) -> Result<(), String> {
        let sk = w(subkey);
        let err = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(sk.as_ptr())) };
        if err != ERROR_SUCCESS && err.0 != 2 {
            return Err(format!("删除注册表键失败（错误码 {}）", err.0));
        }
        Ok(())
    }

    /// 键是否存在
    pub fn key_exists(subkey: &str) -> bool {
        let sk = w(subkey);
        let mut hk = HKEY::default();
        let err = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(sk.as_ptr()),
                0,
                KEY_QUERY_VALUE,
                &mut hk,
            )
        };
        if err == ERROR_SUCCESS {
            unsafe {
                let _ = windows::Win32::System::Registry::RegCloseKey(hk);
            }
            true
        } else {
            false
        }
    }

    /// 指定键下是否存在某个值（如 Run 键中的 TexMantis）
    pub fn key_exists_run(base: &str, value: &str) -> bool {
        let sk = w(base);
        let vn = w(value);
        let mut hk = HKEY::default();
        let err = unsafe {
            RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(sk.as_ptr()), 0, KEY_QUERY_VALUE, &mut hk)
        };
        if err != ERROR_SUCCESS {
            return false;
        }
        let mut buf = [0u8; 8];
        let mut cb = 8u32;
        let e2 = unsafe {
            windows::Win32::System::Registry::RegQueryValueExW(
                hk,
                PCWSTR(vn.as_ptr()),
                None,
                None,
                Some(buf.as_mut_ptr()),
                Some(&mut cb),
            )
        };
        unsafe {
            let _ = windows::Win32::System::Registry::RegCloseKey(hk);
        }
        // 值存在即可（无论类型）；REG_SZ 长度与 4 不符时 cb 会反映实际长度，仍视为存在
        e2 == ERROR_SUCCESS
    }
}

// ---------------------------------------------------------------------------
// 桌面显示（快捷方式方式）：在用户桌面创建指向系统命名空间的 .lnk，
// 即时生效、删除即还原，全程不触碰 explorer 进程
// ---------------------------------------------------------------------------
#[cfg(windows)]
const DESKTOP_LINKS: &[(&str, &str, &str)] = &[
    // (key, 显示名, 命名空间目标)
    ("this_pc", "此电脑", "::{20D04FE0-3AEA-1069-A2D8-08002B30309D}"),
    ("recycle_bin", "回收站", "::{645FF040-5081-101B-9F08-00AA002F954E}"),
    ("control_panel", "控制面板", "::{21EC2020-3AEA-1069-A2DD-08002B30309D}"),
    ("network", "网络", "::{F02C1A0D-BE21-4350-88B0-7367FC96EF3C}"),
];

/// 用户桌面目录（SHGetKnownFolderPath，兼容 OneDrive 重定向，零硬编码）
#[cfg(windows)]
fn desktop_dir() -> Result<std::path::PathBuf, String> {
    unsafe {
        use windows::Win32::System::Com::CoTaskMemFree;
        use windows::Win32::UI::Shell::{
            SHGetKnownFolderPath, FOLDERID_Desktop, KNOWN_FOLDER_FLAG,
        };
        let mut pw = SHGetKnownFolderPath(&FOLDERID_Desktop, KNOWN_FOLDER_FLAG(0), None)
            .map_err(|e| format!("解析桌面目录失败：{e}"))?;
        // 计算以 NUL 结尾的长度
        let mut n = 0usize;
        while *pw.0.add(n) != 0 {
            n += 1;
        }
        let s = std::slice::from_raw_parts(pw.0, n);
        let path = String::from_utf16_lossy(s);
        CoTaskMemFree(Some(pw.0 as *const core::ffi::c_void));
        Ok(std::path::PathBuf::from(path))
    }
}

#[cfg(windows)]
fn desktop_link_path(name: &str) -> Result<std::path::PathBuf, String> {
    Ok(desktop_dir()?.join(format!("{name}.lnk")))
}

#[cfg(windows)]
fn desktop_link_exists(name: &str) -> bool {
    desktop_link_path(name).map(|p| p.exists()).unwrap_or(false)
}

/// 创建 .lnk：IShellLink + SetIDList（命名空间目标）+ IPersistFile::Save
#[cfg(windows)]
fn create_desktop_link(display: &str, target: &str) -> Result<(), String> {
    unsafe {
        use windows::core::PCWSTR;
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::UI::Shell::{IShellLinkW, ShellLink, SHParseDisplayName};

        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        // S_OK=0 / S_FALSE=1：需配对 CoUninitialize；RPC_E_CHANGED_MODE：沿用现有套间即可
        let owned = hr.0 >= 0 && hr.0 != -2147417850 && (hr.0 == 0 || hr.0 == 1);
        let _ = hr;
        let result = (|| -> Result<(), String> {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("创建 COM 对象失败：{e}"))?;
            let target_w: Vec<u16> = format!("{target}\0").encode_utf16().collect();
            let mut pidl: *mut windows::Win32::UI::Shell::Common::ITEMIDLIST = std::ptr::null_mut();
            SHParseDisplayName(PCWSTR(target_w.as_ptr()), None, &mut pidl, 0, None)
                .map_err(|e| format!("解析目标失败：{e}"))?;
            let r = link.SetIDList(pidl);
            CoTaskMemFree(Some(pidl as *const core::ffi::c_void));
            r.map_err(|e| format!("设置目标失败：{e}"))?;

            let pf: windows::Win32::System::Com::IPersistFile =
                windows::core::Interface::cast(&link)
                    .map_err(|e| format!("获取持久化接口失败：{e}"))?;
            let dir = desktop_dir()?;
            let path = dir.join(format!("{display}.lnk"));
            let path_w: Vec<u16> = format!("{}\0", path.to_string_lossy()).encode_utf16().collect();
            pf.Save(PCWSTR(path_w.as_ptr()), true)
                .map_err(|e| format!("写入 {display}.lnk 失败：{e}"))
        })();
        if owned {
            windows::Win32::System::Com::CoUninitialize();
        }
        result
    }
}

// ---------------------------------------------------------------------------
// 双击 Ctrl 切换显示桌面（WH_KEYBOARD_LL 全局键盘钩子）
// ---------------------------------------------------------------------------
#[cfg(windows)]
// ---------------------------------------------------------------------------
// 钩子链路诊断日志（工具同目录，opt_apply 与 taskbar 共用）
// ---------------------------------------------------------------------------
pub(crate) static LOG_PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// 日志路径：优先工具同目录（exe 旁，绿色工具直观可见），失败回退 %TEMP%
pub(crate) fn hook_log_path() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("texmantis-hook.log");
            if std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&p)
                .is_ok()
            {
                return p;
            }
        }
    }
    std::env::temp_dir().join("texmantis-hook.log")
}

/// 追加一行日志（带相对时间戳）；诊断开关关闭时为空操作
pub(crate) fn hook_log(msg: &str) {
    if !crate::settings::diag_log_enabled() {
        return;
    }
    use std::io::Write;
    static T0: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let t0 = T0.load(std::sync::atomic::Ordering::Relaxed);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if t0 == 0 {
        T0.store(now_ms, std::sync::atomic::Ordering::Relaxed);
    }
    let el = now_ms.saturating_sub(if t0 == 0 { now_ms } else { t0 });
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(LOG_PATH.get_or_init(hook_log_path))
    {
        let _ = writeln!(f, "[+{:>6}ms] {}", el, msg);
    }
}

#[cfg(windows)]
mod taskbar {
    use super::{hook_log, hook_log_path, LOG_PATH};
    use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
    use std::sync::Mutex;
    use std::time::Instant;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
        UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_QUIT,
    };

    const WM_APP_TOGGLE_DESKTOP: u32 = 0x8001; // WM_APP + 1


    pub static HOOK_HANDLE: AtomicIsize = AtomicIsize::new(0);
    pub static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
    /// 主通道：合成 Win+D（原生"显示桌面"热键，explorer 处理，不受提权影响）。
    /// 触发瞬间第二下 Ctrl 还物理按着，必须先注入 Ctrl 抬起清空逻辑状态，
    /// 否则系统会看到 Ctrl+Win+D = 新建虚拟桌面！
    unsafe fn send_win_d() -> bool {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VK_CONTROL, VK_D,
            VK_LWIN,
        };
        let mk = |vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY,
                  up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS(0) },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let inputs = [
            mk(VK_CONTROL, true),
            mk(VK_LWIN, false),
            mk(VK_D, false),
            mk(VK_D, true),
            mk(VK_LWIN, true),
        ];
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) == 5
    }

    /// 官方"显示桌面"切换（与任务栏角落按钮一致）：Shell.Application → IShellDispatch4
    unsafe fn toggle_desktop_com() -> Result<(), String> {
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED,
        };
        use windows::Win32::UI::Shell::IShellDispatch4;
        // Shell.Application 的 CLSID（shldisp）
        const CLSID_SHELL_APP: windows::core::GUID =
            windows::core::GUID::from_u128(0x13709620_C279_11CE_A49E_444553540000);
        let hr = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let owned = hr.0 == 0 || hr.0 == 1; // S_OK / S_FALSE 需配对 CoUninitialize
        let r = (|| -> Result<(), String> {
            let shell: IShellDispatch4 = CoCreateInstance(&CLSID_SHELL_APP, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| format!("COM 对象创建失败：{e}"))?;
            shell
                .ToggleDesktop()
                .map_err(|e| format!("切换桌面失败：{e}"))
        })();
        if owned {
            CoUninitialize();
        }
        r
    }

    static CTRL_DOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    /// v0.9.23 权威就绪标志：true = 钩子已安装且在运行。
    /// 修复：HOOK_THREAD 的 0/u32::MAX/真实tid 三态对 is_active/stop/opt_apply 同时可见，
    /// 安装窗口期内 stop() 静默丢失关闭请求 + 观测态被持久化 → "设置未开启却生效"。
    static HOOK_READY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static PENDING: Mutex<Option<Instant>> = Mutex::new(None);

    unsafe extern "system" fn keybd_ll(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        use windows::Win32::UI::Input::KeyboardAndMouse::{VK_LCONTROL, VK_RCONTROL};
        use windows::Win32::UI::WindowsAndMessaging::{
            WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
        };
        if code >= 0 {
            let info = &*(lp.0 as *const KBDLLHOOKSTRUCT);
            let vk = info.vkCode;
            let is_ctrl = vk == VK_LCONTROL.0 as u32 || vk == VK_RCONTROL.0 as u32;
            let is_down = wp.0 as u32 == WM_KEYDOWN || wp.0 as u32 == WM_SYSKEYDOWN;
            let is_up = wp.0 as u32 == WM_KEYUP || wp.0 as u32 == WM_SYSKEYUP;
            if is_ctrl {
                if is_down {
                    // 仅统计"独立按下"：按住不放的自动重复（CTRL_DOWN 仍为 true）忽略
                    if !CTRL_DOWN.swap(true, Ordering::Relaxed) {
                        let now = Instant::now();
                        let win = crate::settings::ctrl_dbl_window_ms() as u128;
                        let mut pend = PENDING.lock().unwrap();
                        let elapsed = match *pend {
                            Some(t) => now.duration_since(t).as_millis(),
                            None => 0,
                        };
                        let fire = matches!(*pend, Some(t) if elapsed <= win);
                        if fire {
                            *pend = None;
                            hook_log(&format!("双击 Ctrl 命中（间隔 {}ms / 时限 {}ms）", elapsed, win));
                            crate::settings::app_notify("已捕获双击 Ctrl");
                            // 钩子回调内只投递线程消息，切换在钩子线程消息循环执行
                            let _ = PostThreadMessageW(
                                HOOK_THREAD.load(Ordering::Relaxed),
                                WM_APP_TOGGLE_DESKTOP,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        } else {
                            if pend.is_some() {
                                hook_log(&format!(
                                    "间隔 {}ms 超出时限 {}ms，重新计时",
                                    elapsed, win
                                ));
                            }
                            *pend = Some(now);
                        }
                    }
                } else if is_up {
                    CTRL_DOWN.store(false, Ordering::Relaxed);
                }
            } else if is_down {
                // 两次 Ctrl 之间按了其它键 → 作废（避免 Ctrl+C / Ctrl+V 等组合误触）
                *PENDING.lock().unwrap() = None;
            }
        }
        CallNextHookEx(HHOOK::default(), code, wp, lp)
    }

    pub fn is_active() -> bool {
        let tid = HOOK_THREAD.load(Ordering::Relaxed);
        HOOK_READY.load(Ordering::Relaxed) && tid != 0 && tid != u32::MAX
    }

    pub fn start() -> Result<(), String> {
        if HOOK_THREAD
            .compare_exchange(0, u32::MAX, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return Ok(()); // 已在运行
        }
        if crate::settings::diag_log_enabled() {
            let header = format!("===== 钩子诊断日志 v{} =====\n", env!("CARGO_PKG_VERSION"));
            let _ = std::fs::write(LOG_PATH.get_or_init(hook_log_path), &header);
            hook_log("start(): 进入启用流程");
        }
        std::thread::spawn(|| unsafe {
            // WH_KEYBOARD_LL 全局低级键盘钩子：hmod 传 None 即可
            let hook = match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keybd_ll), None, 0) {
                Ok(h) => h,
                Err(e) => {
                    HOOK_THREAD.store(0, Ordering::Relaxed);
                    hook_log(&format!("SetWindowsHookExW 失败：{e}"));
                    return;
                }
            };
            hook_log("WH_KEYBOARD_LL 安装成功（双击 Ctrl 监听中）");
            HOOK_HANDLE.store(hook.0 as isize, Ordering::Relaxed);
            HOOK_THREAD.store(GetCurrentThreadId(), Ordering::Relaxed);
            HOOK_READY.store(true, Ordering::Relaxed);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
                if msg.message == WM_APP_TOGGLE_DESKTOP {
                    hook_log("收到 WM_APP_TOGGLE_DESKTOP，执行切换");
                    // 主通道：Win+D 注入（COM ToggleDesktop 在提权进程内会返回 Ok 却静默无效）
                    if unsafe { send_win_d() } {
                        hook_log("Win+D 注入完成，桌面切换");
                    } else if let Err(e) = unsafe { toggle_desktop_com() } {
                        hook_log(&format!("Win+D 注入失败且 ToggleDesktop 失败：{e}"));
                        crate::settings::app_notify(&format!("切换失败：{e}"));
                    } else {
                        hook_log("Win+D 注入失败，已用 ToggleDesktop 兜底");
                    }
                }
            }
            HOOK_READY.store(false, Ordering::Relaxed);
            let _ = UnhookWindowsHookEx(hook);
            HOOK_HANDLE.store(0, Ordering::Relaxed);
            HOOK_THREAD.store(0, Ordering::Relaxed);
        });
        // v0.9.23 同步等待明确的安装结果（成功 / 失败二值，绝不返回过渡态）：
        // HOOK_READY=true = 安装成功；HOOK_THREAD 归 0 = 安装失败；2s 超时 = 系统异常
        for _ in 0..200 {
            if HOOK_READY.load(Ordering::Relaxed) {
                return Ok(());
            }
            if HOOK_THREAD.load(Ordering::Relaxed) == 0 {
                return Err("键盘钩子安装失败（可能被系统策略限制）".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = HOOK_THREAD.compare_exchange(u32::MAX, 0, Ordering::Relaxed, Ordering::Relaxed);
        Err("键盘钩子安装超时".to_string())
    }

    pub fn stop() -> Result<(), String> {
        // v0.9.23 同步等待卸载完成：此前对过渡态静默不投递 → 关闭请求丢失、钩子带配置继续存活
        for _ in 0..200 {
            let tid = HOOK_THREAD.load(Ordering::Relaxed);
            if !HOOK_READY.load(Ordering::Relaxed) && tid == 0 {
                return Ok(()); // 已完全停止
            }
            if tid != 0 && tid != u32::MAX {
                unsafe {
                    let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 资源管理器右键终端
// ---------------------------------------------------------------------------
#[cfg(windows)]
const CTX_ROOTS: &[&str] = &[
    r"Software\Classes\Directory\Background\shell",
    r"Software\Classes\Directory\shell",
    r"Software\Classes\Drive\shell",
];

#[cfg(windows)]
fn ctx_leaf(kind: &str) -> &'static str {
    match kind {
        "ps" => "TexMantis.OpenPowerShell",
        "cmd" => "TexMantis.OpenCMD",
        _ => "TexMantis.Unlock",
    }
}

/// 旧版 AK.* 键名清理（历史遗留）
#[cfg(windows)]
fn ctx_cleanup_legacy() {
    for legacy in ["AK.OpenPowerShell", "AK.OpenCMD"] {
        for root in CTX_ROOTS {
            let _ = reg::delete_tree(&format!(r"{}\{}", root, legacy));
        }
    }
}

#[cfg(windows)]
fn ctx_installed(kind: &str) -> bool {
    reg::key_exists(&format!(r"{}\{}\command", CTX_ROOTS[0], ctx_leaf(kind)))
}

#[cfg(windows)]
fn ctx_set(kind: &str, on: bool) -> Result<(), String> {
    let leaf = ctx_leaf(kind);
    let (mui, icon, mk_command): (String, String, Box<dyn Fn(&str) -> String>) = match kind {
        "ps" => (
            "在此处打开 PowerShell".into(),
            "powershell.exe".into(),
            Box::new(|_| r#"powershell.exe -NoProfile -NoExit -Command "Set-Location -LiteralPath '%V'""#.into()),
        ),
        "cmd" => (
            "在此处打开 CMD".into(),
            "cmd.exe".into(),
            Box::new(|_| r#"cmd.exe /s /k pushd "%V""#.into()),
        ),
        _ => {
            let exe = std::env::current_exe()
                .map_err(|e| format!("无法定位程序：{e}"))?
                .display()
                .to_string();
            (
                "强制解锁/删除 - 青刃".into(),
                exe.clone(),
                Box::new(move |arg| format!("\"{}\" --unlock \"{}\"", exe, arg)),
            )
        }
    };
    // 解锁菜单作用于文件与文件夹（含文件夹空白处）；终端菜单作用于文件夹
    let roots: &[&str] = match kind {
        "unlock" => &[
            r"Software\Classes\*\shell",
            r"Software\Classes\Directory\shell",
            r"Software\Classes\Directory\Background\shell",
        ],
        _ => CTX_ROOTS,
    };
    for root in roots {
        let base = format!(r"{}\{}", root, leaf);
        if on {
            // 动词键默认值 = 菜单显示文字；Icon 为动词键上的命名值
            reg::set_default_sz(&base, &mui)?;
            reg::set_sz(&base, "Icon", &icon)?;
            let arg = if root.contains("Background") { "%V" } else { "%1" };
            reg::set_default_sz(&format!(r"{}\command", base), &mk_command(arg))?;
        } else {
            reg::delete_tree(&base)?;
        }
    }
    if kind == "ps" || kind == "cmd" {
        ctx_cleanup_legacy();
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tauri 命令
// ---------------------------------------------------------------------------
/// 启动时按已保存偏好恢复可恢复的优化项（双击 Ctrl 钩子随进程存活）
#[cfg(windows)]
pub fn restore_persisted() {
    if crate::settings::config().opt_ctrl_dbl {
        let _ = taskbar::start();
        hook_log("已按偏好自动恢复双击 Ctrl 监听");
    }
}

#[tauri::command]
pub async fn opt_get_all() -> Result<OptState, String> {
    #[cfg(windows)]
    {
        Ok(OptState {
            desktop_this_pc: desktop_link_exists("此电脑"),
            desktop_recycle_bin: desktop_link_exists("回收站"),
            desktop_control_panel: desktop_link_exists("控制面板"),
            desktop_network: desktop_link_exists("网络"),
            taskbar_dbl_desktop: taskbar::is_active(),
            ctx_power_shell: ctx_installed("ps"),
            ctx_cmd: ctx_installed("cmd"),
            ctx_unlock: ctx_installed("unlock"),
            ctrl_window_ms: crate::settings::ctrl_dbl_window_ms(),
        })
    }
    #[cfg(not(windows))]
    {
        Ok(OptState {
            desktop_this_pc: false,
            desktop_recycle_bin: false,
            desktop_control_panel: false,
            desktop_network: false,
            taskbar_dbl_desktop: false,
            ctx_power_shell: false,
            ctx_cmd: false,
            ctx_unlock: false,
            ctrl_window_ms: 600,
        })
    }
}

/// 设置双击 Ctrl 触发时限（ms，100~2000）
#[tauri::command]
pub async fn opt_set_ctrl_window(ms: u32) -> Result<(), String> {
    #[cfg(windows)]
    {
        crate::settings::set_ctrl_dbl_window_ms(ms)?;
        crate::optimize::hook_log(&format!("双击 Ctrl 时限调整为 {}ms", ms));
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = ms;
        Ok(())
    }
}

/// 一键优化：按勾选状态批量应用（桌面快捷方式 + 任务栏钩子 + 右键终端）
#[tauri::command]
pub async fn opt_apply(
    desktop: Vec<String>,
    taskbar: bool,
    ctx_ps: bool,
    ctx_cmd: bool,
    unlock_ctx: bool,
) -> Result<OptState, String> {
    #[cfg(windows)]
    {
        if crate::settings::diag_log_enabled() {
            let header = format!(
                "===== 钩子诊断日志 v{} =====\nopt_apply 进入：desktop={:?} taskbar={} ctx_ps={} ctx_cmd={}\n",
                env!("CARGO_PKG_VERSION"),
                desktop,
                taskbar,
                ctx_ps,
                ctx_cmd
            );
            let _ = std::fs::write(LOG_PATH.get_or_init(hook_log_path), &header);
        }
        // 1) 桌面快捷方式：勾选 = 创建，未勾选 = 移除
        for (key, display, target) in DESKTOP_LINKS {
            let wanted = desktop.iter().any(|k| k == key);
            let exists = desktop_link_exists(display);
            if wanted && !exists {
                create_desktop_link(display, target)?;
            } else if !wanted && exists {
                let p = desktop_link_path(display)?;
                std::fs::remove_file(&p).map_err(|e| format!("移除 {display}.lnk 失败：{e}"))?;
            }
        }
        // 2) 任务栏双击钩子
        hook_log("桌面快捷方式段完成，进入任务栏段");
        let taskbar_active = if taskbar {
            taskbar::start()?;
            let a = taskbar::is_active();
            if !a {
                return Err("任务栏钩子安装失败（可能被系统策略限制）".to_string());
            }
            a
        } else {
            taskbar::stop()?;
            for _ in 0..100 {
                if !taskbar::is_active() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            taskbar::is_active()
        };
        hook_log(&format!(
            "任务栏段完成 is_active={}",
            taskbar_active
        ));
        // 3) 资源管理器右键终端 + 解锁直达菜单
        ctx_set("ps", ctx_ps)?;
        ctx_set("cmd", ctx_cmd)?;
        ctx_set("unlock", unlock_ctx)?;
        crate::settings::update_config(|c| {
            c.opt_ctrl_dbl = taskbar; // v0.9.23 持久化请求态（此前写观测态：关闭失败会误存 true）
            c.opt_ctx_ps = ctx_ps;
            c.opt_ctx_cmd = ctx_cmd;
            c.opt_unlock_ctx = unlock_ctx;
        });
        return Ok(OptState {
            desktop_this_pc: desktop_link_exists("此电脑"),
            desktop_recycle_bin: desktop_link_exists("回收站"),
            desktop_control_panel: desktop_link_exists("控制面板"),
            desktop_network: desktop_link_exists("网络"),
            taskbar_dbl_desktop: taskbar_active,
            ctx_power_shell: ctx_installed("ps"),
            ctx_cmd: ctx_installed("cmd"),
            ctx_unlock: ctx_installed("unlock"),
            ctrl_window_ms: crate::settings::ctrl_dbl_window_ms(),
        });
    }
    #[cfg(not(windows))]
    {
        let _ = (desktop, taskbar, ctx_ps, ctx_cmd, unlock_ctx);
        Ok(OptState {
            desktop_this_pc: false,
            desktop_recycle_bin: false,
            desktop_control_panel: false,
            desktop_network: false,
            taskbar_dbl_desktop: false,
            ctx_power_shell: false,
            ctx_cmd: false,
            ctx_unlock: false,
            ctrl_window_ms: 600,
        })
    }
}

#[tauri::command]
pub async fn opt_set_ctx_menu(kind: String, on: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        match kind.as_str() {
            "ps" | "cmd" => ctx_set(&kind, on),
            other => Err(format!("未知类型：{other}")),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (kind, on);
        Ok(())
    }
}
