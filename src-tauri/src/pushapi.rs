// ---------------------------------------------------------------------------
// v0.9.23 开放消息推送接口：脚本/自动化的 DIY 数据监控 → 统一气泡消息
// ---------------------------------------------------------------------------
// 监听 127.0.0.1:25110（仅回环，外部不可达）。
//   GET  /push?key=密钥&title=标题&body=正文   （参数需 URL 编码）
//   POST /push   JSON {"key":"..","title":"..","body":".."}
// 鉴权：?key= / X-API-Key 头 / Authorization: Bearer .. 任一匹配设置里生成的 10 位密钥。
// 密钥为空 = 接口关闭。最近 10 条推送历史持久化，设置页可查看。
// ---------------------------------------------------------------------------

use std::io::{Read, Write};
use std::sync::LazyLock;

pub const API_PORT: u16 = 25110;
const API_HOST: &str = "127.0.0.1";
const MAX_HEAD: usize = 8 * 1024;
const MAX_BODY: usize = 8 * 1024;
const TITLE_MAX: usize = 80; // 字符数
const BODY_MAX: usize = 600; // 字符数
const HISTORY_CAP: usize = 10;

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ApiPush {
    pub ts: u64,
    pub title: String,
    pub body: String,
}

static API_PUSHES: LazyLock<std::sync::Mutex<Vec<ApiPush>>> =
    LazyLock::new(|| std::sync::Mutex::new(Vec::new()));
// 气泡弹窗节流：防脚本风暴刷屏（被节流的推送仍入历史）
static LAST_POPUP: LazyLock<std::sync::Mutex<std::time::Instant>> =
    LazyLock::new(|| std::sync::Mutex::new(std::time::Instant::now() - std::time::Duration::from_secs(3600)));

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn history_path() -> std::path::PathBuf {
    crate::settings::social_dir().join("api_pushes.json")
}

fn load_history() {
    if let Ok(txt) = std::fs::read_to_string(history_path()) {
        if let Ok(v) = serde_json::from_str::<Vec<ApiPush>>(&txt) {
            *API_PUSHES.lock().unwrap_or_else(|e| e.into_inner()) = v;
        }
    }
}

fn save_history() {
    let g = API_PUSHES.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(txt) = serde_json::to_string(&*g) {
        drop(g);
        let _ = std::fs::write(history_path(), txt);
    }
}

/// 可读密钥字符集：剔除 0/O/1/I/l 等易混字符
const KEY_CHARSET: &[u8] = b"abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789";

fn gen_key_str() -> String {
    let mut buf = [0u8; 10];
    if getrandom::getrandom(&mut buf).is_err() {
        // 随机源失败：时间熵兜底（正常路径不会走到）
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
        for (i, b) in buf.iter_mut().enumerate() {
            *b = ((t >> (i * 7)) & 0xff) as u8;
        }
    }
    buf.iter().map(|b| KEY_CHARSET[(b % KEY_CHARSET.len() as u8) as usize] as char).collect()
}

fn read_config_key() -> String {
    crate::settings::config().api_key.clone() // v0.9.23 config() 返回守卫，字段须克隆出借
}

/// 推送一条：入历史 + 气泡提醒（300ms 节流）
fn push_notify(title: String, body: String) {
    let item = ApiPush { ts: now_ms(), title: title.clone(), body };
    {
        let mut g = API_PUSHES.lock().unwrap_or_else(|e| e.into_inner());
        g.insert(0, item);
        if g.len() > HISTORY_CAP {
            g.truncate(HISTORY_CAP);
        }
    }
    save_history();
    let allow_popup = {
        let mut last = LAST_POPUP.lock().unwrap_or_else(|e| e.into_inner());
        let ok = last.elapsed() >= std::time::Duration::from_millis(300);
        if ok {
            *last = std::time::Instant::now();
        }
        ok
    };
    if allow_popup {
        if let Some(app) = crate::settings::APP.get() {
            crate::monitor::notify_api(app, &title, "");
            crate::social::sound_notify();
        }
    }
}

