//! 青刃 TexMantis Rust 后端 —— 进程/服务/启动项/计划任务 的安全应急管理命令。
//!
//! 设计原则：能用原生 API 的用原生（sysinfo / TerminateProcess / token 提权检查），
//! 其余（服务、计划任务、注册表启动项、数字签名）走 PowerShell 一次性调用并解析 JSON，
//! 以最小代码覆盖最强功能。

use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;
use std::collections::HashMap;
use std::process::Command;
use std::sync::{LazyLock, Mutex};

pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Windows 下隐藏控制台窗口（CREATE_NO_WINDOW）；其他平台为空实现。
fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

// ---------------------------------------------------------------------------
// JSON 空值容忍反序列化（PowerShell ConvertTo-Json 会输出 null）
// ---------------------------------------------------------------------------
fn de_null_str<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(Option::<String>::deserialize(d)?.unwrap_or_default())
}
fn de_null_u32<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    Ok(Option::<u32>::deserialize(d)?.unwrap_or(0))
}

// ---------------------------------------------------------------------------
// 进程快照（sysinfo，2 次刷新以获得 CPU 增量）
// ---------------------------------------------------------------------------
use sysinfo::{ProcessesToUpdate, System, MINIMUM_CPU_UPDATE_INTERVAL};

static SYS: LazyLock<Mutex<System>> = LazyLock::new(|| Mutex::new(System::new_all()));

#[derive(Serialize, Clone)]
pub struct ProcInfo {
    pub pid: u32,
    pub ppid: Option<u32>,
    pub name: String,
    pub cpu: f32,
    pub mem_mb: f64,
    pub status: String,
    pub exe: String,
    pub cmd: String,
    pub started: u64,
}

#[tauri::command]
pub async fn get_processes() -> Result<Vec<ProcInfo>, String> {
    let mut sys = SYS.lock().map_err(|e| e.to_string())?;
    sys.refresh_processes(ProcessesToUpdate::All, true);
    std::thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL);
    sys.refresh_processes(ProcessesToUpdate::All, true);
    Ok(sys
        .processes()
        .values()
        .map(|p| ProcInfo {
            pid: p.pid().as_u32(),
            ppid: p.parent().map(|x| x.as_u32()),
            name: p.name().to_string_lossy().to_string(),
            cpu: p.cpu_usage(),
            mem_mb: p.memory() as f64 / 1024.0 / 1024.0,
            status: format!("{:?}", p.status()),
            exe: p.exe().map(|e| e.display().to_string()).unwrap_or_default(),
            cmd: p.cmd().iter().map(|c| c.to_string_lossy().to_string()).collect::<Vec<_>>().join(" "),
            started: p.start_time(),
        })
        .collect())
}

// ---------------------------------------------------------------------------
// 结束进程 / 结束进程树
// ---------------------------------------------------------------------------
#[cfg(windows)]
unsafe fn terminate_pid(pid: u32) -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
    let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) else {
        return false;
    };
    let ok = TerminateProcess(h, 1).is_ok();
    let _ = CloseHandle(h);
    ok
}
#[cfg(not(windows))]
unsafe fn terminate_pid(_pid: u32) -> bool {
    false
}

fn kill_impl(pid: u32, tree: bool) -> Result<String, String> {
    if pid < 10 {
        return Err(format!("拒绝操作关键系统进程 (PID {pid})"));
    }
    let mut targets = vec![pid];
    if tree {
        let sys = SYS.lock().map_err(|e| e.to_string())?;
        let mut children: HashMap<u32, Vec<u32>> = HashMap::new();
        for p in sys.processes().values() {
            if let Some(pp) = p.parent() {
                children.entry(pp.as_u32()).or_default().push(p.pid().as_u32());
            }
        }
        drop(sys);
        let mut i = 0;
        while i < targets.len() {
            if let Some(kids) = children.get(&targets[i]) {
                for &k in kids {
                    if !targets.contains(&k) {
                        targets.push(k);
                    }
                }
            }
            i += 1;
        }
    }
    let mut killed = 0usize;
    for &t in targets.iter().rev() {
        if unsafe { terminate_pid(t) } {
            killed += 1;
        }
    }
    if killed == 0 {
        // 权限不足等场景回退 taskkill /T /F
        let mut cmd = Command::new("taskkill");
        cmd.args(["/PID", &pid.to_string(), "/T", "/F"]);
        hide_window(&mut cmd);
        let out = cmd
            .output()
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(format!("taskkill 已结束进程树（根 PID {pid}）"));
        }
        return Err(format!("结束进程失败：{}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(format!(
        "已结束 {killed} 个进程{}",
        if tree { "（含进程树）" } else { "" }
    ))
}

#[tauri::command]
pub async fn kill_process(pid: u32) -> Result<String, String> {
    kill_impl(pid, false)
}

#[tauri::command]
pub async fn kill_tree(pid: u32) -> Result<String, String> {
    kill_impl(pid, true)
}

// ---------------------------------------------------------------------------
// PowerShell 辅助
// ---------------------------------------------------------------------------
pub(crate) fn ps(script: &str) -> Result<String, String> {
    use std::io::Read;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script]);
    hide_window(&mut cmd);
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 PowerShell 失败：{e}"))?;

    // 后台线程持续读取管道，避免输出量大时管道写满导致子进程阻塞
    let mut so = child.stdout.take().unwrap();
    let mut se = child.stderr.take().unwrap();
    let t1 = std::thread::spawn(move || { let mut b = Vec::new(); let _ = so.read_to_end(&mut b); b });
    let t2 = std::thread::spawn(move || { let mut b = Vec::new(); let _ = se.read_to_end(&mut b); b });

    // 轮询等待 + 超时保护，杜绝"一直加载中"
    let start = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(st) => break Some(st),
            None => {
                if start.elapsed() > Duration::from_secs(90) {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = t1.join();
                    let _ = t2.join();
                    return Err("PowerShell 执行超时（90 秒），已强制终止".into());
                }
                std::thread::sleep(Duration::from_millis(60));
            }
        }
    };

    let stdout = t1.join().map_err(|_| "读取 stdout 失败".to_string())?;
    let stderr = t2.join().map_err(|_| "读取 stderr 失败".to_string())?;
    let out_text = String::from_utf8_lossy(&stdout).to_string();
    if status.map(|s| s.success()).unwrap_or(false) {
        Ok(out_text)
    } else {
        Err(format!("PowerShell 错误：{}\n{}", String::from_utf8_lossy(&stderr), out_text))
    }
}


// ---------------------------------------------------------------------------
// Sysmon 一键安装 / 自定义配置导入（威胁预警数据源）
// ---------------------------------------------------------------------------
/// 与引擎规则配套的最小化采集配置：仅采集规则所需事件源，日志量与资源占用最小化
pub(crate) const SYSMON_CONFIG_XML: &str = r##"<Sysmon schemaversion="4.90">
  <!-- 青刃 TexMantis 内置规则配套采集配置（高频事件定向过滤，低频高价值事件全量） -->
  <!-- TexMantis Capture Profile v6 -->
  <HashAlgorithms>sha256</HashAlgorithms>
  <EventFiltering>
    <!-- 进程创建：全量采集（低频高价值事件，普通主机每天数百条 <1MB；
         红队命令行规则依赖完整命令行可见性）。高频事件（模块/文件/注册表）仍定向过滤 -->
    <ProcessCreate onmatch="exclude">
      <Image condition="begin with">C:\Windows\System32\conhost.exe</Image>
    </ProcessCreate>
    <ImageLoad onmatch="include">
      <ImageLoaded condition="contains">\Temp\</ImageLoaded>
    </ImageLoad>
    <CreateRemoteThread onmatch="exclude">
      <SourceImage condition="begin with">C:\Windows\</SourceImage>
    </CreateRemoteThread>
    <ProcessAccess onmatch="include">
      <TargetImage condition="end with">lsass.exe</TargetImage>
    </ProcessAccess>
    <FileCreate onmatch="include">
      <TargetFilename condition="contains">Startup</TargetFilename>
      <TargetFilename condition="contains">\Temp\</TargetFilename>
    </FileCreate>
    <RegistryEvent onmatch="include">
      <TargetObject condition="contains">CurrentVersion\Run</TargetObject>
      <TargetObject condition="contains">DisableAntiSpyware</TargetObject>
      <TargetObject condition="contains">Windows Defender</TargetObject>
      <TargetObject condition="contains">Image File Execution Options</TargetObject>
      <TargetObject condition="contains">\Winlogon\</TargetObject>
      <TargetObject condition="contains">AppInit_DLLs</TargetObject>
      <TargetObject condition="contains">AppInit_X64_DLLs</TargetObject>
      <TargetObject condition="contains">ExtensionInstallForcelist</TargetObject>
      <TargetObject condition="contains">DefaultSearchProvider</TargetObject>
      <TargetObject condition="contains">RestoreOnStartup</TargetObject>
      <TargetObject condition="contains">StartupPages</TargetObject>
      <TargetObject condition="contains">Homepage</TargetObject>
      <TargetObject condition="contains">SearchScopes</TargetObject>
      <TargetObject condition="contains">Start Page</TargetObject>
    </RegistryEvent>
  </EventFiltering>
