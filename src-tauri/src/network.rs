//! 网络监控：TCP 连接表（GetExtendedTcpTable）+ 离线 IP 归属地（ip2region.xdb 自动识别）
//!
//! - 连接表走 iphlpapi 原生 API（netstat 同款），毫秒级返回，无 PowerShell 开销
//! - IP 库自动识别：程序目录 / %APPDATA%\TexMantis / %LOCALAPPDATA%\TexMantis /
//!   C:\ProgramData\TexMantis / 项目目录 下的 ip2region.xdb；也可手动指定路径
//! - xdb 全量载入内存（约 11MB），查询 O(log n)，线程安全（&[u8] 只读共享）

use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Serialize, Clone)]
pub struct NetConn {
    pub local: String,
    pub remote: String,
    pub state: String,
    pub pid: u32,
    pub proc_name: String,
}

#[derive(Serialize, Clone)]
pub struct GeoStatus {
    pub found: bool,
    pub kind: String, // "ip2region" | "none"
    pub path: String,
    pub msg: String,
}

// ---------------------------------------------------------------------------
// TCP 连接表
// ---------------------------------------------------------------------------
#[cfg(windows)]
pub fn tcp_connections() -> Result<Vec<NetConn>, String> {
    use windows::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCPTABLE_OWNER_MODULE, TCP_TABLE_OWNER_MODULE_ALL,
    };

    const AF_INET: u32 = 2;
    const AF_INET6: u32 = 23;

    let names = crate::commands::proc_snapshot();
    let mut out = Vec::new();

    unsafe {
        for (af, _is_v6) in [(AF_INET, false), (AF_INET6, true)] {
            let mut size: u32 = 16 * 1024;
            loop {
                let mut buf = vec![0u8; size as usize];
                let err = GetExtendedTcpTable(
                    Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
                    &mut size,
                    windows::Win32::Foundation::FALSE, // 不排序
                    af,
                    TCP_TABLE_OWNER_MODULE_ALL,
                    0,
                );
                if err == 0 {
                    if af == AF_INET {
                        let t = &*(buf.as_ptr() as *const MIB_TCPTABLE_OWNER_MODULE);
                        let rows =
                            std::slice::from_raw_parts(t.table.as_ptr(), t.dwNumEntries as usize);
                        for r in rows {
                            out.push(row_to_conn(
                                r.dwState,
                                r.dwLocalAddr,
                                r.dwLocalPort,
                                r.dwRemoteAddr,
                                r.dwRemotePort,
                                r.dwOwningPid,
                                &names,
                            ));
                        }
                    } else {
                        // v6：MIB_TCP6TABLE_OWNER_MODULE 布局一致（前 4 字节为条目数，
                        // 行结构里地址为 [u8;16]），手动按内存布局解析
                        let t = &*(buf.as_ptr() as *const MIB_TCP6TABLE_OWNER_MODULE_REPR);
                        let rows = std::slice::from_raw_parts(
                            t.rows.as_ptr(),
                            (t.num_entries as usize).min(65536),
                        );
                        for row in rows {
                            out.push(NetConn {
                                local: format!(
                                    "[{}]:{}",
                                    ipv6_str(&row.local_addr),
                                    swap_port(row.local_port)
                                ),
                                remote: format!(
                                    "[{}]:{}",
                                    ipv6_str(&row.remote_addr),
                                    swap_port(row.remote_port)
                                ),
                                state: state_name(row.state),
                                pid: row.pid,
                                proc_name: names
                                    .get(&row.pid)
                                    .map(|(n, _)| n.clone())
                                    .unwrap_or_else(|| "System".to_string()),
                            });
                        }
                    }
                    break;
                }
                // ERROR_INSUFFICIENT_BUFFER (122)：按返回的所需大小重试
                if err == 122 && size > 64 * 1024 * 1024 {
                    break;
                }
                if err != 122 {
                    return Err(format!("GetExtendedTcpTable 失败（错误码 {err}）"));
                }
            }
        }
    }
    Ok(out)
}

/// 手动镜像 MIB_TCP6TABLE_OWNER_MODULE 的内存布局（避免依赖不稳定绑定）
#[cfg(windows)]
#[repr(C)]
struct MIB_TCP6TABLE_OWNER_MODULE_REPR {
    num_entries: u32,
    rows: [MIB_TCP6ROW_OWNER_MODULE_REPR; 1],
}
#[cfg(windows)]
#[repr(C)]
struct MIB_TCP6ROW_OWNER_MODULE_REPR {
    state: u32,          // MIB_TCP_STATE
    local_addr: [u8; 16],
    local_scope_id: u32,
    local_port: u32,
    remote_addr: [u8; 16],
    remote_scope_id: u32,
    remote_port: u32,
    pid: u32,
    _create_ts: i64,
    _module_info: [u64; 16],
}