// ---------------------------- HTTP 服务 ----------------------------

pub fn start_server() {
    load_history();
    std::thread::Builder::new().name("pushapi".into()).spawn(move || {
        let listener = match std::net::TcpListener::bind((API_HOST, API_PORT)) {
            Ok(l) => l,
            Err(e) => {
                crate::social::socdbg(&format!("pushapi: 监听 {API_HOST}:{API_PORT} 失败: {e}"));
                return;
            }
        };
        crate::social::socdbg(&format!("pushapi: 开放推送接口就绪 {API_HOST}:{API_PORT}"));
        for conn in listener.incoming() {
            match conn {
                Ok(s) => {
                    std::thread::spawn(move || handle_conn(s));
                }
                Err(_) => continue,
            }
        }
    });
}

struct Req {
    method: String,
    path: String,
    query: Vec<(String, String)>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn read_request(s: &mut std::net::TcpStream) -> Result<Req, String> {
    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(3)));
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    // 读到头结束
    let head_end = loop {
        if buf.len() > MAX_HEAD {
            return Err("请求头过大".into());
        }
        let n = s.read(&mut chunk).map_err(|e| format!("读取失败: {e}"))?;
        if n == 0 {
            return Err("连接中断".into());
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = find_head_end(&buf) {
            break i;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let reqline = lines.next().unwrap_or("");
    let mut parts = reqline.split_whitespace();
    let method = parts.next().unwrap_or("").to_uppercase();
    let target = parts.next().unwrap_or("/").to_string();
    let mut headers = Vec::new();
    for l in lines {
        if let Some(i) = l.find(':') {
            headers.push((l[..i].trim().to_lowercase(), l[i + 1..].trim().to_string()));
        }
    }
    let content_len = headers.iter().find(|(k, _)| k == "content-length").and_then(|(_, v)| v.parse::<usize>().ok()).unwrap_or(0);
    if content_len > MAX_BODY {
        return Err("请求体过大".into());
    }
    let mut body: Vec<u8> = buf[head_end + 4..].to_vec();
    while body.len() < content_len {
        let n = s.read(&mut chunk).map_err(|e| format!("读取失败: {e}"))?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_len);
    let (path, query) = match target.split_once('?') {
        Some((p, q)) => (p.to_string(), parse_query(q)),
        None => (target.clone(), Vec::new()),
    };
    Ok(Req { method, path, query, headers, body })
}

fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn parse_query(q: &str) -> Vec<(String, String)> {
    q.split('&')
        .filter_map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            Some((url_decode(k), url_decode(v)))
        })
        .collect()
}