</Sysmon>
"##;

#[cfg(windows)]
fn sysmon_dir() -> Result<std::path::PathBuf, String> {
    let dir = crate::settings::config_path()
        .map(|p| p.with_file_name("sysmon"))
        .ok_or("无法定位工具目录")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建目录失败：{e}"))?;
    Ok(dir)
}

#[cfg(windows)]
fn sysmon_install_with_config(exe: &std::path::Path, cfg: &std::path::Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new(exe)
        .args(["-accepteula", "-i"])
        .arg(cfg)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("启动 Sysmon 安装失败：{e}"))?;
    if !out.status.success() {
        let msg = format!("{}{}", decode_proc_output(&out.stderr), decode_proc_output(&out.stdout));
        let msg: String = msg.trim().chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect();
        crate::diag::write("E", &format!("Sysmon -i 失败: {}", msg.trim()));
        return Err(format!("Sysmon 安装失败：{}", msg.trim()));
    }
    Ok(())
}

#[cfg(windows)]
fn sysmon_download(dir: &std::path::Path, app: &tauri::AppHandle) -> Result<(), String> {
    use std::io::{BufRead, BufReader};
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    let zip = dir.join("Sysmon.zip");
    let script = format!(
        "$ErrorActionPreference='Stop';\
         [Net.ServicePointManager]::SecurityProtocol=[Net.SecurityProtocolType]::Tls12;\
         $u='https://download.sysinternals.com/files/Sysmon.zip';\
         $o='{}';\
         $r=[Net.HttpWebRequest]::Create($u);\
         $resp=$r.GetResponse();$total=$resp.ContentLength;$rs=$resp.GetResponseStream();\
         $fs=[IO.File]::Create($o);$buf=New-Object byte[] 65536;$sum=0;\
         while(($n=$rs.Read($buf,0,$buf.Length)) -gt 0){{\
           $fs.Write($buf,0,$n);$sum+=$n;\
           if($total -gt 0){{ Write-Output ('PROGRESS ' + [int]($sum*100/$total)) }}\
         }};\
         $fs.Close();$rs.Close();$resp.Close();\
         if($total -gt 0 -and $sum -ne $total){{ throw 'download incomplete' }};\
         Expand-Archive -Path $o -DestinationPath '{}' -Force;\
         Write-Output 'DOWNLOAD_OK'",
        zip.display(), dir.display()
    );
    let mut child = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动下载失败：{e}"))?;
    if let Some(out) = child.stdout.take() {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if let Some(pg) = line.strip_prefix("PROGRESS ") {
                if let Ok(n) = pg.trim().parse::<u8>() {
                    emit_install_progress(app, n.min(98), &format!("下载中 {n}%"));
                }
            }
        }
    }
    let out = child.wait_with_output().map_err(|e| format!("等待下载失败：{e}"))?;
    if !out.status.success() {
        let msg = decode_proc_output(&out.stderr);
        let msg = msg.trim();
        return Err(if msg.is_empty() {
            "下载失败（网络错误或官方源不可达）".into()
        } else {
            format!("下载失败：{msg}")
        });
    }
    let exe = dir.join("Sysmon64.exe");
    if !exe.exists() {
        return Err("下载完成但解压后未找到 Sysmon64.exe".into());
    }
    Ok(())
}

