// 预警链路诊断日志（debug 级明细）：落盘 <工具目录>/diag.log，超 1MB 轮转为 diag.log.1
// 设计约束：诊断设施绝不影响业务（写入失败静默）；绿色便携无残留（与工具同目录）
use std::io::Write;
use std::sync::Mutex;

static DIAG_IO: Mutex<()> = Mutex::new(());

fn path() -> Option<std::path::PathBuf> {
    Some(crate::settings::logs_dir().join("diag.log"))
}

/// 关闭诊断日志时删除已有文件（diag.log + 轮转备份）
pub(crate) fn remove_files() {
    if let Some(p) = path() {
        let _ = std::fs::remove_file(&p);
        let _ = std::fs::remove_file(p.with_extension("log.1"));
    }
}

/// 追加一行诊断日志（线程安全）
pub(crate) fn write(level: &str, msg: &str) {
    // 门控：未开启诊断日志时完全不产生文件（先读开关再拿 IO 锁，避免锁序交叉）
    if !crate::settings::diag_log_enabled() {
        return;
    }
    let Ok(_g) = DIAG_IO.lock() else { return };
    let Some(p) = path() else { return };
    if let Ok(md) = std::fs::metadata(&p) {
        if md.len() > 1_000_000 {
            let _ = std::fs::rename(&p, p.with_extension("log.1"));
        }
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let (y, mo, d, h, mi, s, ms) = civil_utc(now);
    let line = format!("[{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}.{ms:03}][{level}] {msg}\n");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let _ = f.write_all(line.as_bytes());
    }
}

/// epoch ms → (y, m, d, h, min, s, ms)（UTC，Howard Hinnant civil_from_days 算法）
fn civil_utc(ms: u64) -> (i64, u32, u32, u32, u32, u32, u32) {
    let secs = ms / 1000;
    let days = (secs / 86400) as i64;
    let rem = (secs % 86400) as u32;
    let h = rem / 3600;
    let mi = (rem % 3600) / 60;
    let s = rem % 60;
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d, h, mi, s, (ms % 1000) as u32)
}

/// 供 UI 读取：合并 diag.log.1 与 diag.log，返回尾部最多 max 字节（对齐行首）
pub(crate) fn tail(max: usize) -> String {
    let Some(p) = path() else { return String::new() };
    let mut buf: Vec<u8> = Vec::new();
    if let Ok(b) = std::fs::read(p.with_extension("log.1")) {
        buf.extend_from_slice(&b);
    }
    if let Ok(b) = std::fs::read(&p) {
        buf.extend_from_slice(&b);
    }
    if buf.len() > max {
        let start = buf.len() - max;
        let mut i = start;
        while i < buf.len() && buf[i] != b'\n' {
            i += 1;
        }
        if i <= buf.len() {
            buf.drain(..i);
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// 清空诊断日志
pub(crate) fn clear() {
    let Ok(_g) = DIAG_IO.lock() else { return };
    let Some(p) = path() else { return };
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_file(p.with_extension("log.1"));
}