#[cfg(windows)]
/// 端口：DWORD 低 16 位为网络字节序（netstat 同款换算）
fn swap_port(p: u32) -> u16 {
    u16::from_be((p & 0xFFFF) as u16)
}

#[cfg(windows)]
fn ipv6_str(b: &[u8; 16]) -> String {
    // IPv4 映射地址直接按 v4 展示
    if b[..12] == [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff] {
        return format!("{}.{}.{}.{}", b[12], b[13], b[14], b[15]);
    }
    let segs: Vec<String> = (0..8)
        .map(|i| format!("{:x}", u16::from_be_bytes([b[i * 2], b[i * 2 + 1]])))
        .collect();
    segs.join(":")
}

#[cfg(windows)]
fn row_to_conn(
    state: u32,
    laddr: u32,
    lport: u32,
    raddr: u32,
    rport: u32,
    pid: u32,
    names: &HashMap<u32, (String, String)>,
) -> NetConn {
    let la = std::net::Ipv4Addr::from(laddr.to_be());
    let ra = std::net::Ipv4Addr::from(raddr.to_be());
    let name = names
        .get(&pid)
        .map(|(n, _)| n.clone())
        .unwrap_or_else(|| if pid == 0 { "System".into() } else { format!("PID {pid}") });
    NetConn {
        local: format!("{la}:{}", swap_port(lport)),
        remote: format!("{ra}:{}", swap_port(rport)),
        state: state_name(state),
        pid,
        proc_name: name,
    }
}

#[cfg(windows)]
fn state_name(s: u32) -> String {
    let n = match s {
        2 => "LISTEN",
        3 => "SYN_SENT",
        4 => "SYN_RCVD",
        5 => "ESTABLISHED",
        6 => "FIN_WAIT1",
        7 => "FIN_WAIT2",
        8 => "CLOSE_WAIT",
        9 => "CLOSING",
        10 => "LAST_ACK",
        11 => "TIME_WAIT",
        12 => "DELETE_TCB",
        _ => "CLOSED",
    };
    n.to_string()
}

#[cfg(not(windows))]
pub fn tcp_connections() -> Result<Vec<NetConn>, String> {
    Ok(vec![])
}

// ---------------------------------------------------------------------------
// ip2region.xdb 离线 IP 库（自动识别 + 全量内存缓存）
// ---------------------------------------------------------------------------
static XDB: Mutex<Option<std::sync::Arc<Vec<u8>>>> = Mutex::new(None);
static XDB_PATH: Mutex<String> = Mutex::new(String::new());

fn candidate_paths(custom: &str) -> Vec<String> {
    let mut v = Vec::new();
    if !custom.trim().is_empty() {
        v.push(custom.trim().to_string());
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(d) = exe.parent() {
            v.push(d.join("ip2region.xdb").display().to_string());
        }
        v.push(exe.parent().unwrap().display().to_string());
    }
    if let Ok(d) = std::env::current_dir() {
        v.push(d.join("ip2region.xdb").display().to_string());
    }
    for var in ["APPDATA", "LOCALAPPDATA", "ProgramData"] {
        if let Ok(base) = std::env::var(var) {
            v.push(
                std::path::Path::new(&base)
                    .join("TexMantis")
                    .join("ip2region.xdb")
                    .display()
                    .to_string(),
            );
        }
    }
    v
}

pub fn geo_status(custom: &str) -> GeoStatus {
    let loaded = XDB.lock().unwrap().is_some();
    let path = XDB_PATH.lock().unwrap().clone();
    if loaded {
        return GeoStatus { found: true, kind: "ip2region".into(), path: path.clone(), msg: format!("IP 库已加载：{path}") };
    }
    // 尝试加载
    for p in candidate_paths(custom) {
        let path = std::path::Path::new(&p);
        if path.is_file() {
            match std::fs::read(path) {
                Ok(data) => {
                    // 头部 256 字节，offset 0 = 版本号（LE u16），v2/v3 格式
                    let ver = u16::from_le_bytes([*data.first().unwrap_or(&0), *data.get(1).unwrap_or(&0)]);
                    if data.len() > 300 && (ver == 2 || ver == 3) {
                        *XDB.lock().unwrap() = Some(std::sync::Arc::new(data));
                        *XDB_PATH.lock().unwrap() = p.clone();
                        return GeoStatus {
                            found: true,
                            kind: "ip2region".into(),
                            path: p.clone(),
                            msg: format!("IP 库已加载：{p}"),
                        };
                    }
                    return GeoStatus {
                        found: false,
                        kind: "none".into(),
                        path: p,
                        msg: "文件存在但不是有效的 ip2region.xdb".into(),
                    };
                }
                Err(e) => {
                    return GeoStatus {
                        found: false,
                        kind: "none".into(),
                        path: p,
                        msg: format!("读取失败：{e}"),
                    }
                }
            }
        }
    }
    GeoStatus {
        found: false,
        kind: "none".into(),
        path: String::new(),
        msg: "未找到 ip2region.xdb —— 请将其放到 TexMantis.exe 同目录、%APPDATA%\\TexMantis 或 %LOCALAPPDATA%\\TexMantis，或在下方填写完整路径".into(),
    }
}