#[cfg(windows)]
fn sysmon_install_impl(app: &tauri::AppHandle) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    let prog = |pct: u8, msg: &str| emit_install_progress(app, pct, msg);
    let dir = sysmon_dir()?;
    let exe = dir.join("Sysmon64.exe");
    if !exe.exists() {
        prog(2, "开始下载 Sysmon（微软官方源）…");
        sysmon_download(&dir, app)?;
    } else {
        prog(45, "本机已有 Sysmon 安装包，跳过下载");
    }
    let cfgp = dir.join("texmantis-default.xml");
    std::fs::write(&cfgp, SYSMON_CONFIG_XML).map_err(|e| format!("写入采集配置失败：{e}"))?;
    prog(55, "检查已有 Sysmon 服务…");
    // 根因修复：服务已注册时 -i 必然报 already registered —— 先卸载再装（告警记录保留）
    if sysmon_service_registered() {
        if exe.exists() {
            prog(58, "检测到已有 Sysmon：先卸载旧实例（告警记录保留）…");
            let _ = std::process::Command::new(&exe)
                .args(["-u", "force"])
                .creation_flags(CREATE_NO_WINDOW)
                .output();
        } else {
            prog(58, "清理残留的 Sysmon 服务注册…");
            let _ = std::process::Command::new("sc").args(["stop", "Sysmon64"]).creation_flags(CREATE_NO_WINDOW).output();
            let _ = std::process::Command::new("sc").args(["delete", "Sysmon64"]).creation_flags(CREATE_NO_WINDOW).output();
        }
        for _ in 0..20 {
            if !sysmon_service_registered() { break; }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    }
    crate::diag::write("I", &format!("一键安装开始（exe 已存在={}）", exe.exists()));
    prog(70, "安装 Sysmon 服务并应用内置采集配置…");
    sysmon_install_with_config(&exe, &cfgp)?;
    crate::diag::write("I", "Sysmon -i 安装成功");
    crate::monitor::reset_subscriptions(); // 重装重建通道：旧订阅句柄已失效，强制重订
    prog(88, "配置事件通道容量上限…");
    // 事件日志为环形覆盖（写满自动淘汰最旧）：统一钉住通道上限 128MB，杜绝磁盘无限占用
    let _ = std::process::Command::new("wevtutil")
        .args(["sl", "Microsoft-Windows-Sysmon/Operational", "/ms:134217728"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    // 立即唤醒预警引擎重连（不等基线重试周期）
    crate::monitor::wake_engine();
    prog(100, "完成：引擎已接管");
    crate::monitor::health_store(crate::monitor::sysmon_health()); // 安装完立即刷新健康快照（不等巡检周期）
    std::thread::sleep(std::time::Duration::from_millis(800));
    if crate::monitor::sysmon_ok() {
        Ok("Sysmon 安装并配置完成：内置最小化采集规则已启用，威胁预警引擎即时接管（无需重启工具）".into())
    } else {
        Ok("Sysmon 已安装，事件通道初始化中（约数秒），稍后刷新即可生效".into())
    }
}

/// 安装进度事件（前端进度条消费）
#[cfg(windows)]
fn emit_install_progress(app: &tauri::AppHandle, pct: u8, msg: &str) {
    use tauri::Emitter;
    let _ = app.emit("sysmon-progress", serde_json::json!({ "pct": pct, "msg": msg }));
}

/// Sysmon64 服务是否已注册（存在即视为注册，含停止态）
#[cfg(windows)]
fn sysmon_service_registered() -> bool {
    use std::os::windows::process::CommandExt;
    std::process::Command::new("sc")
        .args(["query", "Sysmon64"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Sysinternals 工具重定向到管道时输出 UTF-16LE（直连控制台才是 ANSI）：
/// 按 NUL 字节占比启发式识别并按 UTF-16 解码，否则按 UTF-8
#[cfg(windows)]
pub(crate) fn decode_proc_output(b: &[u8]) -> String {
    let n = b.len().min(64);
    if n >= 4 {
        let zeros = b[..n].iter().filter(|&&x| x == 0).count();
        if zeros * 4 > n {
            let u: Vec<u16> = b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            return String::from_utf16_lossy(&u);
        }
    }
    String::from_utf8_lossy(b).into_owned()
}

/// 已装 Sysmon 时静默重应用内置配置（工具升级后配置自动跟进；后台线程不阻塞）
#[cfg(windows)]
pub(crate) fn sysmon_sync_if_installed() {
    std::thread::Builder::new().name("sysmon-sync".into()).spawn(|| {
        // 自检 = 巡检比对：部署文件已含当前版本标记则跳过，绝不每次启动无条件重写
        if cfg_deployed_current() {
            crate::diag::write("D", "配置自检：部署文件已是当前版，跳过重写");
            return;
        }
        if apply_sysmon_config() {
            crate::diag::write("I", "配置自修复：部署文件缺失/过期 → 已重写并应用");
            let _ = crate::monitor::wake_engine();
        } else {
            crate::diag::write("W", "配置自修复：重写/应用失败（Sysmon 未安装或不可执行）");
        }
    }).ok();
}

/// 部署文件一致性检查：含当前版本标记即视为已部署（不比对实时配置，避免误判反复重写）
#[cfg(windows)]
pub(crate) fn cfg_deployed_current() -> bool {
    crate::settings::config_path()
        .map(|p| p.with_file_name("sysmon").join("texmantis-default.xml"))
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| s.contains("TexMantis Capture Profile v6"))
        .unwrap_or(false)
}

/// 重写内置采集配置并让 Sysmon 立即生效（返回是否成功应用）
#[cfg(windows)]
pub(crate) fn apply_sysmon_config() -> bool {
    let Some(dir) = crate::settings::config_path().map(|p| p.with_file_name("sysmon")) else {
        return false;
    };
    let exe = dir.join("Sysmon64.exe");
    let cfgp = dir.join("texmantis-default.xml");
    if !exe.exists() {
        return false;
    }
    if std::fs::write(&cfgp, SYSMON_CONFIG_XML).is_err() {
        return false;
    }
    use std::os::windows::process::CommandExt;
    let ok = std::process::Command::new(&exe)
        .args(["-c"])
        .arg(&cfgp)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    crate::diag::write("I", &format!("重应用内置采集配置到 Sysmon ok={ok}（exe={}）", exe.display()));
    ok
}

/// 深度自检结果项
#[derive(serde::Serialize, Clone)]
pub struct CheckItem {
    pub name: String,
    pub ok: bool,
    pub detail: String,
    pub fixed: bool,
}

/// 深度自检：逐层核查采集链路（服务/部署/活性/通道/审计/订阅），可修复项当场修复并标注
#[tauri::command]
pub async fn deep_selfcheck() -> Vec<CheckItem> {
    let mut out: Vec<CheckItem> = Vec::new();
    // 1) Sysmon 服务
    let ok = crate::monitor::sysmon_ok();
    out.push(CheckItem {
        name: "Sysmon 服务".into(),
        ok,
        detail: if ok { "Sysmon64 与 SysmonDrv 正在运行".into() } else { "服务未就绪 —— 请使用「重装 Sysmon（修复用）」".into() },
        fixed: false,
    });
    // 2) 配置部署一致性（漂移才修复，不无条件重写）
    let cur = cfg_deployed_current();
    let (ok2, fixed2) = if cur {
        (true, false)
    } else {
        let r = apply_sysmon_config();
        (r, r)
    };
    out.push(CheckItem {
        name: "采集配置部署".into(),
        ok: ok2,
        fixed: fixed2,
        detail: if ok2 && fixed2 { "部署文件过期/缺失 → 已重写并应用到 Sysmon".into() } else if ok2 { "部署文件与内置配置一致".into() } else { "部署失败（Sysmon 未安装）".into() },
    });
    // 3) 采集活性（独立于引擎订阅的铁证）
    match crate::monitor::sysmon_channel_fresh_events() {
        Some(n) if n > 0 => out.push(CheckItem {
            name: "采集活性".into(),
            ok: true,
            fixed: false,
            detail: format!("Sysmon 通道近 30 分钟已有 {} 条事件，采集正常", if n >= 8 { "8+".to_string() } else { n.to_string() }),
        }),
        Some(_) => out.push(CheckItem {
            name: "采集活性".into(),
            ok: false,
            fixed: false,
            detail: "近 30 分钟 Sysmon 通道 0 条新事件：Sysmon 未在产出事件——建议「重装 Sysmon（修复用）」".into(),
        }),
        None => out.push(CheckItem {
            name: "采集活性".into(),
            ok: false,
            fixed: false,
            detail: "无法查询 Sysmon 通道（服务未运行或通道不存在）".into(),
        }),
    }
    // 4) 通道启用态（禁用 → 修复）
    for (name, enabled, fixed) in crate::monitor::ensure_channels_enabled() {
        out.push(CheckItem {
            name: format!("通道 · {name}"),
            ok: enabled,
            fixed,
            detail: if fixed { "原被禁用 → 已自动启用".into() } else if enabled { "已启用".into() } else { "启用失败（需管理员权限）".into() },
        });
    }
    // 5) 审计策略（关闭 → 修复）
    let (aok, afixed) = crate::monitor::ensure_audit_policy();
    out.push(CheckItem {
        name: "账户管理审计".into(),
        ok: aok,
        fixed: afixed,
        detail: if afixed { "原被关闭 → 已启用成功审计（4720/4732 依赖）".into() } else if aok { "已启用".into() } else { "启用失败（需管理员权限）".into() },
    });
    // 6) 引擎订阅状态
    let subs = crate::monitor::chan_states();
    let n_ok = subs.iter().filter(|(_, ok)| *ok).count();
    out.push(CheckItem {
        name: "引擎订阅".into(),
        ok: n_ok == subs.len(),
        fixed: false,
        detail: format!("{}/{} 通道已建立实时订阅（每分钟自动重试）", n_ok, subs.len()),
    });
    crate::diag::write("I", &format!("深度自检完成：{}/{} 项正常", out.iter().filter(|c| c.ok).count(), out.len()));
    out
}

/// 威胁预警白名单（系统来源过滤 + 自定义程序/目标）
#[derive(serde::Serialize, Clone)]
pub struct AlertWl {
    pub sys_src: bool,
    pub procs: Vec<String>,
    pub targets: Vec<String>,
}

#[tauri::command]
pub async fn alert_wl_get() -> Result<AlertWl, String> {
    let c = crate::settings::config();
    Ok(AlertWl { sys_src: c.wl_sys_src, procs: c.wl_procs.clone(), targets: c.wl_targets.clone() })
}

#[tauri::command]
pub async fn alert_wl_set(sys_src: bool, procs: Vec<String>, targets: Vec<String>) -> Result<(), String> {
    crate::settings::update_config(|c| {
        c.wl_sys_src = sys_src;
        c.wl_procs = procs.into_iter().map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
        c.wl_targets = targets.into_iter().map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect();
    });
    crate::diag::write("I", "白名单已更新（系统来源/程序/目标），对新增事件即时生效");
    Ok(())
}

/// 读取诊断日志尾部（预警页「诊断日志」弹窗）
#[tauri::command]
pub async fn diag_read() -> Result<String, String> {
    Ok(crate::diag::tail(160_000))
}

/// 浮漂自愈看门狗用：主窗是否处于前台（浮漂可见期间轮询，回焦未收到事件时兜底收起）
#[tauri::command]
pub fn main_focused(app: tauri::AppHandle) -> bool {
    use tauri::Manager;
    app.get_webview_window("main")
        .and_then(|m| m.is_focused().ok())
        .unwrap_or(false)
}

/// 清空诊断日志
#[tauri::command]
pub async fn diag_clear() -> Result<(), String> {
    crate::diag::clear();
    Ok(())
}

/// 重新同步内置采集配置到本机 Sysmon（不重装，立即生效并刷新健康快照）
#[cfg(windows)]
#[tauri::command]
pub async fn alert_sysmon_resync() -> Result<String, String> {
    if apply_sysmon_config() {
        crate::monitor::health_store(crate::monitor::sysmon_health());
        let _ = crate::monitor::wake_engine();
        Ok("已重新应用内置采集配置到 Sysmon，健康状态已刷新".into())
    } else {
        Err("重同步失败：未找到 Sysmon64.exe 或应用配置被拒绝（可能未安装）".into())
    }
}

#[cfg(not(windows))]
#[tauri::command]
pub async fn alert_sysmon_resync() -> Result<String, String> { Ok("仅支持 Windows".into()) }

/// 一键下载 + 安装 + 自动配置 Sysmon（微软官方源，内置规则配套配置）
/// 全程通过 sysmon-progress 事件上报阶段进度（下载为真实百分比）
#[cfg(windows)]
#[tauri::command]
pub async fn alert_sysmon_install(app: tauri::AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || sysmon_install_impl(&app))
        .await
        .map_err(|e| format!("任务失败：{e}"))?
}

#[cfg(windows)]
fn sysmon_import_impl(xml: String) -> Result<String, String> {
    let x = xml.trim();
    if x.len() < 20 || !x.contains("<Sysmon") || !x.contains("</Sysmon>") {
        return Err("不是有效的 Sysmon 配置文件（缺少 <Sysmon> 根元素）".into());
    }
    let dir = sysmon_dir()?;
    let cfgp = dir.join("custom-config.xml");
    std::fs::write(&cfgp, x).map_err(|e| format!("保存配置失败：{e}"))?;
    // 第一道校验：.NET XML 解析器严格验证格式
    ps(&format!("[xml](Get-Content -Raw '{}') | Out-Null;'XML_OK'", cfgp.display()))
        .map_err(|_| "XML 解析失败：文件格式不正确".to_string())?;
    let exe = dir.join("Sysmon64.exe");
    if !exe.exists() {
        return Ok("校验通过（XML 格式有效）。尚未安装 Sysmon：请先「一键安装 Sysmon」，之后再次导入即可应用。".into());
    }
    // 第二道校验：Sysmon 自身校验规则语义并应用
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new(&exe)
        .args(["-c"])
        .arg(&cfgp)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("应用配置失败：{e}"))?;
    if !out.status.success() {
        let msg = format!("{}{}", decode_proc_output(&out.stderr), decode_proc_output(&out.stdout));
        return Err(format!("Sysmon 拒绝了该配置：{}", msg.trim()));
    }
    Ok("自定义 Sysmon 配置已应用（引擎规则开关仍独立生效）".into())
}

/// 导入自定义 Sysmon 配置：双重校验（XML 解析 + Sysmon 引擎自身校验）后应用
#[cfg(windows)]
#[tauri::command]
pub async fn alert_import_config(xml: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || sysmon_import_impl(xml))
        .await
        .map_err(|e| format!("任务失败：{e}"))?
}

#[cfg(not(windows))]
#[tauri::command]
pub async fn alert_sysmon_install() -> Result<String, String> { Err("仅支持 Windows".into()) }

#[cfg(not(windows))]
#[tauri::command]
pub async fn alert_import_config(_xml: String) -> Result<String, String> { Err("仅支持 Windows".into()) }

fn parse_json_array<T: DeserializeOwned>(raw: &str) -> Result<Vec<T>, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Ok(Vec::new());
    }
    let v: serde_json::Value = serde_json::from_str(s).map_err(|e| format!("解析失败：{e}\n{s}"))?;
    let arr = match v {
        serde_json::Value::Array(a) => a,
        other => vec![other],
    };
    arr.into_iter()
        .map(|item| serde_json::from_value(item).map_err(|e| e.to_string()))
        .collect()
}