/// 百分号解码（+ 视作空格）
fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < b.len() => {
                let h = (b[i + 1] as char).to_digit(16);
                let l = (b[i + 2] as char).to_digit(16);
                match (h, l) {
                    (Some(h), Some(l)) => {
                        out.push(((h << 4) | l) as u8);
                        i += 3;
                    }
                    _ => {
                        out.push(b[i]);
                        i += 1;
                    }
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

fn http_resp(status: &str, body: &str) -> Vec<u8> {
    let cors = "Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Content-Type, X-API-Key, Authorization\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\n";
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\n{cors}Connection: close\r\n\r\n{}",
        body.len(),
        body
    ).into_bytes()
}

fn handle_conn(mut s: std::net::TcpStream) {
    let resp = match read_request(&mut s) {
        Err(e) => http_resp("400 Bad Request", &format!("{{\"ok\":false,\"error\":\"{e}\"}}")),
        Ok(req) => route(req),
    };
    let _ = s.write_all(&resp);
    let _ = s.flush();
}

fn route(req: Req) -> Vec<u8> {
    if req.method == "OPTIONS" {
        return http_resp("204 No Content", ""); // CORS 预检直接放行
    }
    if req.path != "/push" {
        return http_resp("404 Not Found", "{\"ok\":false,\"error\":\"未知路径（请用 /push）\"}");
    }
    // 鉴权：密钥未设置 = 接口关闭
    let conf_key = read_config_key();
    if conf_key.is_empty() {
        return http_resp("403 Forbidden", "{\"ok\":false,\"error\":\"接口未启用（请在社交设置中生成密钥）\"}");
    }
    let given = req
        .query
        .iter()
        .find(|(k, _)| k == "key")
        .map(|(_, v)| v.clone())
        .or_else(|| req.headers.iter().find(|(k, _)| k == "x-api-key").map(|(_, v)| v.clone()))
        .or_else(|| {
            req.headers
                .iter()
                .find(|(k, _)| k == "authorization")
                .map(|(_, v)| v.strip_prefix("Bearer ").unwrap_or(v).to_string())
        })
        .unwrap_or_default();
    if given != conf_key {
        crate::social::socdbg("pushapi: 鉴权失败（密钥不匹配）");
        return http_resp("401 Unauthorized", "{\"ok\":false,\"error\":\"密钥无效\"}");
    }
    // 参数：GET 查询串 / POST JSON
    let (mut title, mut body) = (
        req.query.iter().find(|(k, _)| k == "title").map(|(_, v)| v.clone()).unwrap_or_default(),
        req.query.iter().find(|(k, _)| k == "body").map(|(_, v)| v.clone()).unwrap_or_default(),
    );
    if req.method == "POST" && (title.is_empty() || body.is_empty()) {
        if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&req.body) {
            let gt = v.get("title").and_then(|x| x.as_str()).unwrap_or("");
            let gb = v.get("body").and_then(|x| x.as_str()).unwrap_or("");
            if title.is_empty() {
                title = gt.to_string();
            }
            if body.is_empty() {
                body = gb.to_string();
            }
        }
    }
    let title = truncate_chars(title.trim(), TITLE_MAX);
    let body = truncate_chars(body.trim(), BODY_MAX);
    if title.is_empty() && body.is_empty() {
        return http_resp("400 Bad Request", "{\"ok\":false,\"error\":\"title 与 body 不能同时为空\"}");
    }
    let show_title = if title.is_empty() { "🔔 脚本推送".to_string() } else { title };
    crate::social::socdbg(&format!("pushapi: 推送 {show_title:?}"));
    push_notify(show_title, body);
    http_resp("200 OK", "{\"ok\":true}")
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

// ---------------------------- Tauri 命令 ----------------------------

#[derive(serde::Serialize)]
pub struct ApiState {
    pub enabled: bool,
    pub key: String,
    pub port: u16,
    pub recent: Vec<ApiPush>,
}

fn current_state() -> ApiState {
    let key = read_config_key();
    ApiState {
        enabled: !key.is_empty(),
        key,
        port: API_PORT,
        recent: API_PUSHES.lock().unwrap_or_else(|e| e.into_inner()).clone(),
    }
}

/// 生成（或重生成）10 位密钥并启用接口
#[tauri::command]
pub async fn api_key_gen() -> Result<ApiState, String> {
    let key = gen_key_str();
    crate::settings::update_config(|c| c.api_key = key.clone());
    crate::social::socdbg("pushapi: 密钥已生成，接口启用");
    Ok(current_state())
}

/// 清除密钥并关闭接口（历史保留供查看）
#[tauri::command]
pub async fn api_key_clear() -> Result<ApiState, String> {
    crate::settings::update_config(|c| c.api_key = String::new());
    crate::social::socdbg("pushapi: 密钥已清除，接口关闭");
    Ok(current_state())
}

/// 接口状态 + 最近推送（设置页渲染用）
#[tauri::command]
pub async fn api_state() -> Result<ApiState, String> {
    Ok(current_state())
}