pub fn geo_set_path(p: &str) -> GeoStatus {
    *XDB.lock().unwrap() = None;
    *XDB_PATH.lock().unwrap() = String::new();
    if p.trim().is_empty() {
        return geo_status("");
    }
    geo_status(p)
}

/// ip2region v2 查询：返回 "国家|区域|省|市|ISP"
fn xdb_search(buf: &[u8], ip: std::net::Ipv4Addr) -> Option<String> {
    let o = ip.octets();
    let ip_u32 = u32::from_be_bytes(o);
    // header: 0..256；vector index 起始 256，slot = 前两字节
    let slot = (o[0] as usize) * 256 + (o[1] as usize);
    let vp = 256 + slot * 8;
    if vp + 8 > buf.len() {
        return None;
    }
    let s_ptr = u32::from_le_bytes([buf[vp], buf[vp + 1], buf[vp + 2], buf[vp + 3]]) as usize;
    let e_ptr = u32::from_le_bytes([buf[vp + 4], buf[vp + 5], buf[vp + 6], buf[vp + 7]]) as usize;
    if e_ptr < s_ptr || e_ptr + 14 > buf.len() {
        return None;
    }
    // index 区二分：每条 14 字节 = sip(4) eip(4) len(2) data_ptr(4)
    let mut lo = 0usize;
    let mut hi = (e_ptr - s_ptr) / 14;
    while lo <= hi {
        let mid = (lo + hi) / 2;
        let base = s_ptr + mid * 14;
        if base + 14 > buf.len() {
            return None;
        }
        let sip = u32::from_le_bytes([buf[base], buf[base + 1], buf[base + 2], buf[base + 3]]);
        let eip = u32::from_le_bytes([buf[base + 4], buf[base + 5], buf[base + 6], buf[base + 7]]);
        if ip_u32 < sip {
            if mid == 0 { return None; }
            hi = mid - 1;
        } else if ip_u32 > eip {
            lo = mid + 1;
        } else {
            let dlen = u16::from_le_bytes([buf[base + 8], buf[base + 9]]) as usize;
            let dp = u32::from_le_bytes([buf[base + 10], buf[base + 11], buf[base + 12], buf[base + 13]]) as usize;
            // data 区：index 条目内的 data_len 即内容长度，data_ptr 直接指向内容
            if dlen == 0 || dp + dlen > buf.len() {
                return None;
            }
            return Some(String::from_utf8_lossy(&buf[dp..dp + dlen]).to_string());
        }
    }
    None
}

/// 批量归属地查询；未命中或无库返回 ""
pub fn geo_lookup(ips: Vec<String>) -> HashMap<String, String> {
    let mut res = HashMap::new();
    let guard = XDB.lock().unwrap();
    let Some(buf) = guard.as_ref() else { return res };
    for ip in ips {
        if res.contains_key(&ip) {
            continue;
        }
        let out = match ip.parse::<std::net::Ipv4Addr>() {
            Ok(v) => xdb_search(buf, v)
                .map(|s| s.split('|').filter(|x| !x.is_empty() && *x != "0").collect::<Vec<_>>().join(" "))
                .unwrap_or_default(),
            Err(_) => String::new(),
        };
        res.insert(ip, out);
    }
    res
}

// ---------------------------------------------------------------------------
// Tauri 命令
// ---------------------------------------------------------------------------
#[tauri::command]
pub async fn net_connections() -> Result<Vec<NetConn>, String> {
    #[cfg(windows)]
    {
        tcp_connections()
    }
    #[cfg(not(windows))]
    {
        Ok(vec![])
    }
}

#[tauri::command]
pub async fn geo_get_status() -> Result<GeoStatus, String> {
    Ok(geo_status(""))
}

#[tauri::command]
pub async fn geo_set_db_path(path: String) -> Result<GeoStatus, String> {
    Ok(geo_set_path(&path))
}

#[tauri::command]
pub async fn geo_lookup_ips(ips: Vec<String>) -> Result<HashMap<String, String>, String> {
    Ok(geo_lookup(ips))
}