fn ps_quote(s: &str) -> String {
    s.replace('\'', "''")
}

// ---------------------------------------------------------------------------
// 服务（Get-CimInstance Win32_Service + 公司信息）
// ---------------------------------------------------------------------------
#[derive(Serialize, Deserialize, Clone)]
pub struct SvcInfo {
    #[serde(default, deserialize_with = "de_null_str", alias = "Name")]
    pub name: String,
    #[serde(default, deserialize_with = "de_null_str", alias = "Display")]
    pub display: String,
    #[serde(default, deserialize_with = "de_null_str", alias = "State")]
    pub state: String,
    #[serde(default, deserialize_with = "de_null_str", alias = "StartMode")]
    pub start_mode: String,
    #[serde(default, deserialize_with = "de_null_str", alias = "Account")]
    pub account: String,
    #[serde(default, alias = "ProcessId")]
    pub pid: Option<u32>,
    #[serde(default, deserialize_with = "de_null_str", alias = "Exe")]
    pub exe: String,
    #[serde(default, deserialize_with = "de_null_str", alias = "Company")]
    pub company: String,
    #[serde(default, alias = "IsMicrosoft")]
    pub is_microsoft: bool,
}

const SERVICES_PS: &str = r#"[Console]::OutputEncoding=[System.Text.Encoding]::UTF8
$cache=@{}
@(Get-CimInstance Win32_Service | ForEach-Object {
  $exe=''
  if ($_.PathName) {
    if ($_.PathName -match '^(?:")?([^"]+?\.exe)(?:")?') { $exe=$Matches[1] }
    else { $exe=($_.PathName -split '\s+-|\s+/')[0].Trim('"') }
  }
  $company=''
  if ($exe -and -not $cache.ContainsKey($exe)) {
    $cache[$exe]=[string]((Get-Item $exe -ErrorAction SilentlyContinue).VersionInfo.CompanyName)
  }
  $company=$cache[$exe]
  [pscustomobject]@{ Name=[string]$_.Name; Display=[string]$_.DisplayName; State=[string]$_.State; StartMode=[string]$_.StartMode; Account=[string]$_.StartName; ProcessId=$_.ProcessId; Exe=[string]$exe; Company=[string]$company; IsMicrosoft=($company -match 'Microsoft') }
}) | ConvertTo-Json -Compress -Depth 3"#;

#[tauri::command]
pub async fn get_services() -> Result<Vec<SvcInfo>, String> {
    parse_json_array(&ps(SERVICES_PS)?)
}

#[tauri::command]
pub async fn service_action(name: String, action: String) -> Result<String, String> {
    let n = ps_quote(&name);
    let cmd = match action.as_str() {
        "start" => format!("Start-Service -Name '{n}'"),
        "stop" => format!("Stop-Service -Name '{n}' -Force"),
        "auto" => format!("Set-Service -Name '{n}' -StartupType Automatic"),
        "manual" => format!("Set-Service -Name '{n}' -StartupType Manual"),
        "disabled" => format!("Set-Service -Name '{n}' -StartupType Disabled"),
        "delete" => format!("& sc.exe delete '{n}'"),
        other => return Err(format!("未知操作：{other}")),
    };
    let script = format!("[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; $ErrorActionPreference='Stop'; {cmd}");
    let out = ps(&script)?;
    let trimmed = out.trim();
    Ok(if trimmed.is_empty() { "操作成功".to_string() } else { trimmed.to_string() })
}

// ---------------------------------------------------------------------------
// 启动项（注册表 Run/RunOnce 各视图 + 启动文件夹）
// ---------------------------------------------------------------------------
#[derive(Serialize, Deserialize, Clone)]
pub struct StartupInfo {
    #[serde(default, deserialize_with = "de_null_str", alias = "Hive")]
    pub hive: String,
    #[serde(default, deserialize_with = "de_null_str", alias = "Name")]
    pub name: String,
    #[serde(default, deserialize_with = "de_null_str", alias = "Command")]
    pub command: String,
    #[serde(default, deserialize_with = "de_null_str", alias = "Kind")]
    pub kind: String,
}

const STARTUP_PS: &str = r#"[Console]::OutputEncoding=[System.Text.Encoding]::UTF8
$out=@()
$keys=@('HKLM:\Software\Microsoft\Windows\CurrentVersion\Run','HKLM:\Software\Microsoft\Windows\CurrentVersion\RunOnce','HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Run','HKCU:\Software\Microsoft\Windows\CurrentVersion\Run','HKCU:\Software\Microsoft\Windows\CurrentVersion\RunOnce')
foreach ($k in $keys) {
  if (Test-Path $k) {
    $p=Get-ItemProperty -Path $k
    foreach ($prop in $p.PSObject.Properties) {
      if ($prop.Name -notlike 'PS*') {
        $out += [pscustomobject]@{ Hive=[string]$k; Name=[string]$prop.Name; Command=[string]$prop.Value; Kind='注册表' }
      }
    }
  }
}
foreach ($f in @([Environment]::GetFolderPath('CommonStartup'), [Environment]::GetFolderPath('Startup'))) {
  if ($f -and (Test-Path $f)) {
    Get-ChildItem -Path $f -File -ErrorAction SilentlyContinue | ForEach-Object {
      $out += [pscustomobject]@{ Hive=$f; Name=$_.Name; Command=$_.FullName; Kind='启动文件夹' }
    }
  }
}
$out | ConvertTo-Json -Compress -Depth 3"#;

#[tauri::command]
pub async fn get_startup() -> Result<Vec<StartupInfo>, String> {
    parse_json_array(&ps(STARTUP_PS)?)
}

#[tauri::command]
pub async fn remove_startup(kind: String, hive: String, name: String, command: String) -> Result<String, String> {
    let script = if kind == "注册表" {
        format!(
            "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; $ErrorActionPreference='Stop'; Remove-ItemProperty -Path '{}' -Name '{}'",
            ps_quote(&hive),
            ps_quote(&name)
        )
    } else {
        format!(
            "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; $ErrorActionPreference='Stop'; Remove-Item -LiteralPath '{}' -Force",
            ps_quote(&command)
        )
    };
    ps(&script)?;
    Ok(format!("已删除启动项「{name}」"))
}

// ---------------------------------------------------------------------------
// 计划任务（非 Microsoft 路径）
// ---------------------------------------------------------------------------
#[derive(Serialize, Deserialize, Clone)]
pub struct TaskInfo {
    #[serde(default, deserialize_with = "de_null_str", alias = "Path")]
    pub path: String,
    #[serde(default, deserialize_with = "de_null_str", alias = "Name")]
    pub name: String,
    #[serde(default, deserialize_with = "de_null_str", alias = "State")]
    pub state: String,
    #[serde(default, deserialize_with = "de_null_str", alias = "Author")]
    pub author: String,
    #[serde(default, deserialize_with = "de_null_str", alias = "Exe")]
    pub exe: String,
    #[serde(default, deserialize_with = "de_null_str", alias = "Args")]
    pub args: String,
    #[serde(default, deserialize_with = "de_null_u32", alias = "TriggerCount")]
    pub trigger_count: u32,
}

const TASKS_PS: &str = r#"[Console]::OutputEncoding=[System.Text.Encoding]::UTF8
@(Get-ScheduledTask | Where-Object { $_.TaskPath -notlike '\Microsoft*' } | ForEach-Object {
  $a=@($_.Actions) | Select-Object -First 1
  [pscustomobject]@{ Path=$_.TaskPath; Name=$_.TaskName; State=[string]$_.State; Author=[string]$_.Author; Exe=[string]$a.Execute; Args=[string]$a.Arguments; TriggerCount=@($_.Triggers).Count }
}) | ConvertTo-Json -Compress -Depth 3"#;

#[tauri::command]
pub async fn get_tasks() -> Result<Vec<TaskInfo>, String> {
    parse_json_array(&ps(TASKS_PS)?)
}

#[tauri::command]
pub async fn task_action(path: String, name: String, action: String) -> Result<String, String> {
    let tn = format!("{path}{name}");
    let script = format!(
        "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; $ErrorActionPreference='Stop'; $t='{}'; {}",
        ps_quote(&tn),
        match action.as_str() {
            "run" => "& schtasks.exe /Run /TN $t",
            "end" => "& schtasks.exe /End /TN $t",
            "enable" => "& schtasks.exe /Change /TN $t /ENABLE",
            "disable" => "& schtasks.exe /Change /TN $t /DISABLE",
            "delete" => "& schtasks.exe /Delete /TN $t /F",
            other => return Err(format!("未知操作：{other}")),
        }
    );
    let out = ps(&script)?;
    let trimmed = out.trim();
    Ok(if trimmed.is_empty() { "操作成功".to_string() } else { trimmed.to_string() })
}

// ---------------------------------------------------------------------------
// 数字签名批量检测（Get-AuthenticodeSignature）
// ---------------------------------------------------------------------------
#[derive(Serialize, Deserialize, Clone)]
pub struct SigInfo {
    #[serde(default, deserialize_with = "de_null_str", alias = "Path")]
    pub path: String,
    #[serde(default, deserialize_with = "de_null_str", alias = "Status")]
    pub status: String,
    #[serde(default, deserialize_with = "de_null_str", alias = "Signer")]
    pub signer: String,
}

#[tauri::command]
pub async fn check_signatures(paths: Vec<String>) -> Result<Vec<SigInfo>, String> {
    #[cfg(windows)]
    {
        tauri::async_runtime::spawn_blocking(move || {
            paths.iter().map(|p| native_sig(p)).collect::<Vec<_>>()
        })
        .await
        .map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        Ok(paths
            .into_iter()
            .map(|p| SigInfo { path: p, status: "Unknown".into(), signer: String::new() })
            .collect())
    }
}

/// 原生 Authenticode 检测：WinVerifyTrust + 签名者提取（无 PowerShell 依赖，毫秒级）
#[cfg(windows)]
fn native_sig(path: &str) -> SigInfo {
    let (status, signer) = unsafe {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::Security::WinTrust::{
            WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA,
            WINTRUST_FILE_INFO, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_IGNORE,
            WTD_UI_NONE, WTD_UICONTEXT_EXECUTE,
        };
        let path_w: Vec<u16> = format!("{path}\0").encode_utf16().collect();
        let mut file_info = WINTRUST_FILE_INFO::default();
        file_info.cbStruct = std::mem::size_of::<WINTRUST_FILE_INFO>() as u32;
        file_info.pcwszFilePath = windows::core::PCWSTR(path_w.as_ptr());
        file_info.hFile = HANDLE::default();
        let mut wtd = WINTRUST_DATA::default();
        wtd.cbStruct = std::mem::size_of::<WINTRUST_DATA>() as u32;
        wtd.dwUIChoice = WTD_UI_NONE;
        wtd.fdwRevocationChecks = WTD_REVOKE_NONE;
        wtd.dwUnionChoice = WTD_CHOICE_FILE;
        wtd.dwStateAction = WTD_STATEACTION_IGNORE;
        wtd.dwUIContext = WTD_UICONTEXT_EXECUTE;
        wtd.Anonymous.pFile = &mut file_info;
        let hr = WinVerifyTrust(
            windows::Win32::Foundation::HWND::default(),
            &WINTRUST_ACTION_GENERIC_VERIFY_V2 as *const windows::core::GUID
                as *mut windows::core::GUID,
            &mut wtd as *mut _ as *mut core::ffi::c_void,
        );
        match hr {
            0 => ("Valid".to_string(), extract_signer(path)),
            e if e == 0x800B0100u32 as i32 => ("NotSigned".to_string(), None),
            _ => ("Invalid".to_string(), extract_signer(path)),
        }
    };
    SigInfo {
        path: path.to_string(),
        status: status.to_string(),
        signer: signer.unwrap_or_default(),
    }
}

/// 签名者名称：CryptQueryObject → CryptMsgGetParam → CertFindCertificateInStore → CertGetNameString
#[cfg(windows)]
unsafe fn extract_signer(path: &str) -> Option<String> {
    use windows::Win32::Security::Cryptography::*;
    let path_w: Vec<u16> = format!("{path}\0").encode_utf16().collect();
    let mut store = HCERTSTORE::default();
    let mut msg: *mut core::ffi::c_void = std::ptr::null_mut();
    CryptQueryObject(
        CERT_QUERY_OBJECT_FILE,
        path_w.as_ptr() as *const core::ffi::c_void,
        CERT_QUERY_CONTENT_FLAG_ALL,
        CERT_QUERY_FORMAT_FLAG_BINARY,
        0,
        None,
        None,
        None,
        Some(&mut store),
        Some(&mut msg),
        None,
    )
    .ok()?;
    let mut len = 0u32;
    if CryptMsgGetParam(msg, CMSG_SIGNER_INFO_PARAM, 0, None, &mut len).is_err() {
        let _ = CryptMsgClose(Some(msg));
        let _ = CertCloseStore(store, 0);
        return None;
    }
    let mut buf = vec![0u8; len as usize];
    let r = CryptMsgGetParam(msg, CMSG_SIGNER_INFO_PARAM, 0, Some(buf.as_mut_ptr() as *mut core::ffi::c_void), &mut len);
    let _ = CryptMsgClose(Some(msg));
    if r.is_err() {
        let _ = CertCloseStore(store, 0);
        return None;
    }
    let si = *(buf.as_ptr() as *const CMSG_SIGNER_INFO);
    let find = CERT_INFO {
        dwVersion: 0,
        SerialNumber: si.SerialNumber,
        SignatureAlgorithm: CRYPT_ALGORITHM_IDENTIFIER::default(),
        Issuer: si.Issuer,
        NotBefore: windows::Win32::Foundation::FILETIME::default(),
        NotAfter: windows::Win32::Foundation::FILETIME::default(),
        Subject: CRYPT_INTEGER_BLOB::default(),
        SubjectPublicKeyInfo: CERT_PUBLIC_KEY_INFO::default(),
        IssuerUniqueId: CRYPT_BIT_BLOB::default(),
        SubjectUniqueId: CRYPT_BIT_BLOB::default(),
        cExtension: 0,
        rgExtension: std::ptr::null_mut(),
    };
    let ctx = CertFindCertificateInStore(
        store,
        CERT_QUERY_ENCODING_TYPE((X509_ASN_ENCODING.0 | PKCS_7_ASN_ENCODING.0) as u32),
        0,
        CERT_FIND_SUBJECT_CERT,
        Some(&find as *const CERT_INFO as *const core::ffi::c_void),
        None,
    );
    if ctx.is_null() {
        let _ = CertCloseStore(store, 0);
        return None;
    }
    let mut name = vec![0u16; 256];
    let n = CertGetNameStringW(
        ctx,
        CERT_NAME_SIMPLE_DISPLAY_TYPE,
        0,
        None,
        Some(&mut name),
    );
    let _ = CertFreeCertificateContext(Some(ctx));
    let _ = CertCloseStore(store, 0);
    if n > 1 {
        Some(String::from_utf16_lossy(&name[..(n as usize - 1).min(name.len())]))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 事件日志查询（应急视角 5 类：登录/账户变更/服务安装/计划任务/日志清除）
// 内联 PowerShell 查询，全程内存完成，不产生任何临时文件
// ---------------------------------------------------------------------------
#[derive(Serialize, Deserialize, Clone)]
pub struct EventRow {
    pub time: String,
    pub id: u32,
    pub kind: String,
    pub result: String,
    pub user: String,
    pub ip: String,
    pub detail: String,
}

const EVENT_PS: &str = r#"[Console]::OutputEncoding=[System.Text.Encoding]::UTF8
$ids = @(4624,4625,4634,4647,4720,4722,4726,4724,4728,4732,4735,4698,1102,7045,104)
$lt = @{2='本地交互';3='网络';4='批处理';5='服务';7='解锁';8='网络明文';9='新凭据';10='远程交互(RDP)';11='缓存凭据';12='缓存远程交互';13='缓存解锁'}
$sub = @{'0xC0000064'='用户不存在';'0xC000006A'='密码错误';'0xC0000072'='账户已禁用';'0xC0000234'='账户已锁定';'0xC000006F'='超出登录时间';'0xC0000070'='工作站不受信任';'0xC000006D'='凭据错误'}
$start = (Get-Date).AddHours(-__HOURS__)
$evs = Get-WinEvent -FilterHashtable @{LogName='Security','System'; Id=$ids; StartTime=$start} -MaxEvents 2000 -ErrorAction SilentlyContinue
$out = @()
foreach ($e in @($evs)) {
  $x = [xml]$e.ToXml()
  $d = @{}
  if ($x.Event.EventData -and $x.Event.EventData.Data) { foreach ($dd in @($x.Event.EventData.Data)) { $d[$dd.Name] = [string]$dd.'#text' } }
  $id = $e.Id
  $kind = 'logon'; $user = ''; $result = '信息'; $detail = ''; $ip = ''
  switch ($id) {
    4624 { $kind='logon'; $user=[string]$d['TargetUserName']; $result='成功'; $ip=[string]$d['IpAddress']; $detail='登录类型：' + $lt[[int]$d['LogonType']] + '；工作站：' + $d['WorkstationName'] + '；认证：' + $d['AuthenticationPackageName'] }
    4625 { $kind='logon'; $user=[string]$d['TargetUserName']; $result='失败'; $ip=[string]$d['IpAddress']; $detail='登录类型：' + $lt[[int]$d['LogonType']] + '；工作站：' + $d['WorkstationName'] + '；原因：' + $sub[[string]$d['SubStatus']] }
    4634 { $kind='logon'; $user=[string]$d['TargetUserName']; $result='注销'; $detail='登录类型：' + $lt[[int]$d['LogonType']] }
    4647 { $kind='logon'; $user=[string]$d['TargetUserName']; $result='注销'; $detail='用户主动注销' }
    4720 { $kind='account'; $user=[string]$d['TargetUserName']; $result='高风险'; $detail='创建用户账户；操作者：' + $d['SubjectUserName'] }
    4722 { $kind='account'; $user=[string]$d['TargetUserName']; $result='变更'; $detail='启用用户账户；操作者：' + $d['SubjectUserName'] }
    4726 { $kind='account'; $user=[string]$d['TargetUserName']; $result='变更'; $detail='删除用户账户；操作者：' + $d['SubjectUserName'] }
    4724 { $kind='account'; $user=[string]$d['TargetUserName']; $result='变更'; $detail='重置账户密码；操作者：' + $d['SubjectUserName'] }
    4728 { $kind='account'; $user=[string]$d['TargetUserName']; $result='变更'; $detail='成员 ' + ($d['MemberName'] -replace '^CN=([^,]+).*','$1') + ' 加入组；操作者：' + $d['SubjectUserName'] }
    4732 { $kind='account'; $user=[string]$d['TargetUserName']; $result='高风险'; $detail='成员 ' + ($d['MemberName'] -replace '^CN=([^,]+).*','$1') + ' 加入本地组；操作者：' + $d['SubjectUserName'] }
    4735 { $kind='account'; $user=[string]$d['TargetUserName']; $result='变更'; $detail='本地组变更；操作者：' + $d['SubjectUserName'] }
    4698 { $kind='task'; $user=[string]$d['SubjectUserName']; $result='高风险'; $detail='创建计划任务：' + $d['TaskName'] }
    1102 { $kind='clearlog'; $user=[string]$d['SubjectUserName']; $result='高风险'; $detail='安全审计日志被清除（反取证信号）' }
    104  { $kind='clearlog'; $user=''; $result='高风险'; $detail='系统日志被清除（反取证信号）' }
    7045 { $kind='service'; $user=[string]$d['AccountName']; $result='高风险'; $detail='安装新服务：' + $d['ServiceName'] + '；路径：' + $d['ImagePath'] }
  }
  $out += [pscustomobject]@{ time=$e.TimeCreated.ToString('yyyy-MM-dd HH:mm:ss'); id=$id; kind=$kind; result=$result; user=$user; ip=$ip; detail=$detail }
}
ConvertTo-Json -InputObject @($out) -Compress
"#;

#[tauri::command]
pub async fn event_query(hours: u32) -> Result<Vec<EventRow>, String> {
    let script = EVENT_PS.replace("__HOURS__", &hours.clamp(1, 24 * 31).to_string());
    let mut rows: Vec<EventRow> = parse_json_array(&ps(&script)?)?;
    rows.sort_by(|a, b| b.time.cmp(&a.time));
    Ok(rows)
}

// ---------------------------------------------------------------------------
// 管理员权限检测（原生 TokenElevation）
// ---------------------------------------------------------------------------
#[tauri::command]
pub async fn is_elevated() -> Result<bool, String> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::Security::{
            GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
        };
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).map_err(|e| e.to_string())?;
        let mut elev = TOKEN_ELEVATION::default();
        let mut ret = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elev as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret,
        );
        let _ = CloseHandle(token);
        ok.map_err(|e| e.to_string())?;
        Ok(elev.TokenIsElevated != 0)
    }
    #[cfg(not(windows))]
    {
        Ok(false)
    }
}

// ---------------------------------------------------------------------------
// 资源管理器定位文件
// ---------------------------------------------------------------------------
#[tauri::command]
pub async fn open_folder(path: String) -> Result<(), String> {
    let p = std::path::PathBuf::from(&path);
    if !p.exists() {
        return Err(format!("路径不存在：{path}"));
    }
    #[cfg(windows)]
    {
        // explorer /select, 的参数含空格时，Rust 默认会给整段参数加引号，
        // 导致 explorer 解析出错打开错误目录 —— 用 raw_arg 精确控制引号：
        // 正确形式为 /select,"C:\Program Files\xxx\app.exe"
        use std::os::windows::process::CommandExt;
        let mut cmd = Command::new("explorer.exe");
        if p.is_dir() {
            cmd.raw_arg(format!("\"{}\"", p.to_string_lossy()));
        } else {
            cmd.raw_arg(format!("/select,\"{}\"", p.to_string_lossy()));
        }
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd.spawn().map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = p;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 文件占用解除（Restart Manager 检测占用进程 + 强制结束 + 递归删除）
// ---------------------------------------------------------------------------
#[derive(Serialize, Clone)]
pub struct LockEntry {
    pub pid: u32,
    pub name: String,
    pub exe: String,
}

#[derive(Serialize, Clone)]
pub struct ScanReport {
    pub locks: Vec<LockEntry>,
    pub file_count: usize,
    pub truncated: bool,
}

#[derive(Serialize, Clone)]
pub struct DeleteFail {
    pub path: String,
    pub error: String,
}

#[derive(Serialize, Clone)]
pub struct DeleteReport {
    pub killed: Vec<String>,
    pub deleted_files: u32,
    pub failed: Vec<DeleteFail>,
}

const EXPAND_CAP: usize = 4000;

/// 递归展开目录为文件列表（不跟随符号链接/挂载点，防止环路与误删）
fn expand_paths(paths: &[String], files: &mut Vec<String>, truncated: &mut bool) {
    for p in paths {
        expand_one(std::path::Path::new(p), files, truncated);
    }
}

fn expand_one(p: &std::path::Path, files: &mut Vec<String>, truncated: &mut bool) {
    if files.len() >= EXPAND_CAP {
        *truncated = true;
        return;
    }
    let Ok(meta) = std::fs::symlink_metadata(p) else {
        return; // 不存在的路径（已被删）直接跳过
    };
    let is_dir = meta.is_dir() && !meta.file_type().is_symlink();
    if !is_dir {
        files.push(p.to_string_lossy().to_string());
        return;
    }
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            expand_one(&e.path(), files, truncated);
            if files.len() >= EXPAND_CAP {
                *truncated = true;
                return;
            }
        }
    }
}

/// 长路径安全化（\\?\ 前缀），供 Windows API 删除使用
fn wide_long(p: &std::path::Path) -> Vec<u16> {
    let s = p.to_string_lossy();
    let s = if s.starts_with(r"\\?\") {
        s.to_string()
    } else if s.starts_with(r"\\") {
        format!(r"\\?\UNC\{}", &s[2..])
    } else {
        format!(r"\\?\{s}")
    };
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 用 Restart Manager 查询占用这些文件的进程
#[cfg(windows)]
fn rm_query_locks(files: &[String]) -> Result<Vec<(u32, String)>, String> {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::Foundation::ERROR_MORE_DATA;
    use windows::Win32::System::RestartManager::{
        RmEndSession, RmGetList, RmRegisterResources, RmStartSession, RM_PROCESS_INFO,
    };

    if files.is_empty() {
        return Ok(vec![]);
    }
    let wide: Vec<Vec<u16>> = files
        .iter()
        .map(|f| f.encode_utf16().chain(std::iter::once(0)).collect())
        .collect();
    let ptrs: Vec<PCWSTR> = wide.iter().map(|w| PCWSTR(w.as_ptr())).collect();

    unsafe {
        let mut session = 0u32;
        let mut key = [0u16; 33]; // CCH_RM_SESSION_KEY(32) + 1
        let err = RmStartSession(&mut session, 0, PWSTR(key.as_mut_ptr()));
        if err.0 != 0 {
            return Err(format!("RmStartSession 失败（错误码 {}）", err.0));
        }
        let result = (|| -> Result<Vec<(u32, String)>, String> {
            let err = RmRegisterResources(session, Some(&ptrs), None, None);
            if err.0 != 0 {
                return Err(format!("RmRegisterResources 失败（错误码 {}）", err.0));
            }
            let mut needed = 0u32;
            let mut count = 0u32;
            let mut reasons = 0u32;
            let err = RmGetList(session, &mut needed, &mut count, None, &mut reasons);
            if err.0 != 0 && err.0 != ERROR_MORE_DATA.0 {
                return Err(format!("RmGetList 失败（错误码 {}）", err.0));
            }
            if needed == 0 {
                return Ok(vec![]);
            }
            let mut infos: Vec<RM_PROCESS_INFO> = vec![unsafe { std::mem::zeroed() }; needed as usize];
            count = needed;
            let err = RmGetList(session, &mut needed, &mut count, Some(infos.as_mut_ptr()), &mut reasons);
            if err.0 != 0 {
                return Err(format!("RmGetList 失败（错误码 {}）", err.0));
            }
            infos.truncate(count as usize);
            Ok(infos
                .into_iter()
                .map(|p| {
                    let name_end = p.strAppName.iter().position(|&c| c == 0).unwrap_or(p.strAppName.len());
                    (p.Process.dwProcessId, String::from_utf16_lossy(&p.strAppName[..name_end]))
                })
                .collect())
        })();
        let _ = RmEndSession(session);
        result
    }
}

#[cfg(not(windows))]
fn rm_query_locks(_files: &[String]) -> Result<Vec<(u32, String)>, String> {
    Ok(vec![])
}

/// 温和关闭占用进程（RmShutdown ≈ 用户点窗口 ×，应用可保存文档后退出）——
/// 仅对不响应温和关闭的幸存者才走 TerminateProcess。既是更好的用户体验，
/// 也让行为链远离"强杀+删除"的勒索软件模式（降低杀软行为误报）
#[cfg(windows)]
fn rm_shutdown_graceful(files: &[String]) -> Result<Vec<(u32, String)>, String> {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::System::RestartManager::{
        RmEndSession, RmRegisterResources, RmShutdown, RmStartSession,
    };
    if files.is_empty() {
        return Ok(vec![]);
    }
    let wide: Vec<Vec<u16>> = files
        .iter()
        .map(|f| f.encode_utf16().chain(std::iter::once(0)).collect())
        .collect();
    let ptrs: Vec<PCWSTR> = wide.iter().map(|w| PCWSTR(w.as_ptr())).collect();
    unsafe {
        let mut session = 0u32;
        let mut key = [0u16; 33];
        let err = RmStartSession(&mut session, 0, PWSTR(key.as_mut_ptr()));
        if err.0 != 0 {
            return Err(format!("RmStartSession 失败（错误码 {}）", err.0));
        }
        let result = (|| -> Result<(), String> {
            let err = RmRegisterResources(session, Some(&ptrs), None, None);
            if err.0 != 0 {
                return Err(format!("RmRegisterResources 失败（错误码 {}）", err.0));
            }
            // 0 = 温和关闭（WM_CLOSE）；不传强制标志
            let err = RmShutdown(session, 0, None);
            if err.0 != 0 {
                return Err(format!("RmShutdown 失败（错误码 {}）", err.0));
            }
            Ok(())
        })();
        let _ = RmEndSession(session);
        result?;
    }
    std::thread::sleep(std::time::Duration::from_millis(500)); // 给应用一点保存/退出的时间
    rm_query_locks(files) // 幸存者 = 仍占用的进程
}

#[cfg(not(windows))]
fn rm_shutdown_graceful(_files: &[String]) -> Result<Vec<(u32, String)>, String> {
    Ok(vec![])
}

/// 进程信息来自 sysinfo 快照（补充真实 exe 路径）
pub(crate) fn proc_snapshot() -> std::collections::HashMap<u32, (String, String)> {
    let Ok(mut sys) = SYS.lock() else { return std::collections::HashMap::new() };
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
    sys.processes()
        .values()
        .map(|p| {
            (
                p.pid().as_u32(),
                (
                    p.name().to_string_lossy().to_string(),
                    p.exe().map(|e| e.display().to_string()).unwrap_or_default(),
                ),
            )
        })
        .collect()
}

#[tauri::command]
pub async fn unlock_scan(paths: Vec<String>) -> Result<ScanReport, String> {
    if paths.is_empty() {
        return Err("请先添加要检测的文件或文件夹".into());
    }
    let mut files = Vec::new();
    let mut truncated = false;
    expand_paths(&paths, &mut files, &mut truncated);
    if files.is_empty() {
        return Ok(ScanReport { locks: vec![], file_count: 0, truncated });
    }

    // 单次 Restart Manager 会话查询全部文件的占用进程（快）
    let raw = rm_query_locks(&files)?;
    let snap = proc_snapshot();
    let me = std::process::id();

    let mut seen = std::collections::HashSet::new();
    let locks = raw
        .into_iter()
        .filter(|(pid, _)| *pid != me && seen.insert(*pid))
        .map(|(pid, app_name)| {
            let (name, exe) = snap.get(&pid).cloned().unwrap_or_else(|| (app_name, String::new()));
            LockEntry { pid, name, exe }
        })
        .collect();

    Ok(ScanReport { locks, file_count: files.len(), truncated })
}

#[tauri::command]
pub async fn force_delete(paths: Vec<String>) -> Result<DeleteReport, String> {
    if paths.is_empty() {
        return Err("请先添加要删除的文件或文件夹".into());
    }
    let mut files = Vec::new();
    let mut truncated = false;
    expand_paths(&paths, &mut files, &mut truncated);

    // 1) 查询并结束占用进程
    let mut killed = Vec::new();
    let raw = rm_query_locks(&files)?;
    let snap = proc_snapshot();
    let me = std::process::id();
    let mut pids: Vec<u32> = raw
        .into_iter()
        .map(|(pid, _)| pid)
        .filter(|pid| *pid != me && *pid >= 10)
        .collect();
    pids.sort_unstable();
    pids.dedup();
    #[cfg(windows)]
    {
        // 温和关闭优先（应用可保存文档），仅对不退出的幸存者强制结束
        let survivors: Vec<u32> =
            rm_shutdown_graceful(&files)?.into_iter().map(|(pid, _)| pid).collect();
        for pid in &pids {
            let graceful = !survivors.contains(pid);
            let ok = graceful || unsafe { terminate_pid(*pid) };
            let (name, _) = snap.get(pid).cloned().unwrap_or_default();
            killed.push(format!(
                "PID {pid} {name} {}",
                if graceful {
                    "（已温和关闭，应用可保存文档）"
                } else if ok {
                    "（不响应关闭，已强制结束）"
                } else {
                    "（结束失败，将尝试直接删除）"
                }
            ));
        }
        if !survivors.is_empty() {
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    }
    #[cfg(not(windows))]
    let _ = (&pids, &snap, &mut killed);

    // 2) 递归删除（带重试）
    let mut report = DeleteReport { killed, deleted_files: 0, failed: vec![] };
    for root in &paths {
        let p = std::path::PathBuf::from(root);
        let mut last_err = String::new();
        let mut ok = false;
        for _ in 0..3 {
            last_err = String::new();
            ok = delete_recursive(&p, &mut report.deleted_files, &mut last_err);
            if ok {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        if !ok {
            report.failed.push(DeleteFail {
                path: root.clone(),
                error: if last_err.is_empty() { "删除失败".into() } else { last_err },
            });
        }
    }
    Ok(report)
}

/// 递归删除：先清属性再删；返回是否整体成功
fn delete_recursive(p: &std::path::Path, deleted: &mut u32, err_out: &mut String) -> bool {
    let mut ok = true;
    if let Ok(meta) = std::fs::symlink_metadata(p) {
        if meta.is_dir() && !meta.file_type().is_symlink() {
            if let Ok(rd) = std::fs::read_dir(p) {
                for e in rd.flatten() {
                    if !delete_recursive(&e.path(), deleted, err_out) {
                        ok = false;
                    }
                }
            }
        }
    }
    #[cfg(windows)]
    unsafe {
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{
            SetFileAttributesW, FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES,
        };
        let w = wide_long(p);
        let _ = SetFileAttributesW(PCWSTR(w.as_ptr()), FILE_FLAGS_AND_ATTRIBUTES(FILE_ATTRIBUTE_NORMAL.0));
    }
    let is_dir = p.is_dir();
    let res = if is_dir { std::fs::remove_dir(p) } else { std::fs::remove_file(p) };
    match res {
        Ok(()) => {
            if !is_dir {
                *deleted += 1;
            }
            true
        }
        Err(_) => {
            // 高阶回退：备份语义句柄 + POSIX 置位删除
            // （SeBackup/SeRestore 特权下 FILE_FLAG_BACKUP_SEMANTICS 可绕过常规 ACL 检查，
            //   FileDispositionInfoEx = POSIX 删除，关闭句柄即生效，对付拒绝访问/占用文件）
            if backup_semantics_delete(p, is_dir) {
                if !is_dir {
                    *deleted += 1;
                }
                true
            } else {
                if err_out.is_empty() {
                    *err_out = "拒绝访问（已尝试提权与备份语义删除）".to_string();
                }
                false
            }
        }
    }
}

/// 备份语义强制删除：SeBackup/SeRestore + FILE_FLAG_BACKUP_SEMANTICS + POSIX 删除
#[cfg(windows)]
fn backup_semantics_delete(p: &std::path::Path, is_dir: bool) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, SetFileInformationByHandle, FILE_DISPOSITION_INFO_EX, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_SHARE_MODE, FileDispositionInfoEx, OPEN_EXISTING, DELETE,
    };
    let _ = is_dir;
    let w = wide_long(p);
    unsafe {
        let h = match CreateFileW(
            PCWSTR(w.as_ptr()),
            DELETE.0,
            FILE_SHARE_MODE(0),
            None,
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            HANDLE::default(),
        ) {
            Ok(h) => h,
            Err(_) => return false,
        };
        // POSIX 语义：置位后关闭句柄即删除（目录需为空，递归已先行处理）
        let info = FILE_DISPOSITION_INFO_EX {
            Flags: windows::Win32::Storage::FileSystem::FILE_DISPOSITION_INFO_EX_FLAGS(1),
        };
        let r = SetFileInformationByHandle(
            h,
            FileDispositionInfoEx,
            &info as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
        );
        let _ = windows::Win32::Foundation::CloseHandle(h);
        r.is_ok()
    }
}

#[cfg(not(windows))]
fn backup_semantics_delete(_p: &std::path::Path, _is_dir: bool) -> bool {
    false
}

// ---------------------------------------------------------------------------
// 文件 Hash（威胁情报查询用）与外部浏览器打开
// ---------------------------------------------------------------------------
#[tauri::command]
pub async fn hash_file(path: String) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut f = std::fs::File::open(&path).map_err(|e| format!("无法打开文件：{e}"))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[tauri::command]
pub async fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("仅允许 https 链接".to_string());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        Command::new("cmd")
            .args(["/C", "start", "", &url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
