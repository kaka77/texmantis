// ---------------------------------------------------------------------------
// 社交模块「青刃·知己」—— 零服务器局域网即时通讯引擎（P2：加密文字聊天）
//
// 设计原则（对应需求评审定稿）：
// · 单端口复用：TCP 会话 + UDP 发现同号绑定（端口族 25100-25109，被占顺延），
//   全部流量走一个端口，绝不多端口扫描 / 全网段连接试探 —— 不触发安全设备告警
// · 无风暴：雷达按需扫描（2 个 UDP 包）；应答随机抖动 0-450ms 且对同一扫描者
//   10 秒内只应答一次；在线心跳仅在社交页打开时每 20 秒 1 个宣告包（页面关闭零流量）
// · 端到端加密：X25519（身份密钥持久化）DH 协商会话密钥 → ChaCha20-Poly1305 AEAD
//   加密所有后续帧（聊天/图片/文件统一走会话层，天然全覆盖）；SHA256 公钥指纹
//   前后端展示，双方核对一致即确认无中间人
// · 生命周期：随工具启动（默认开，设置可关），随进程退出即销毁 —— 无服务、无驱动、无常驻
// · TCP 帧协议：[magic TXZ1][type][flags][len:4][payload]；flags.0 = 已加密；
//   HELLO 明文携带档案+公钥，其余帧必须加密；未知类型安全忽略（前向兼容）
// ---------------------------------------------------------------------------

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, KeyInit, Nonce};
use x25519_dalek::{PublicKey, StaticSecret};

pub const PORT_BASE: u16 = 25100;
const PORT_SPAN: u16 = 10;
const MAGIC: [u8; 4] = *b"TXZ1";
const MAX_FRAME: u32 = 8 * 1024 * 1024;
const FLAG_ENC: u8 = 0x01;
const PEER_FRESH_MS: u64 = 90_000; // 在线判定：心跳 20s × 4.5 容忍
// v0.9.3：列表保留策略改为"历史联系人永久保留"（peers.json 落盘 + 仅手动移除；
// 容量由 save 的 200 条上限约束，按最近活跃排序截断）
const REPLY_SUPPRESS_MS: u64 = 10_000;
const MAX_TEXT_CHARS: usize = 4000;

// UDP 发现类型
const D_DISCOVER: u8 = 1;
const D_ANNOUNCE: u8 = 2;
const D_BYE: u8 = 3;
// TCP 帧类型（P2：HELLO + TEXT；P3+：FILE_*/SHAKE/ASSIST_*）
const T_HELLO: u8 = 16;
const T_TEXT: u8 = 17;
// v0.9.0：文件传输 / 群聊帧族（全部走既有加密会话层，未知类型安全忽略 = 前向兼容）
const T_FILE_OFFER: u8 = 18; // 发→收：JSON FileOffer（文件邀请）
const T_FILE_ACK: u8 = 19;   // 收→发：JSON FileAck（接受/拒绝 + 续传偏移）
const T_FILE_CHUNK: u8 = 20; // 发→收：[id16][seq:u32][data]（整帧 AEAD 加密）
const T_FILE_END: u8 = 21;   // 发→收：JSON FileEnd（传输结束 ok/失败）
const T_FILE_CTRL: u8 = 22;  // 双向：JSON FileCtrl（pause/resume/cancel）
const T_GROUP_TEXT: u8 = 23; // 成员→成员：JSON GroupText（群消息逐人单播分发）
const T_GROUP_SYNC: u8 = 24; // 群主/管理→成员：JSON GroupInfo（名单权威同步）
const T_GROUP_NUDGE: u8 = 25; // 群主/管理→成员：JSON GroupNudge（一键提醒：弹出社交界面）
const T_GROUP_LEAVE: u8 = 26; // 成员→群主/管理：JSON GroupLeave（退群通知）
const T_PEER_SHAKE: u8 = 27; // 好友抖一抖：JSON PeerShake（让对方窗口原生抖动 2 秒）
const T_READ: u8 = 28; // 好友已读回执：JSON ReadUpTo（"已读至某时间戳"，幂等且天然合并）

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Profile {
    pub uid: String,
    pub nickname: String,
    pub gender: String, // m / f / x（中性）
    pub avatar: u8,     // 0..=19
    pub port: u16,      // 对方 TCP 服务端口（同号族）
    pub host: String,   // 计算机名
    #[serde(default)]
    pub mid: String, // v0.9.3 机器指纹（MachineGuid 摘要 8 hex）：同机重装=新 uid 同 mid → 僵尸号识别
}

/// TCP 握手帧载荷（明文）：档案 + 身份公钥
#[derive(Serialize, Deserialize, Clone)]
struct HelloMsg {
    p: Profile,
    k: String, // X25519 公钥 hex64
}

#[derive(Serialize, Deserialize, Clone)]
struct UdpMsg {
    t: u8,
    sid: String, // 扫描会话 id（DISCOVER 携带，应答回带）
    p: Profile,
}

#[derive(Serialize, Clone)]
pub struct PeerOut {
    pub uid: String,
    pub nickname: String,
    pub remark: String, // v0.9.0 好友备注（本机设置，优先于昵称展示）
    pub gender: String,
    pub avatar: u8,
    pub host: String,
    pub ip: String,
    pub port: u16,
    pub online: bool,
    pub last_seen_ms: u64,
    pub fav: bool,
}

#[derive(Serialize, Clone)]
pub struct SocialStatus {
    pub running: bool,
    pub port: u16,
    pub reason: String,
    pub uid: String,
    pub nickname: String,
    pub gender: String,
    pub avatar: u8,
    pub host: String,
    pub fp: String,       // 本机加密指纹（16 hex）
    pub named: bool,      // 是否已生成/设置过昵称
    pub chat_left: bool,  // 两栏布局：聊天在左（记住的用户布局偏好）
    pub firewall: String, // ok / guest（非管理员，等待系统弹窗允许）/ fail
    pub peers: Vec<PeerOut>,
    pub favs: Vec<String>,
    pub groups: Vec<GroupInfo>, // v0.9.0 群列表
    pub xfers: Vec<XferOut>,    // v0.9.0 文件传输（进行中/近期）
    #[serde(default)]
    pub sessions: Vec<String>,  // v0.9.9 当前活跃加密会话的 uid（对方来信/发送成功都会建立）
}

#[derive(Serialize, Clone)]
pub struct OpenAck {
    pub fp: String, // 对方指纹（聊天头部展示，双方核对）
}

#[derive(Serialize, Clone)]
pub struct SendAck {
    pub ts: u64,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct HistItem {
    pub d: String, // in / out
    pub ts: u64,
    pub text: String,
    // v0.9.0 扩展（serde 默认兼容旧记录）：
    #[serde(default)]
    pub file: String,      // 非空 = 文件传输记录（文件名）
    #[serde(default)]
    pub size: u64,         // 文件字节数
    #[serde(default)]
    pub path: String,      // 落地/来源完整路径
    #[serde(default)]
    pub from: String,      // 群消息发送者 uid
    #[serde(default)]
    pub from_name: String, // 群消息发送者昵称
}

/// 已建立的加密会话
struct Sess {
    stream: Arc<Mutex<TcpStream>>,
    key: [u8; 32],
    fp: String,
}

static RUNNING: AtomicBool = AtomicBool::new(false);
static MY_PORT: AtomicU16 = AtomicU16::new(0);
static UDP_SOCK: Mutex<Option<Arc<UdpSocket>>> = Mutex::new(None);
static PEERS: LazyLock<Mutex<HashMap<String, (Profile, String, u64)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new())); // uid → (profile, "ip:port", last_seen_ms)
static REPLIED: LazyLock<Mutex<HashMap<String, Instant>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static SESSIONS: LazyLock<Mutex<HashMap<String, Sess>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static FW_STATE: LazyLock<Mutex<String>> = LazyLock::new(|| Mutex::new(String::new())); // 防火墙规则状态缓存

/// 中毒兜底锁：任何线程 panic 过也不级联（锁保护的数据结构自身一致即可用）
fn pl<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

// ------------------------- 社交调试日志（底层取证层） -------------------------
// 设计（问题定位期默认常开）：
// · 独立文件 data\social\debug.log，不依赖/不触碰配置锁，绝不与任何业务锁嵌套
// · 每行 = [进程相对毫秒][绝对毫秒][线程] 消息 —— 相邻行时间差即真实耗时
// · 一切可能阻塞的调用（netsh / sendto / 文件 / 连接 / 锁等待）用 BEGIN/END 括号包裹，
//   卡死时"有 BEGIN 无 END"的那一行就是元凶本体
// · 2MB 自动轮转（保留一份 .1），磁盘占用有界

// v0.9.27 发布默认关；排障时设环境变量 TEXMANTIS_SOCDBG=1 临时开启（LazyLock 首次访问时读取）
static SOCDBG_ON: LazyLock<AtomicBool> = LazyLock::new(|| AtomicBool::new(
    std::env::var("TEXMANTIS_SOCDBG").map(|v| v == "1").unwrap_or(false),
));
static SOCDBG_T0: LazyLock<Instant> = LazyLock::new(Instant::now);

/// 专职日志线程：所有文件 I/O 只发生在这里。调用方仅做通道投递（微秒级），
/// 即使磁盘写入被挂起，任何业务线程也绝不因此停顿 —— 取证层自身必须是免疫的。
static SOCDBG_TX: LazyLock<std::sync::mpsc::Sender<String>> = LazyLock::new(|| {
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let _ = std::thread::Builder::new().name("socdbg".into()).spawn(move || {
        let dir = crate::settings::social_dir();
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("debug.log");
        for line in rx {
            if let Ok(md) = std::fs::metadata(&p) {
                if md.len() > 2_000_000 {
                    let _ = std::fs::remove_file(dir.join("debug.log.1"));
                    let _ = std::fs::rename(&p, dir.join("debug.log.1"));
                }
            }
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
                let _ = f.write_all(line.as_bytes());
                let _ = f.write_all(b"\n");
            }
        }
    });
    tx
});

pub(crate) fn socdbg(msg: &str) {
    if !SOCDBG_ON.load(Ordering::Relaxed) {
        return;
    }
    let now = now_ms();
    let el = SOCDBG_T0.elapsed().as_millis();
    let th = std::thread::current().id();
    let _ = SOCDBG_TX.send(format!("[+{el:>10}ms][{now}][{th:?}] {msg}"));
}

/// 作用域括号：创建时打 BEGIN，drop 时打 END（含耗时）；卡死 = 悬空的 BEGIN
struct SocSpan(String, Instant);
impl SocSpan {
    fn new(tag: &str) -> Self {
        socdbg(&format!("BEGIN {tag}"));
        SocSpan(tag.to_string(), Instant::now())
    }
}
impl Drop for SocSpan {
    fn drop(&mut self) {
        socdbg(&format!("END   {} ({}ms)", self.0, self.1.elapsed().as_millis()));
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// 供兄弟模块（pushapi/monitor）复用的毫秒时间戳
pub fn now_ms_pub() -> u64 {
    now_ms()
}

fn hostname() -> String {
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into())
}

fn username() -> String {
    std::env::var("USERNAME").unwrap_or_else(|_| "用户".into())
}

fn hex_enc(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn hex_dec(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len() / 2).map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()).collect()
}

fn os_rand(buf: &mut [u8]) -> Result<(), String> {
    getrandom::getrandom(buf).map_err(|e| format!("随机源失败: {e}"))
}

fn hide(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(crate::commands::CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

fn is_admin() -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
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
        ok.is_ok() && elev.TokenIsElevated != 0
    }
    #[cfg(not(windows))]
    false
}

// ---------------------------- 身份密钥与加密 ----------------------------

/// 身份私钥：进程内仅初始化一次（首次访问生成并持久化），此后全部纯内存读取——
/// 状态查询路径绝不触碰配置写盘（v0.6.0 稳定性原则）
static IDKEY: LazyLock<[u8; 32]> = LazyLock::new(ensure_idkey);

/// 身份私钥（首次生成持久化；X25519 静态密钥，指纹稳定可核验）
fn ensure_idkey() -> [u8; 32] {
    let _sp = SocSpan::new("ensure_idkey");
    let hex = crate::settings::config().social_idkey.clone();
    if hex.len() == 64 {
        if let Some(b) = hex_dec(&hex) {
            if b.len() == 32 {
                let mut k = [0u8; 32];
                k.copy_from_slice(&b);
                socdbg("idkey: 已有（配置读取）");
                return k;
            }
        }
        socdbg("idkey: 配置中的密钥无效，重新生成");
    } else {
        socdbg("idkey: 配置为空，首次生成");
    }
    let mut sk = [0u8; 32];
    if os_rand(&mut sk).is_err() {
        socdbg("idkey: 随机源失败（罕见）");
        return [0u8; 32];
    }
    // 落盘移出调用线程：首次生成也不在状态查询路径上做任何磁盘写
    let hex2 = hex_enc(&sk);
    std::thread::spawn(move || {
        crate::settings::update_config(|c| c.social_idkey = hex2);
        socdbg("idkey: 已异步持久化");
    });
    sk
}

fn pubkey_of(sk: &[u8; 32]) -> [u8; 32] {
    *PublicKey::from(&StaticSecret::from(*sk)).as_bytes()
}

fn idkey() -> [u8; 32] {
    *IDKEY
}

fn fp_of(pub_bytes: &[u8; 32]) -> String {
    let d = Sha256::digest(pub_bytes);
    hex_enc(&d)[..16].to_string()
}

fn my_fp() -> String {
    fp_of(&pubkey_of(&idkey()))
}

/// DH 会话密钥协商 + 对方指纹
fn derive_key(their_pub: &[u8; 32]) -> ([u8; 32], String) {
    let shared = StaticSecret::from(idkey()).diffie_hellman(&PublicKey::from(*their_pub));
    (shared.to_bytes(), fp_of(their_pub))
}

fn cipher_of(key: &[u8; 32]) -> ChaCha20Poly1305 {
    ChaCha20Poly1305::new_from_slice(key).expect("key len")
}

/// AEAD 加密帧：payload = nonce(12) || ciphertext||tag
fn frame_send(s: &mut TcpStream, key: &[u8; 32], t: u8, payload: &[u8]) -> std::io::Result<()> {
    let mut nonce = [0u8; 12];
    os_rand(&mut nonce).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let ct = cipher_of(key)
        .encrypt(Nonce::from_slice(&nonce), payload)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::Other, "encrypt failed"))?;
    let mut buf = Vec::with_capacity(12 + ct.len());
    buf.extend_from_slice(&nonce);
    buf.extend_from_slice(&ct);
    write_frame(s, t, FLAG_ENC, &buf)
}

fn frame_recv(key: &[u8; 32], flags: u8, payload: Vec<u8>) -> std::io::Result<Vec<u8>> {
    if flags & FLAG_ENC == 0 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "未加密帧被拒绝"));
    }
    if payload.len() < 12 + 16 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "帧过短"));
    }
    let (nonce, ct) = payload.split_at(12);
    cipher_of(key)
        .decrypt(Nonce::from_slice(nonce), ct)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "解密失败（会话密钥不一致）"))
}

fn read_raw_frame(s: &mut TcpStream) -> std::io::Result<(u8, u8, Vec<u8>)> {
    let mut hdr = [0u8; 10];
    s.read_exact(&mut hdr)?;
    if hdr[0..4] != MAGIC {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "bad magic"));
    }
    let (t, flags) = (hdr[4], hdr[5]);
    let len = u32::from_be_bytes([hdr[6], hdr[7], hdr[8], hdr[9]]);
    if len > MAX_FRAME {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut payload = vec![0u8; len as usize];
    if len > 0 {
        s.read_exact(&mut payload)?;
    }
    Ok((t, flags, payload))
}

fn write_frame(s: &mut TcpStream, t: u8, flags: u8, payload: &[u8]) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(10 + payload.len());
    buf.extend_from_slice(&MAGIC);
    buf.push(t);
    buf.push(flags);
    buf.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    buf.extend_from_slice(payload);
    s.write_all(&buf)
}

// ---------------------------- 档案与防火墙 ----------------------------

/// 我的档案（读配置 + 兜底默认）
pub fn my_profile() -> Profile {
    let c = crate::settings::config();
    Profile {
        uid: c.social_uid.clone(),
        nickname: if c.social_nickname.is_empty() { username() } else { c.social_nickname.clone() },
        gender: c.social_gender.clone(),
        avatar: c.social_avatar.min(19),
        port: c.social_port,
        host: hostname(),
        mid: machine_mid(),
    }
}

/// 机器指纹：HKLM\...\Cryptography\MachineGuid 摘要前 8 hex（跨重装稳定）
fn machine_mid() -> String {
    use std::sync::OnceLock;
    static MID: OnceLock<String> = OnceLock::new();
    MID.get_or_init(|| {
        #[cfg(windows)]
        {
            const REG_SZ: u32 = 1;
            use windows::core::PCWSTR;
            use windows::Win32::Foundation::ERROR_SUCCESS;
            use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
            let sk = windows::core::w!("SOFTWARE\\Microsoft\\Cryptography");
            let vn = windows::core::w!("MachineGuid");
            let mut buf = [0u16; 128];
            let mut cb = (buf.len() * 2) as u32;
            let err = unsafe {
                RegGetValueW(
                    HKEY_LOCAL_MACHINE,
                    PCWSTR(sk.as_ptr()),
                    PCWSTR(vn.as_ptr()),
                    RRF_RT_REG_SZ,
                    None,
                    Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
                    Some(&mut cb),
                )
            };
            if err == ERROR_SUCCESS {
                let len = (cb as usize) / 2;
                let guid = String::from_utf16_lossy(&buf[..len.saturating_sub(1).min(127)]);
                let digest = sha2::Sha256::digest(guid.as_bytes());
                format!("{:02x}{:02x}", digest[0], digest[1])
            } else {
                String::new()
            }
        }
        #[cfg(not(windows))]
        {
            String::new()
        }
    })
    .clone()
}

/// 规则是否存在（netsh 查询无需管理员；仅供后台线程调用，绝不进命令路径）
fn fw_rule_exists() -> bool {
    #[cfg(windows)]
    {
        let name = "TexMantis Social";
        let mut check = std::process::Command::new("netsh");
        check.args(["advfirewall", "firewall", "show", "rule", &format!("name={name}")]);
        hide(&mut check);
        let t0 = Instant::now();
        socdbg("BEGIN netsh show rule（查询规则）");
        let out = check.output();
        socdbg(&format!("END   netsh show rule ({}ms)", t0.elapsed().as_millis()));
        match out {
            Ok(o) => {
                let found = String::from_utf8_lossy(&o.stdout).contains(name);
                socdbg(&format!("netsh show rule: exit={} found={found}", o.status.success()));
                found
            }
            Err(e) => {
                socdbg(&format!("netsh show rule 执行失败: {e}"));
                false
            }
        }
    }
    #[cfg(not(windows))]
    false
}



/// 防火墙：管理员静默添加仅限专用网络的入站规则（幂等，只加一次）；非管理员交给系统弹窗
fn ensure_firewall_rule() -> &'static str {
    let _sp = SocSpan::new("ensure_firewall_rule");
    #[cfg(windows)]
    {
        let admin = is_admin();
        socdbg(&format!("firewall: is_admin={admin}"));
        if !admin {
            return "guest";
        }
        if fw_rule_exists() {
            socdbg("firewall: 规则已存在 → ok");
            return "ok";
        }
        let ranges = format!("{}-{}", PORT_BASE, PORT_BASE + PORT_SPAN - 1);
        for proto in ["TCP", "UDP"] {
            let mut add = std::process::Command::new("netsh");
            add.args([
                "advfirewall", "firewall", "add", "rule",
                "name=TexMantis Social", "dir=in", "action=allow",
                &format!("protocol={proto}"), &format!("localport={ranges}"), "profile=private",
            ]);
            hide(&mut add);
            let tag = format!("netsh add rule {proto}");
            socdbg(&format!("BEGIN {tag}"));
            let t0 = Instant::now();
            let res = add.output();
            socdbg(&format!("END   {tag} ({}ms)", t0.elapsed().as_millis()));
            match res {
                Ok(o) if o.status.success() => {
                    socdbg(&format!("netsh add {proto}: 成功"));
                }
                Ok(o) => {
                    socdbg(&format!("netsh add {proto}: 失败 exit={:?} stderr={}", o.status.code(), String::from_utf8_lossy(&o.stderr).chars().take(120).collect::<String>()));
                    return "fail";
                }
                Err(e) => {
                    socdbg(&format!("netsh add {proto}: 执行失败 {e}"));
                    return "fail";
                }
            }
        }
        "ok"
    }
    #[cfg(not(windows))]
    "guest"
}

static BIND_REASON: Mutex<String> = Mutex::new(String::new());

/// 启动社交服务：绑定端口族 → UDP 发现循环 + TCP 会话接受循环（阻塞线程，进程退出即销毁）
pub fn start(_app: tauri::AppHandle) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    socdbg("===== 社交服务启动 session-begin =====");
    let _sp = SocSpan::new("start 总耗时");
    // 端口选择：优先上次成功端口，否则顺延探测
    let prefer = crate::settings::config().social_port;
    socdbg(&format!("start: 偏好端口 {prefer}"));
    let mut tried: Vec<u16> = Vec::new();
    if (PORT_BASE..PORT_BASE + PORT_SPAN).contains(&prefer) {
        tried.push(prefer);
    }
    for p in PORT_BASE..PORT_BASE + PORT_SPAN {
        if !tried.contains(&p) {
            tried.push(p);
        }
    }
    let mut bound: Option<(TcpListener, UdpSocket, u16)> = None;
    for p in &tried {
        let t0 = Instant::now();
        socdbg(&format!("BEGIN bind port {p}（TCP+UDP）"));
        let tcp = match TcpListener::bind(("0.0.0.0", *p)) {
            Ok(l) => l,
            Err(e) => {
                socdbg(&format!("bind {p} TCP 失败: {e} ({}ms)", t0.elapsed().as_millis()));
                continue;
            }
        };
        let udp = match UdpSocket::bind(("0.0.0.0", *p)) {
            Ok(u) => u,
            Err(e) => {
                socdbg(&format!("bind {p} UDP 失败: {e} ({}ms)", t0.elapsed().as_millis()));
                continue;
            }
        };
        socdbg(&format!("END   bind port {p} ({}ms) 成功", t0.elapsed().as_millis()));
        let _ = udp.set_broadcast(true);
        bound = Some((tcp, udp, *p));
        break;
    }
    let Some((tcp, udp, port)) = bound else {
        RUNNING.store(false, Ordering::SeqCst);
        let why = format!("端口族 {}-{} 均被其他程序占用，服务无法启动", PORT_BASE, PORT_BASE + PORT_SPAN - 1);
        *BIND_REASON.lock().unwrap_or_else(|e| e.into_inner()) = why.clone();
        socdbg(&format!("start: 失败 —— {why}"));
        crate::diag::write("W", &format!("社交服务端口族 {}-{} 全部被占，服务未启动", PORT_BASE, PORT_BASE + PORT_SPAN - 1));
        return;
    };
    MY_PORT.store(port, Ordering::SeqCst);
    socdbg(&format!("start: 绑定成功 port={port}"));
    {
        let _sp = SocSpan::new("start: save social_port");
        crate::settings::update_config(|c| c.social_port = port);
    }
    {
        let _sp = SocSpan::new("start: 首次 profile/fp（含 idkey 初始化）");
        crate::diag::write("I", &format!("社交服务上线 port={port} uid={} fp={}", my_profile().uid, my_fp()));
    }
    // 防火墙规则：在启动线程内直接执行一次（v0.6.0 语义）——只阻塞本线程，与 UI 无关
    let fw = ensure_firewall_rule().to_string();
    *pl(&FW_STATE) = fw.clone();
    socdbg(&format!("start: 防火墙状态 = {fw}"));
    crate::diag::write("I", &format!("社交防火墙规则: {fw}"));
    let udp = Arc::new(udp);
    *pl(&UDP_SOCK) = Some(udp.clone());
    load_groups(); // v0.9.0：启动加载本地群列表（创建者权威镜像）
    load_peers(); // v0.9.3：恢复历史联系人（离线灰显，稍后由一级心跳探测）
    load_read_state(); // v0.9.21：恢复已读回执（重启后"已读"标记不丢）
    // 上线即宣告一次（v0.6.0 语义：本线程直接发送）
    announce_broadcast(&udp, D_ANNOUNCE, String::new());
    socdbg(&format!("start: 收包/会话循环拉起（udp+tcp）—— 服务就绪 port={port}"));
    {
        let udp = udp.clone();
        std::thread::spawn(move || udp_loop(udp));
    }
    std::thread::spawn(move || accept_loop(tcp));
    // v0.9.2 在线保活：工具开着就"在线"，与社交页是否打开无关
    let udp_ka = udp.clone();
    std::thread::spawn(move || keepalive_loop(udp_ka));
}

/// 两级心跳（轻量与安静优先）：
/// ① 25 秒一个宣告包（对端 udp_loop 常收，工具开着 → 我在对方名单里始终在线）；
/// ② 一级：对"见过但 30 秒没动静"的历史联系人做单播微型探测（~40B，直连、不广播，
///    每 2 秒至多 1 个、最久未见优先）——对方的应答会刷新其 last_seen；
/// ③ 二级：仅当名单全空时，才做一次局域网 DISCOVER 探测（90 秒至多一次，含 /24 定向广播）。
/// 全程无泛洪：最坏稳态 ≈ 每秒 0.5 个微小包，聊天活跃时探测自动归零。
fn keepalive_loop(udp: Arc<UdpSocket>) {
    socdbg("keepalive: 两级心跳线程启动（25s 宣告 + 历史联系人单播探测）");
    // 启动即宣告 + 探测一次：工具一开就对局域网可见、也主动找老朋友
    announce_broadcast(&udp, D_ANNOUNCE, String::new());
    announce_broadcast(&udp, D_DISCOVER, String::new());
    let mut last_ann = now_ms();
    let mut last_bcast = now_ms();
    let mut rr: usize = 0;
    loop {
        std::thread::sleep(Duration::from_secs(2));
        let now = now_ms();
        save_peers_if_dirty(); // 历史联系人落盘（脏标记 + 10 秒节流，不新增线程）
        if now.saturating_sub(last_ann) > 25_000 {
            last_ann = now;
            announce_broadcast(&udp, D_ANNOUNCE, String::new());
        }
        let victims: Vec<String> = {
            let g = PEERS.lock().unwrap_or_else(|e| e.into_inner());
            let mut v: Vec<(u64, String)> = g
                .values()
                .filter(|(_, _, seen)| now.saturating_sub(*seen) > 30_000)
                .map(|(_, addr, seen)| (*seen, addr.clone()))
                .collect();
            v.sort_by_key(|(seen, _)| *seen); // 最久未见优先
            v.into_iter().map(|(_, a)| a).collect()
        };
        if !victims.is_empty() {
            let addr = &victims[rr % victims.len()];
            rr = rr.wrapping_add(1);
            // sid 每次探测唯一：对端应答抑制按 sid+我方uid 记账，唯一 sid 才能让每个被探测者都应答
            let msg = UdpMsg { t: D_DISCOVER, sid: format!("hb{}", now_ms()), p: my_profile() };
            if let Ok(bin) = serde_json::to_vec(&msg) {
                let _ = udp.send_to(&bin, addr.as_str());
            }
        } else if now.saturating_sub(last_bcast) > 90_000 {
            last_bcast = now;
            announce_broadcast(&udp, D_DISCOVER, String::new());
        }
    }
}

// ---------------------------- UDP 发现 ----------------------------

fn announce_broadcast(udp: &UdpSocket, t: u8, sid: String) {
    let ty = if t == D_DISCOVER { "DISCOVER" } else { "ANNOUNCE" };
    let _sp = SocSpan::new(&format!("announce {ty}"));
    let msg = UdpMsg { t, sid, p: my_profile() };
    if let Ok(bin) = serde_json::to_vec(&msg) {
        let targets = broadcast_targets(udp);
        for target in targets {
            let t0 = Instant::now();
            match udp.send_to(&bin, target.as_str()) {
                Ok(n) => socdbg(&format!("sendto {target} → {n}B ({}ms)", t0.elapsed().as_millis())),
                Err(e) => socdbg(&format!("sendto {target} 失败: {e} ({}ms)", t0.elapsed().as_millis())),
            }
        }
    }
}

/// 广播地址：全网广播 + 本机所在 /24 子网定向广播（跨网段路由器不转发，天然无风暴）
fn broadcast_targets(udp: &UdpSocket) -> Vec<String> {
    let port = MY_PORT.load(Ordering::SeqCst);
    let mut out = vec![format!("255.255.255.255:{port}")];
    if let Ok(local) = udp.local_addr() {
        if let std::net::IpAddr::V4(v4) = local.ip() {
            let o = v4.octets();
            out.push(format!("{}.{}.{}.255:{port}", o[0], o[1], o[2]));
        }
    }
    out
}

fn udp_loop(udp: Arc<UdpSocket>) {
    let mut buf = [0u8; 2048];
    loop {
        let (n, src) = match udp.recv_from(&mut buf) {
            Ok(x) => x,
            Err(e) => {
                socdbg(&format!("recv_from 失败: {e}，200ms 后重试"));
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
        };
        let Ok(msg) = serde_json::from_slice::<UdpMsg>(&buf[..n]) else {
            socdbg(&format!("recv {n}B from {src}：非本协议包，忽略"));
            continue;
        };
        let me = my_profile();
        if msg.p.uid == me.uid {
            continue; // 自己/回环
        }
        socdbg(&format!("recv t={} from {} uid={}", msg.t, src, &msg.p.uid[..8.min(msg.p.uid.len())]));
        match msg.t {
            D_DISCOVER => {
                // 应答抑制：同一扫描者 10 秒内只应答一次（防回包风暴）
                let fresh = {
                    let mut rp = REPLIED.lock().unwrap_or_else(|e| e.into_inner());
                    rp.retain(|_, t| t.elapsed() < Duration::from_millis(REPLY_SUPPRESS_MS * 6));
                    let key = format!("{}|{}", msg.sid, msg.p.uid);
                    if rp.contains_key(&key) {
                        false
                    } else {
                        rp.insert(key, Instant::now());
                        true
                    }
                };
                if !fresh {
                    socdbg("discover 应答被抑制（10s 内已答过）");
                    continue;
                }
                let jit = (now_ms() % 450) as u64;
                socdbg(&format!("discover → 应答（抖动 {jit}ms）"));
                std::thread::sleep(Duration::from_millis(jit)); // 0-450ms 抖动
                let reply = UdpMsg { t: D_ANNOUNCE, sid: msg.sid.clone(), p: me };
                if let Ok(bin) = serde_json::to_vec(&reply) {
                    let t0 = Instant::now();
                    let r = udp.send_to(&bin, src);
                    socdbg(&format!("应答 sendto {src}: {:?} ({}ms)", r.map(|n| format!("{n}B")), t0.elapsed().as_millis()));
                }
            }
            D_ANNOUNCE => {
                let ip = src.ip().to_string();
                let addr = format!("{ip}:{}", msg.p.port);
                // v0.9.3：手动移除过的联系人不再自动回到列表（对方发来消息时才会恢复）
                let is_removed = crate::settings::config().social_removed.contains(&msg.p.uid);
                if !is_removed {
                    let mid = msg.p.mid.clone();
                    {
                        let mut g = PEERS.lock().unwrap_or_else(|e| e.into_inner());
                        g.insert(msg.p.uid.clone(), (msg.p, addr, now_ms()));
                    } // 锁只护插入，事件发送绝不持锁（锁序卫生）
                    if !mid.is_empty() { zombie_sweep("announce"); }
                    mark_peers_dirty();
                }
                if let Some(app) = crate::settings::APP.get() {
                    use tauri::Emitter;
                    let _ = app.emit("social-peers", ());
                }
            }
            D_BYE => {
                // v0.9.3：对方正常退出 = 标记为离线（保留在历史联系人里），不再整条删除
                if let Some((_, _, seen)) = PEERS.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&msg.p.uid) {
                    *seen = now_ms().saturating_sub(PEER_FRESH_MS + 60_000);
                    mark_peers_dirty();
                }
            }
            _ => {}
        }
    }
}

// ---------------------------- TCP 会话（加密握手） ----------------------------

fn accept_loop(listener: TcpListener) {
    for stream in listener.incoming() {
        match stream {
            Ok(s) => {
                std::thread::spawn(move || handle_inbound(s));
            }
            Err(_) => {
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

fn hello_of(me: &Profile) -> Vec<u8> {
    let msg = HelloMsg { p: me.clone(), k: hex_enc(&pubkey_of(&idkey())) };
    serde_json::to_vec(&msg).unwrap_or_default()
}

fn register_session(p: Profile, stream: &TcpStream, key: [u8; 32], fp: String) -> Arc<Mutex<TcpStream>> {
    let ip = stream.peer_addr().map(|a| a.ip().to_string()).unwrap_or_default();
    {
        let mut g = PEERS.lock().unwrap_or_else(|e| e.into_inner());
        g.insert(p.uid.clone(), (p.clone(), format!("{ip}:{}", p.port), now_ms()));
    }
    let shared = Arc::new(Mutex::new(stream.try_clone().expect("clone")));
    SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).insert(p.uid, Sess { stream: shared.clone(), key, fp });
    shared
}

/// 入站：读对方 HELLO → 协商 → 回自己 HELLO → 会话循环
fn handle_inbound(mut stream: TcpStream) {
    let peer = stream.peer_addr().map(|a| a.to_string()).unwrap_or_default();
    socdbg(&format!("inbound: TCP 连接进入 {peer}"));
    // v0.8.1：握手读 3s 超时 —— 对端半开/不回握手时不允许永久挂起
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let (t, flags, payload) = match read_raw_frame(&mut stream) {
        Ok(x) => x,
        Err(e) => {
            socdbg(&format!("inbound: 握手读取失败 {peer} err={e}"));
            return;
        }
    };
    if t != T_HELLO || flags & FLAG_ENC != 0 {
        return;
    }
    let Ok(hello) = serde_json::from_slice::<HelloMsg>(&payload) else { return };
    let me = my_profile();
    if hello.p.uid == me.uid {
        return;
    }
    let Some(their_pub) = hex_dec(&hello.k) else { return };
    let Ok(their_pub) = <[u8; 32]>::try_from(their_pub.as_slice()) else { return };
    let (key, fp) = derive_key(&their_pub);
    if write_frame(&mut stream, T_HELLO, 0, &hello_of(&me)).is_err() {
        return;
    }
    // v0.8.1：会话空闲 5 分钟自动回收（下次发送自动重连）；写 10s 超时防死等
    let _ = stream.set_read_timeout(Some(Duration::from_secs(300)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let shared = register_session(hello.p.clone(), &stream, key, fp);
    session_loop(&mut stream, &hello.p.uid, &key, shared);
}

/// 出站：连接 → 发自己 HELLO → 读对方 HELLO → 协商
fn open_session(uid: &str) -> Result<String, String> {
    let _sp = SocSpan::new("open_session");
    {
        let g = SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = g.get(uid) {
            socdbg("open_session: 复用现有会话");
            return Ok(s.fp.clone());
        }
    }
    let addr: String = {
        let g = PEERS.lock().unwrap_or_else(|e| e.into_inner());
        g.get(uid)
            .map(|(_, a, _)| a.clone())
            .ok_or("尚未发现对方 —— 先「一键雷达」，并确认对方社交页已打开")?
    };
    let addr_v = addr.parse().map_err(|_| "对方地址无效")?;
    socdbg(&format!("open_session: connect {addr}（≤3s）"));
    let t0 = Instant::now();
    let mut stream = TcpStream::connect_timeout(&addr_v, Duration::from_secs(3))
        .map_err(|e| {
            socdbg(&format!("open_session: connect 失败 {e} ({}ms)", t0.elapsed().as_millis()));
            "对方不在线或其防火墙未放行（对方需打开过社交页）"
        })?;
    socdbg(&format!("open_session: connect 成功 ({}ms)", t0.elapsed().as_millis()));
    // v0.8.1：握手读 3s 超时 —— 对端进程在但不回握手时快速报错，绝不挂起界面
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    if write_frame(&mut stream, T_HELLO, 0, &hello_of(&my_profile())).is_err() {
        return Err("握手发送失败".into());
    }
    let (t, flags, payload) = read_raw_frame(&mut stream).map_err(|_| "握手读取失败（对方无响应）")?;
    if t != T_HELLO || flags & FLAG_ENC != 0 {
        return Err("握手协议不符".into());
    }
    let Ok(hello) = serde_json::from_slice::<HelloMsg>(&payload) else {
        return Err("握手档案解析失败".into());
    };
    let Some(their_pub) = hex_dec(&hello.k) else { return Err("握手公钥无效".into()) };
    let Ok(their_pub) = <[u8; 32]>::try_from(their_pub.as_slice()) else { return Err("握手公钥无效".into()) };
    let (key, fp) = derive_key(&their_pub);
    // v0.8.1：会话空闲 5 分钟自动回收（下次发送自动重连）；写 10s 超时防死等
    let _ = stream.set_read_timeout(Some(Duration::from_secs(300)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(10)));
    let shared = register_session(hello.p.clone(), &stream, key, fp.clone());
    {
        let mut g = PEERS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((p, _a, seen)) = g.get_mut(uid) {
            *seen = now_ms();
            p.nickname = hello.p.nickname.clone();
            p.gender = hello.p.gender.clone();
            p.avatar = hello.p.avatar;
        }
    }
    let uid_owned = uid.to_string();
    std::thread::spawn(move || {
        session_loop(&mut stream, &uid_owned, &key, shared);
    });
    Ok(fp)
}

/// 会话循环：解密分发；TEXT → 落盘 + 推送前端；未知类型安全忽略（前向兼容）
/// v0.8.1：退出时按「会话所有权」清理登记表（Arc 指针对比），绝不误删后来的新会话
fn session_loop(stream: &mut TcpStream, uid: &str, key: &[u8; 32], shared: Arc<Mutex<TcpStream>>) {
    socdbg(&format!("session_loop: 开始 uid={}", &uid[..8.min(uid.len())]));
    loop {
        let (t, flags, payload) = match read_raw_frame(stream) {
            Ok(x) => x,
            Err(e) => {
                let idle = e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut;
                socdbg(&format!(
                    "session_loop: {} uid={} err={}",
                    if idle { "会话空闲超时（5 分钟无消息，自动回收；下次发送自动重连）" } else { "帧读取结束" },
                    &uid[..8.min(uid.len())],
                    e
                ));
                break;
            }
        };
        if t == T_HELLO {
            continue; // 会话中重复 HELLO 忽略
        }
        let payload = match frame_recv(key, flags, payload) {
            Ok(p) => p,
            Err(e) => {
                socdbg(&format!("session_loop: 解密失败（密钥不符或帧损坏）: {e}"));
                break;
            }
        };
        if t == T_TEXT {
            let text = String::from_utf8_lossy(&payload).to_string();
            let ts = now_ms();
            // v0.9.3：删除≠拉黑 —— 被移除的联系人主动发来消息时自动回到联系人列表
            if crate::settings::config().social_removed.contains(&uid.to_string()) {
                crate::settings::update_config(|c| c.social_removed.retain(|x| x != uid));
                socdbg(&format!("peers: 已移除联系人 {} 来信，自动恢复", &uid[..8.min(uid.len())]));
            }
            socdbg(&format!("session_loop: 收到文字 uid={} {}字", &uid[..8.min(uid.len())], text.chars().count()));
            persist_msg(uid, "in", &text, ts);
            let remark = crate::settings::config().social_remarks.get(uid).cloned().unwrap_or_default();
            let nickname =
                PEERS.lock().unwrap_or_else(|e| e.into_inner()).get(uid).map(|(p, _, _)| p.nickname.clone()).unwrap_or_default();
            emit_ev("social-msg", serde_json::json!({ "uid": uid, "nickname": nickname, "text": text, "ts": ts }));
            let who = if remark.is_empty() { nickname.clone() } else { remark };
            social_bubble_notify(&format!("peer:{uid}"), &format!("💬 {}", if who.is_empty() { "新消息" } else { &who }),
                &{ let mut t: String = text.chars().take(60).collect(); if text.chars().count() > 60 { t.push('…'); } t },
                &format!("peer:{uid}"));
        }
        // v0.9.0：文件传输 / 群聊 帧分发（全部已解密；未知类型安全忽略 = 前向兼容）
        match t {
            T_FILE_OFFER => handle_file_offer(uid, &payload),
            T_FILE_ACK => route_ack(&payload, shared.clone(), *key),
            T_FILE_CHUNK => route_chunk(&payload),
            T_FILE_END => route_end(&payload),
            T_FILE_CTRL => route_ctrl(&payload),
            T_GROUP_TEXT => handle_group_text(uid, &payload),
            T_GROUP_SYNC => handle_group_sync(uid, &payload),
            T_GROUP_NUDGE => handle_group_nudge(uid, &payload),
            T_PEER_SHAKE => handle_peer_shake(uid, &payload),
            T_READ => handle_read(uid, &payload),
            T_GROUP_LEAVE => handle_group_leave(uid, &payload),
            _ => {}
        }
    }
    // 归还清理：仅当登记表里仍是自己这个会话才移除（防止误删后来者注册的新会话）
    SESSIONS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|_, s| !Arc::ptr_eq(&s.stream, &shared));
}

// ---------------------------- 消息存储 ----------------------------

fn msg_file(uid: &str) -> Option<std::path::PathBuf> {
    // v0.9.0：允许下划线（群记录键 = group_{gid}）
    if uid.is_empty() || !uid.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    let dir = crate::settings::social_dir().join("msgs");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join(format!("{uid}.jsonl")))
}

fn persist_msg(uid: &str, dir: &str, text: &str, ts: u64) {
    let _sp = SocSpan::new("persist_msg");
    if let Some(f) = msg_file(uid) {
        let line = serde_json::json!({ "d": dir, "ts": ts, "text": text });
        let t0 = Instant::now();
        match std::fs::OpenOptions::new().create(true).append(true).open(&f) {
            Ok(mut fh) => {
                if let Err(e) = writeln!(fh, "{line}") {
                    socdbg(&format!("persist_msg: 写入失败 {e}"));
                } else {
                    socdbg(&format!("persist_msg: {} ({}ms)", f.display(), t0.elapsed().as_millis()));
                }
            }
            Err(e) => socdbg(&format!("persist_msg: 打开失败 {e}")),
        }
    }
}

// ---------------------------- 对外命令 ----------------------------

fn collect_peers() -> Vec<PeerOut> {
    let now = now_ms();
    let (favs, remarks, removed) = {
        let c = crate::settings::config();
        (c.social_favs.clone(), c.social_remarks.clone(), c.social_removed.clone())
    };
    let mut g = PEERS.lock().unwrap_or_else(|e| e.into_inner());
    g.retain(|uid, _| !removed.contains(uid)); // 历史联系人保留（离线灰显），仅手动移除才消失
    let mut out: Vec<PeerOut> = g
        .iter()
        .map(|(uid, (p, addr, seen))| PeerOut {
            uid: uid.clone(),
            nickname: p.nickname.clone(),
            remark: remarks.get(uid).cloned().unwrap_or_default(),
            gender: p.gender.clone(),
            avatar: p.avatar.min(19),
            host: p.host.clone(),
            ip: addr.rsplit_once(':').map(|(i, _)| i.to_string()).unwrap_or_default(),
            port: p.port,
            online: now.saturating_sub(*seen) < PEER_FRESH_MS,
            last_seen_ms: *seen,
            fav: favs.contains(uid),
        })
        .collect();
    out.sort_by(|a, b| b.online.cmp(&a.online).then(a.nickname.cmp(&b.nickname)));
    out
}

fn status(reason: String) -> SocialStatus {
    let running = RUNNING.load(Ordering::SeqCst);
    let me = { let _sp = SocSpan::new("status: my_profile"); my_profile() };
    let reason = if !running && reason.is_empty() { BIND_REASON.lock().unwrap_or_else(|e| e.into_inner()).clone() } else { reason };
    let fw = if running {
        let _sp = SocSpan::new("status: fw_state 锁");
        FW_STATE.lock().unwrap_or_else(|e| e.into_inner()).clone()
    } else { String::new() };
    // v0.8.0 根因修复：同一条 let 语句内多次 config() 的临时 MutexGuard
    // 会存活到语句末尾（Rust 临时值规则），第二次 config() 与第一次自死锁。
    // 必须先一次性取锁读全字段并绑定局部变量，守卫在此处立即归还。
    let (named, chat_left, favs) = {
        let _sp = SocSpan::new("status: cfg 一次读取");
        let c = crate::settings::config();
        (c.social_named, c.social_chat_left, c.social_favs.clone())
    };
    let peers = { let _sp = SocSpan::new("status: collect_peers"); collect_peers() };
    // v0.9.0：群列表 + 传输条目（各自独立互斥锁，绝不与配置锁交叉）
    let (groups, xfers) = {
        let gmap = GROUPS.lock().unwrap_or_else(|e| e.into_inner());
        let mut gv: Vec<GroupInfo> = gmap.values().cloned().collect();
        gv.sort_by(|a, b| a.name.cmp(&b.name));
        let xmap = XFERS.lock().unwrap_or_else(|e| e.into_inner());
        let mut xv: Vec<XferOut> = xmap.values().map(|x| xfer_out(x)).collect();
        xv.sort_by(|a, b| b.st.cmp(&a.st).then(b.done.cmp(&a.done)));
        xv.truncate(24);
        (gv, xv)
    };
    // v0.9.9 活跃会话表（独立互斥锁，供前端自愈式会话状态）
    let sessions = {
        let g = SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
        g.keys().cloned().collect::<Vec<_>>()
    };
    // v0.9.11：入站 st5（待回应邀请）若 PENDING_IN 已无对应条目 = 邀请已失效，同步回收，
    // 不再让"僵尸邀请卡"长期占用聊天窗口（出站 st5 不经 PENDING_IN，不受影响）
    {
        let mut xg = XFERS.lock().unwrap_or_else(|e| e.into_inner());
        let pg = PENDING_IN.lock().unwrap_or_else(|e| e.into_inner());
        xg.retain(|id, x| xst(x) != 5 || x.dir != "in" || pg.contains_key(id));
    }
    // v0.9.16：历史终态（完成/取消/失败）10 秒后回收 —— 重开界面不再闪现旧传输状态
    {
        let now = now_ms();
        let mut xg = XFERS.lock().unwrap_or_else(|e| e.into_inner());
        xg.retain(|_, x| {
            let st = xst(x);
            if st != 2 && st != 3 && st != 4 { return true; }
            now.saturating_sub(x.t_end.load(Ordering::SeqCst)) < 10_000
        });
    }
    let st = SocialStatus {
        running,
        port: MY_PORT.load(Ordering::SeqCst),
        reason,
        uid: me.uid,
        nickname: me.nickname,
        gender: me.gender,
        avatar: me.avatar,
        host: me.host,
        fp: my_fp(),
        named,
        chat_left,
        firewall: fw,
        peers,
        favs,
        groups,
        xfers,
        sessions,
    };
    socdbg(&format!("status: 组装完成 running={} port={} peers={} fw={}", st.running, st.port, st.peers.len(), if st.firewall.is_empty() { "-" } else { &st.firewall }));
    st
}

/// 状态查询（v0.6.0 语义）：一次宣告 + 内存收集。社交页打开期间的轮询即心跳，关闭零流量
#[tauri::command]
pub async fn social_get_status() -> SocialStatus {
    let _sp = SocSpan::new("cmd get_status");
    // v0.9.2：开过社交页 → 关窗驻留/退出确认生效；轮询时间戳 → 气泡打扰判定
    let first_open = !SOCIAL_OPENED.swap(true, std::sync::atomic::Ordering::SeqCst);
    LAST_PAGE_POLL.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
    // v0.9.3：首次打开社交页 → 立即补一轮局域网发现（与前端雷达互为双保险，2 个小包）
    if first_open && RUNNING.load(Ordering::SeqCst) {
        if let Some(udp) = pl(&UDP_SOCK).clone() {
            announce_broadcast(&udp, D_DISCOVER, String::new());
        }
    }
    if RUNNING.load(Ordering::SeqCst) {
        if let Some(udp) = pl(&UDP_SOCK).clone() {
            announce_broadcast(&udp, D_ANNOUNCE, String::new());
        }
    } else {
        socdbg("cmd get_status: 服务未运行");
    }
    status(String::new())
}

/// 一键雷达：按需扫描（2 个探测包），稍候收拢结果
#[tauri::command]
pub async fn social_scan() -> SocialStatus {
    LAST_PAGE_POLL.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
    // 1.8s 收拢窗口在阻塞线程池执行，不占用异步工作线程，更不碰主线程
    tauri::async_runtime::spawn_blocking(|| {
        let _sp = SocSpan::new("cmd scan（含 1.8s 收拢窗口）");
        let Some(udp) = pl(&UDP_SOCK).clone() else {
            return status("服务未运行".into());
        };
        let sid = format!("S{}-{}", now_ms(), &my_profile().uid[..8.min(my_profile().uid.len())]);
        announce_broadcast(&udp, D_DISCOVER, sid.clone());
        std::thread::sleep(Duration::from_millis(700));
        announce_broadcast(&udp, D_DISCOVER, sid);
        std::thread::sleep(Duration::from_millis(1100)); // 等待对端抖动应答回流
        let st = status(String::new());
        socdbg(&format!("scan: 收拢完成 peers={}", st.peers.len()));
        st
    })
    .await
    .unwrap_or_else(|_| status("扫描任务失败".into()))
}

/// 保存我的档案（昵称/性别/头像），并立即广播宣告让同伴看到变化
#[tauri::command]
pub async fn social_set_profile(nickname: String, gender: String, avatar: u8) -> SocialStatus {
    let _sp = SocSpan::new("cmd set_profile");
    let gender = match gender.as_str() { "m" | "f" => gender, _ => "x".into() };
    let nickname = nickname.trim().chars().take(16).collect::<String>();
    crate::settings::update_config(|c| {
        c.social_nickname = nickname;
        c.social_gender = gender;
        c.social_avatar = avatar.min(19);
        c.social_named = true;
    });
    if let Some(udp) = UDP_SOCK.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        announce_broadcast(&udp, D_ANNOUNCE, String::new());
    }
    status(String::new())
}

/// 收藏列表（好友）管理：整体保存
#[tauri::command]
pub async fn social_set_favs(favs: Vec<String>) -> SocialStatus {
    let _sp = SocSpan::new("cmd set_favs");
    crate::settings::update_config(|c| {
        c.social_favs = favs.into_iter().collect::<std::collections::BTreeSet<_>>().into_iter().collect();
    });
    status(String::new())
}

/// 保存两栏布局偏好（聊天在左/在右），配置持久化，重启后保持
#[tauri::command]
pub async fn social_set_layout(chat_left: bool) -> SocialStatus {
    let _sp = SocSpan::new("cmd set_layout");
    crate::settings::update_config(|c| c.social_chat_left = chat_left);
    status(String::new())
}

/// 一键修复网络权限：弹出 UAC（系统级"请求用户允许"），管理员添加防火墙入站规则；
/// 4 秒后后台复核规则并刷新快照。用户点「否」则状态保持待修复，可随时再点
#[tauri::command]
pub async fn social_fix_firewall() -> SocialStatus {
    socdbg("cmd fix_firewall: 已拉起后台修复线程");
    std::thread::spawn(|| {
        {
            let _sp = SocSpan::new("fix_firewall_elevated（UAC）");
            fix_firewall_elevated();
        }
        std::thread::sleep(Duration::from_secs(4));
        let ok = fw_rule_exists();
        *pl(&FW_STATE) = if ok { "ok" } else { "fail" }.to_string();
        socdbg(&format!("fix_firewall: 4s 后复核规则 exists={ok}"));
        status(String::new());
    });
    status(String::new())
}

#[cfg(windows)]
fn fix_firewall_elevated() {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::{ShellExecuteExW, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    let ranges = format!("{}-{}", PORT_BASE, PORT_BASE + PORT_SPAN - 1);
    let mk = |proto: &str| {
        format!(
            "netsh advfirewall firewall add rule name=\"TexMantis Social\" dir=in action=allow protocol={proto} localport={ranges} profile=private"
        )
    };
    let ps = format!("-NoProfile -WindowStyle Hidden -Command \"{}; {}\"", mk("TCP"), mk("UDP"));
    let to_w = |x: &str| x.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let verb = to_w("runas");
    let file = to_w("powershell.exe");
    let params = to_w(&ps);
    unsafe {
        let mut sei = SHELLEXECUTEINFOW::default();
        sei.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        sei.fMask = 0x1000_0000; // SEE_MASK_NOASYNC：后台线程调用安全
        sei.lpVerb = PCWSTR(verb.as_ptr());
        sei.lpFile = PCWSTR(file.as_ptr());
        sei.lpParameters = PCWSTR(params.as_ptr());
        sei.nShow = SW_HIDE.0;
        let _ = ShellExecuteExW(&mut sei);
    }
    socdbg("fix_firewall: ShellExecuteExW(runas) 已返回（UAC 弹窗已交出）");
    crate::diag::write("I", "社交防火墙修复：已请求管理员权限（UAC 弹窗）");
}

#[cfg(not(windows))]
fn fix_firewall_elevated() {}

/// 打开（或复用）与对方的加密会话，返回对方指纹供双方核对
#[tauri::command]
pub async fn social_open_session(uid: String) -> Result<OpenAck, String> {
    let uid2 = uid.clone();
    tauri::async_runtime::spawn_blocking(move || open_session(&uid2))
        .await
        .map_err(|_| "会话任务失败".to_string())?
        .map(|fp| OpenAck { fp })
}

/// 发送加密文字消息
#[tauri::command]
pub async fn social_send_text(uid: String, text: String) -> Result<SendAck, String> {
    tauri::async_runtime::spawn_blocking(move || send_text_impl(uid, text))
        .await
        .map_err(|_| "发送任务失败".to_string())?
}

fn send_text_impl(uid: String, text: String) -> Result<SendAck, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("消息为空".into());
    }
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(format!("消息过长（≤{MAX_TEXT_CHARS} 字）"));
    }
    if !SESSIONS.lock().unwrap_or_else(|e| e.into_inner()).contains_key(&uid) {
        open_session(&uid)?;
    }
    let (stream, key) = {
        let g = SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
        let s = g.get(&uid).ok_or("会话不存在")?;
        (s.stream.clone(), s.key)
    };
    let ts = now_ms();
    let mut s = stream.lock().unwrap_or_else(|e| e.into_inner());
    frame_send(&mut s, &key, T_TEXT, text.as_bytes()).map_err(|e| {
        // 仅清理仍属于自己的会话条目，避免误删并发重连的新会话
        SESSIONS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|_, x| !Arc::ptr_eq(&x.stream, &stream));
        format!("发送失败（对方可能已离线）: {e}")
    })?;
    drop(s);
    persist_msg(&uid, "out", &text, ts);
    Ok(SendAck { ts })
}

// ------------------- v0.9.12 轻量资源管理器（应用内文件选择，免原生对话框） -------------------

#[derive(serde::Serialize)]
pub struct DirEntryOut {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: u64, // ms
}

#[derive(serde::Serialize)]
pub struct DirListing {
    pub path: String,          // 规范化后的当前路径（特殊路径返回真实路径）
    pub parent: Option<String>, // ""=此电脑（驱动器层）
    pub entries: Vec<DirEntryOut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sel: Option<String>, // 输入的是文件路径时：预选该文件
}

fn is_junk_dir(name: &str) -> bool {
    name.starts_with('$') || name == "System Volume Information" || name.starts_with('.')
}

/// 列目录（""=此电脑驱动器；"@desktop"/"@downloads"/"@docs"=已知目录）。
/// 纯 Rust 标准库遍历，无 COM/对话框开销 —— 响应即时
#[tauri::command]
pub async fn social_list_dir(path: String) -> Result<DirListing, String> {
    tauri::async_runtime::spawn_blocking(move || {
        if path.is_empty() {
            let mut entries = Vec::new();
            for c in b'C'..=b'Z' {
                let root = format!("{}:", c as char);
                if std::path::Path::new(&format!("{root}\\")).is_dir() {
                    entries.push(DirEntryOut { name: root, is_dir: true, size: 0, mtime: 0 });
                }
            }
            return Ok(DirListing { path: String::new(), parent: None, entries, sel: None });
        }
        let real = match path.as_str() {
            "@desktop" => known_dir(&windows::Win32::UI::Shell::FOLDERID_Desktop).ok_or("无法定位桌面目录")?,
            "@downloads" => downloads_dir().ok_or("无法定位下载目录")?,
            "@docs" => known_dir(&windows::Win32::UI::Shell::FOLDERID_Documents).ok_or("无法定位文档目录")?,
            _ => std::path::PathBuf::from(&path),
        };
        // 输入的是文件路径 → 返回其上级目录并预选该文件（地址栏直达文件）
        let (list_dir, sel) = if real.is_file() {
            let par = real.parent().ok_or("路径无上级目录")?.to_path_buf();
            (par, Some(real.to_string_lossy().to_string()))
        } else {
            (real.clone(), None)
        };
        let real = list_dir;
        if !real.is_dir() { return Err("目录不存在".to_string()); }
        let mut dirs: Vec<DirEntryOut> = Vec::new();
        let mut files: Vec<DirEntryOut> = Vec::new();
        if let Ok(rd) = std::fs::read_dir(&real) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                let Ok(ft) = e.file_type() else { continue };
                if ft.is_dir() {
                    if is_junk_dir(&name) { continue; }
                    dirs.push(DirEntryOut { name, is_dir: true, size: 0, mtime: 0 });
                } else {
                    let (sz, mt) = match e.metadata() {
                        Ok(m) => {
                            let mt = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64).unwrap_or(0);
                            (m.len(), mt)
                        }
                        Err(_) => (0, 0),
                    };
                    files.push(DirEntryOut { name, is_dir: false, size: sz, mtime: mt });
                }
            }
        }
        let key = |a: &DirEntryOut, b: &DirEntryOut| a.name.to_lowercase().cmp(&b.name.to_lowercase());
        dirs.sort_by(key);
        files.sort_by(key);
        let mut entries = dirs;
        entries.extend(files);
        // 父级：普通目录 → parent；盘根（如 C:\）→ ""（此电脑）
        let parent = real.parent().map(|x| x.to_string_lossy().to_string()).filter(|x| !x.is_empty()).or({
            let is_root = real.parent().map(|x| x.as_os_str().is_empty()).unwrap_or(true);
            is_root.then(|| String::new())
        });
        Ok(DirListing { path: real.to_string_lossy().to_string(), parent, entries, sel })
    })
    .await
    .map_err(|_| "目录任务失败".to_string())?
}

// ---------------------------- v0.9.11 聊天数据管理 ----------------------------

/// 目录大小/文件数统计（不跟随符号链接，浅层两级足够）
fn walk_usage(dir: &std::path::Path) -> (u64, u64) {
    let mut bytes = 0u64;
    let mut count = 0u64;
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                let (b, c) = walk_usage(&p);
                bytes += b;
                count += c;
            } else if let Ok(m) = e.metadata() {
                bytes += m.len();
                count += 1;
            }
        }
    }
    (bytes, count)
}

#[derive(serde::Serialize)]
pub struct DataUsage {
    pub msgs_bytes: u64,   // 聊天记录（含文件索引）
    pub files_bytes: u64,  // 接收文件落地区
    pub files_count: u64,
}

/// 聊天数据占用统计（设置面板"数据管理"）
#[tauri::command]
pub async fn social_data_usage() -> Result<DataUsage, String> {
    tauri::async_runtime::spawn_blocking(|| {
        let msgs = walk_usage(&crate::settings::social_dir().join("msgs"));
        let files = walk_usage(&recv_root().join("TexMantis"));
        Ok(DataUsage { msgs_bytes: msgs.0, files_bytes: files.0, files_count: files.1 })
    })
    .await
    .map_err(|_| "统计任务失败".to_string())?
}

/// 清除全部聊天记录（本机全部会话的消息与文件索引；对方不受影响）
#[tauri::command]
pub async fn social_clear_all_history() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(|| {
        let dir = crate::settings::social_dir().join("msgs");
        let mut n = 0usize;
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().map(|x| x == "jsonl").unwrap_or(false) {
                    if std::fs::remove_file(&p).is_ok() { n += 1; }
                }
            }
        }
        socdbg(&format!("cmd clear_all_history: 删除 {n} 个记录文件"));
        Ok(())
    })
    .await
    .map_err(|_| "清理任务失败".to_string())?
}

/// 清除全部接收的文件（托管落地区 recv\TexMantis 内的文件；自定义目录中聊天收到的文件按记录路径另行处理）
#[tauri::command]
pub async fn social_clear_recv_files() -> Result<(u64, u64), String> {
    tauri::async_runtime::spawn_blocking(|| {
        let landing = recv_root().join("TexMantis");
        let (bytes, count) = walk_usage(&landing);
        if landing.exists() {
            let _ = std::fs::remove_dir_all(&landing);
            let _ = std::fs::create_dir_all(&landing);
        }
        socdbg(&format!("cmd clear_recv_files: {} 个文件 / {} 字节", count, bytes));
        Ok((count, bytes))
    })
    .await
    .map_err(|_| "清理任务失败".to_string())?
}

/// 清除某会话"接收的文件"：记录中去掉接收的文件条目（保留文字与已发送），
/// 磁盘上属于托管落地区的文件同步删除；返回清除的条目数
#[tauri::command]
pub async fn social_clear_chat_files(key: String) -> Result<u32, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(f) = msg_file(&key) else { return Ok(0) };
        let Ok(txt) = std::fs::read_to_string(&f) else { return Ok(0) };
        let landing = recv_root().join("TexMantis");
        let mut kept: Vec<&str> = Vec::new();
        let mut n = 0u32;
        for line in txt.lines() {
            let ok = serde_json::from_str::<serde_json::Value>(line).ok();
            let is_recv_file = ok.as_ref()
                .map(|v| {
                    let file = v.get("file").and_then(|x| x.as_str()).unwrap_or("");
                    let d = v.get("d").and_then(|x| x.as_str()).unwrap_or("");
                    !file.is_empty() && d == "in"
                })
                .unwrap_or(false);
            if is_recv_file {
                if let Some(p) = ok.and_then(|v| v.get("path").and_then(|x| x.as_str()).map(String::from)) {
                    let pp = std::path::Path::new(&p);
                    if pp.starts_with(&landing) {
                        let _ = std::fs::remove_file(pp);
                    }
                }
                n += 1;
                continue;
            }
            kept.push(line);
        }
        if n > 0 {
            let out = if kept.is_empty() { String::new() } else { format!("{}
", kept.join("
")) };
            let _ = std::fs::write(&f, out);
        }
        Ok(n)
    })
    .await
    .map_err(|_| "清理任务失败".to_string())?
}

/// 一键清空本地聊天记录（好友 uid 或群键 group_{gid}）；只影响本机，对方不受影响
#[tauri::command]
pub async fn social_clear_history(key: String) -> Result<(), String> {
    let Some(f) = msg_file(&key) else { return Ok(()) };
    match std::fs::remove_file(&f) {
        Ok(_) => {
            socdbg(&format!("cmd clear_history {}", &key[..16.min(key.len())]));
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("无法清除记录: {e}")),
    }
}

/// 聊天记录（最近 1000 条；v0.9.0 支持记录查看器搜索）
#[tauri::command]
pub async fn social_history(uid: String) -> Result<Vec<HistItem>, String> {
    let _sp = SocSpan::new("cmd history");
    let Some(f) = msg_file(&uid) else { return Ok(vec![]) };
    // v0.9.22 只读尾部 1MB：长期运行历史文件增大后，开聊天不再全量读盘（首屏只需最近 1000 条）
    let Ok(meta) = std::fs::metadata(&f) else { return Ok(vec![]) };
    let tail = meta.len().min(1024 * 1024);
    let txt = if tail == meta.len() {
        std::fs::read_to_string(&f).unwrap_or_default()
    } else {
        use std::io::{Read, Seek, SeekFrom};
        let mut fh = match std::fs::File::open(&f) { Ok(x) => x, Err(_) => return Ok(vec![]) };
        if fh.seek(SeekFrom::End(-(tail as i64))).is_err() { return Ok(vec![]) }
        let mut buf = String::new();
        let _ = fh.take(tail).read_to_string(&mut buf);
        match buf.find('\n') { Some(i) => buf[i + 1..].to_string(), None => buf } // 掐掉半行
    };
    let mut items: Vec<HistItem> =
        txt.lines().filter_map(|l| serde_json::from_str::<HistItem>(l).ok()).collect();
    if items.len() > 1000 {
        items = items.split_off(items.len() - 1000);
    }
    Ok(items)
}

// ---------------------------- 昵称生成器 ----------------------------

// ---------------------------------------------------------------------------
// v0.9.27 随机昵称生成：QQ / Kakao 风格中文昵称 —— 短（2~8 字）、自然、可记忆。
// 设计：70% 从精选成品池直接抽取（性别加权），30% 由模板组合产出变化；
// 全部候选与局域网现有同伴去重。
// ---------------------------------------------------------------------------
const NICK_SHARED: &[&str] = &[
    // 食物饮品系
    "汽水不加冰", "半糖去冰", "芋泥波波", "芝士恐龙", "橘子汽水", "烤奶不加糖",
    "一口小笼包", "小熊软糖", "一颗小方糖", "奶芙小方", "桂花酒酿", "气泡水与猫",
    "焦糖玛奇朵", "美式去冰", "草莓芝士", "四月的春卷",
    // 动物系
    "软乎乎的猫", "打盹的柴犬", "摸鱼锦鲤", "困困兽", "爱睡的羊驼", "橘猫打盹",
    "小猫钓鱼", "机箱里的猫", "雾里有只鹿", "鲸鱼小岛",
    // 生活状态系
    "充电中", "刚睡醒", "假装在忙", "装在线", "今天不上班", "躺平小分队",
    "在线装忙", "退堂鼓一级选手", "晚睡选手", "早起失败", "三分钟热度",
    "热搜绝缘体", "别叫我起床", "今天也要好好吃饭", "路过人间", "不愿透露姓名",
    // 文艺短句系
    "凌晨四点半", "月亮邮递员", "晚风拾叁", "山茶与鹿", "躲在云朵里",
    "月球背面居民", "云朵贩卖机", "雾都客人", "夜航船", "末班车",
    "小溪流", "小阵雨", "山野来信", "拾光", "初雪未至",
];

const NICK_MALE: &[&str] = &[
    "代码搬运工", "bug制造机", "重启大法好", "网管小王", "硬件发烧友",
    "服务器看门人", "深夜修灯人", "显卡在燃烧", "像素级手残", "单排上分中",
    "蓝屏终结者", "半夜十二点上线", "草原孤狼", "山谷来客", "冷山", "北屿",
    "南巷旧人", "拾贰月的雪", "潜水中勿扰", "带不动就躺",
];

const NICK_FEMALE: &[&str] = &[
    "奶芙小圆子", "桃桃乌龙", "一颗草莓", "小熊不出门", "软妹元气弹",
    "云朵收藏家", "睡前故事", "月亮打烊了", "星河滚烫", "小满", "半夏",
    "青栀", "南音", "晚来天欲雪", "三色堇", "偷喝奶茶的猫", "爱看云的人",
    "春日限定", "软糖小熊", "橘子味的风",
];

const NICK_MYSTERY: &[&str] = &[
    "无名之辈", "过客", "旅人", "深海乘客", "信号中断", "匿名用户",
    "查无此人", "此人已失踪", "第七频道", "静音模式", "勿扰模式",
    "星际漫游中", "地下铁乘客", "平行时空的我", "守夜人", "无名山丘",
    "频率外的人", "隐身中", "黑箱", "未命名文件",
];

/// 组合模板词库（30% 概率走组合路径，保持 2~7 字的短名观感）
const NICK_SMALL: &[&str] = &[
    "橙子", "海盐", "气泡", "木屋", "鲸鱼", "板凳", "星球", "青椒", "桃子",
    "蘑菇", "汽水", "布丁", "云朵", "海豹", "栗子", "竹子", "灯泡", "北极星",
];
const NICK_DOING: &[&str] = &["摸鱼中", "充电中", "在线中", "飘走了", "起飞了", "到站了", "下线了", "睡着了", "发呆中", "划水中"];
const NICK_LOVE_A: &[&str] = &["爱吃橘子", "爱睡懒觉", "爱喝汽水", "爱看晚霞", "爱踩水坑", "爱数星星", "爱囤零食", "爱听雨声"];
const NICK_LOVE_B: &[&str] = &["的猫", "的熊", "的鱼", "的兔", "的人", "的企鹅", "的柯基", "的仓鼠"];

/// 与局域网同伴去重后取一个昵称
fn nick_pick(gender: &str, taken: &[String]) -> Option<String> {
    let mut buf = [0u8; 16];
    os_rand(&mut buf).ok()?;
    let byte = |i: usize| buf[i % 16] as usize;
    for attempt in 0..12 {
        let name = if byte(attempt) % 10 < 7 {
            // 70%：成品池（性别加权：男/女 60% 专属池 + 40% 共享池；其它 50/50 神秘池 + 共享池）
            let pool: &[&str] = match gender {
                "m" => if byte(attempt + 1) % 10 < 6 { NICK_MALE } else { NICK_SHARED },
                "f" => if byte(attempt + 1) % 10 < 6 { NICK_FEMALE } else { NICK_SHARED },
                _ => if byte(attempt + 1) % 10 < 5 { NICK_MYSTERY } else { NICK_SHARED },
            };
            pool[byte(attempt + 2) % pool.len()].to_string()
        } else {
            // 30%：模板组合
            match byte(attempt + 3) % 3 {
                0 => format!("小{}", NICK_SMALL[byte(attempt + 4) % NICK_SMALL.len()]),
                1 => NICK_DOING[byte(attempt + 5) % NICK_DOING.len()].to_string(),
                _ => format!("{}{}", NICK_LOVE_A[byte(attempt + 6) % NICK_LOVE_A.len()], NICK_LOVE_B[byte(attempt + 7) % NICK_LOVE_B.len()]),
            }
        };
        if !taken.iter().any(|t| t == &name) {
            return Some(name);
        }
    }
    None
}

/// 生成随机昵称（QQ/Kakao 风格中文短昵称），并与当前局域网同伴去重
#[tauri::command]
pub async fn social_gen_name(gender: String) -> Result<String, String> {
    let _sp = SocSpan::new("cmd gen_name");
    let taken: Vec<String> = PEERS.lock().unwrap_or_else(|e| e.into_inner()).values().map(|(p, _, _)| p.nickname.clone()).collect();
    if let Some(n) = nick_pick(&gender, &taken) {
        return Ok(n);
    }
    // 极端去重撞满兜底：短名 + 2 位随机十六进制
    let mut buf = [0u8; 4];
    os_rand(&mut buf)?;
    let hx = hex_enc(&buf);
    Ok(format!("路人{}", &hx[..2].to_uppercase()))
}

/// 下线宣告（退出前调用；进程退出亦等效）
pub fn announce_bye() {
    if let Some(udp) = UDP_SOCK.lock().unwrap_or_else(|e| e.into_inner()).clone() {
        let msg = UdpMsg { t: D_BYE, sid: String::new(), p: my_profile() };
        if let Ok(bin) = serde_json::to_vec(&msg) {
            for target in broadcast_targets(&udp) {
                let _ = udp.send_to(&bin, target);
            }
        }
    }
}

// ===========================================================================
// v0.9.0 文件传输（断点续传 + 大缓冲提速）/ 群聊 / 广播 / 好友备注
//
// 设计要点（延续 v0.6.0 轻量骨架）：
// · 全部复用既有加密会话（单端口单连接，帧级多路复用）——聊天与文件块在帧层
//   交织，互不阻塞；绝不为传输新开端口，不触碰 UDP 广播
// · 提速（「黑科技」的诚实解释）：256KiB 大块 AEAD 帧 + 4MiB 收发套接字缓冲 +
//   顺序落盘（8MiB 才 flush 一次）+ 有界通道天然背压流控 → 千兆局域网可跑满线速
// · 断点续传：文件 id = SHA256(完整路径+大小+mtime) 前 16 hex（跨重启稳定）；
//   接收端以「已落盘字节数」为唯一事实，ACK 携带偏移，发送端 seek 后续传
// · 群聊：创建者权威名单（群主/管理员改名单 → GroupSync 逐人同步）；消息由
//   发送者对每个成员逐人加密单播（无转发风暴，防嗅探聚合）；安全校验一律以
//   会话握手 uid 为准，绝不信任帧内自报身份
// ===========================================================================

use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicU8;

const CHUNK_SZ: usize = 256 * 1024;
const SOCK_BUF: i32 = 4 * 1024 * 1024;

#[derive(Serialize, Deserialize, Clone)]
struct FileOffer { id: String, name: String, size: u64, gid: String }
#[derive(Serialize, Deserialize, Clone)]
struct FileAck { id: String, ok: bool, offset: u64 }
#[derive(Serialize, Deserialize, Clone)]
struct FileCtrl { id: String, action: String } // pause / resume / cancel
#[derive(Serialize, Deserialize, Clone)]
struct FileEnd { id: String, ok: bool }
#[derive(Serialize, Deserialize, Clone)]
struct GroupText { gid: String, from: String, name: String, text: String, ts: u64 }
#[derive(Serialize, Deserialize, Clone)]
struct GroupNudge { gid: String, from: String, name: String }
#[derive(Serialize, Deserialize, Clone)]
struct PeerShake { from: String, name: String }
#[derive(Serialize, Deserialize, Clone)]
struct ReadUpTo { up_to: u64 }
#[derive(Serialize, Deserialize, Clone)]
struct GroupLeave { gid: String, from: String }

/// 群档案（创建者权威；成员本地持久化镜像）
#[derive(Serialize, Deserialize, Clone)]
pub struct GroupInfo {
    pub gid: String,
    pub name: String,
    pub owner: String,
    pub admins: Vec<String>,
    pub members: Vec<GroupMember>,
    #[serde(default)]
    pub dissolve: bool, // 同步帧语义：true = 解散
}

#[derive(Serialize, Deserialize, Clone)]
pub struct GroupMember { pub uid: String, pub nickname: String, pub avatar: u8 }

/// 传输条目快照（前端渲染进度条）
#[derive(Serialize, Clone)]
pub struct XferOut {
    pub id: String,
    pub dir: String, // out / in
    pub peer: String,
    pub gid: String,
    pub name: String,
    pub size: u64,
    pub done: u64,
    pub st: u8, // 0 run 1 pause 2 cancel 3 done 4 fail 5 offered(等待对方/本机确认)
}

struct Xfer {
    fid: String, // v0.9.12 线上传输 id（发送端含目的地；接收端 = 对端 offer.id）
    dir: &'static str,
    peer: String,
    gid: String,
    name: String,
    size: u64,
    path: std::path::PathBuf,
    done: AtomicU64,
    st: AtomicU8,
    t_end: AtomicU64, // 终态(2/3/4)时刻 ms —— 快照回收历史终态用
    tx_in: Mutex<Option<std::sync::mpsc::SyncSender<Vec<u8>>>>, // 接收端：块数据通道（有界=背压）
}

static XFERS: LazyLock<Mutex<HashMap<String, Arc<Xfer>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static PENDING_ACK: LazyLock<Mutex<HashMap<String, Arc<Xfer>>>> = LazyLock::new(|| Mutex::new(HashMap::new())); // v0.9.12：待对方回应的发送（事件驱动，无超时）
static PENDING_IN: LazyLock<Mutex<HashMap<String, (FileOffer, String)>>> = LazyLock::new(|| Mutex::new(HashMap::new())); // id → (offer, 发送者uid)
static PICKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false); // 文件/目录对话框单例守卫
static GROUPS: LazyLock<Mutex<HashMap<String, GroupInfo>>> = LazyLock::new(|| Mutex::new(HashMap::new()));
static LAST_SOUND: AtomicU64 = std::sync::atomic::AtomicU64::new(0);

// v0.9.2 在线保活与气泡判定
static LAST_PAGE_POLL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0); // 社交页最近一次轮询
static SOCIAL_OPENED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false); // 本会话是否开过社交页
static BUBBLE_LAST: LazyLock<Mutex<HashMap<String, u64>>> = LazyLock::new(|| Mutex::new(HashMap::new())); // 气泡节流
static PEERS_DIRTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false); // 联系人落盘脏标记
static LAST_PEERS_SAVE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct PersistPeer {
    p: Profile,
    addr: String,
    seen: u64,
}

fn peers_file() -> Option<std::path::PathBuf> {
    Some(crate::settings::social_dir().join("peers.json"))
}

/// 联系人落盘（历史联系人跨重启展示；脏标记 + 10 秒节流，由保活线程顺带驱动）
fn save_peers_if_dirty() {
    use std::sync::atomic::Ordering;
    if !PEERS_DIRTY.load(Ordering::SeqCst) { return; }
    let now = now_ms();
    if now.saturating_sub(LAST_PEERS_SAVE.load(Ordering::Relaxed)) < 10_000 { return; }
    LAST_PEERS_SAVE.store(now, Ordering::Relaxed);
    PEERS_DIRTY.store(false, Ordering::SeqCst);
    let removed: Vec<String> = crate::settings::config().social_removed.clone();
    let list: Vec<PersistPeer> = {
        let g = PEERS.lock().unwrap_or_else(|e| e.into_inner());
        let mut all: Vec<(&String, &(Profile, String, u64))> = g.iter().collect();
        all.sort_by_key(|(_, (_, _, seen))| std::cmp::Reverse(*seen));
        all.into_iter()
            .filter(|(uid, _)| !removed.contains(*uid))
            .take(200)
            .map(|(uid, (p, addr, seen))| PersistPeer { p: (*p).clone(), addr: (*addr).clone(), seen: *seen })
            .collect()
    };
    if let Some(f) = peers_file() {
        if let Ok(txt) = serde_json::to_string(&list) {
            let _ = std::fs::create_dir_all(f.parent().unwrap_or(std::path::Path::new(".")));
            let _ = std::fs::write(&f, txt);
        }
    }
}

fn mark_peers_dirty() {
    PEERS_DIRTY.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// 启动加载历史联系人（离线灰显；一级心跳稍后会主动探测他们）
fn load_peers() {
    let Some(f) = peers_file() else { return };
    let Ok(txt) = std::fs::read_to_string(&f) else { return };
    let Ok(list) = serde_json::from_str::<Vec<PersistPeer>>(&txt) else { return };
    let removed: Vec<String> = crate::settings::config().social_removed.clone();
    let mut n = 0;
    {
        let mut g = PEERS.lock().unwrap_or_else(|e| e.into_inner());
        for pp in list {
            if pp.p.uid.is_empty() || removed.contains(&pp.p.uid) { continue; }
            g.insert(pp.p.uid.clone(), (pp.p, pp.addr, pp.seen));
            n += 1;
        }
    }
    if n > 0 {
        socdbg(&format!("peers: 已恢复 {n} 位历史联系人（peers.json）"));
        mark_peers_dirty();
        zombie_sweep("load");
    }
}

/// 僵尸号清理：同一台机器（mid 相同）出现多个 uid = 重装残留。
/// 保留最近活跃者；僵尸的备注迁移给保留者（同机，备注本就属于那台机器）后剔除僵尸。
/// 锁序纪律：绝不跨 update_config 持有 PEERS 锁。
fn zombie_sweep(_why: &str) {
    if !crate::settings::config().social_zombie { return; }
    // 1) 只读扫描：找出每个 mid 的保留者与僵尸
    let (zombie_keep, zombies): (Vec<(String, String)>, Vec<String>) = {
        let g = PEERS.lock().unwrap_or_else(|e| e.into_inner());
        let mut best: HashMap<&str, (&String, u64)> = HashMap::new(); // mid → (uid, seen)
        for (uid, (p, _, seen)) in g.iter() {
            if p.mid.is_empty() { continue; }
            match best.get(p.mid.as_str()) {
                Some((_, s)) if *s >= *seen => {}
                _ => { best.insert(p.mid.as_str(), (uid, *seen)); }
            }
        }
        let mut zk = Vec::new();
        let mut zs = Vec::new();
        for (uid, (p, _, _)) in g.iter() {
            if p.mid.is_empty() { continue; }
            match best.get(p.mid.as_str()) {
                Some((k, _)) if k != &uid => { zs.push(uid.clone()); zk.push((uid.clone(), (*k).clone())); }
                _ => {}
            }
        }
        (zk, zs)
    };
    if zombies.is_empty() { return; }
    // 2) 备注迁移 + 僵尸备注清理（不持 PEERS 锁）
    crate::settings::update_config(|c| {
        for (z, k) in &zombie_keep {
            if let Some(r) = c.social_remarks.remove(z) {
                c.social_remarks.entry(k.clone()).or_insert(r);
            }
        }
    });
    // 3) 剔除僵尸 + 落盘收紧
    {
        let mut g = PEERS.lock().unwrap_or_else(|e| e.into_inner());
        for z in &zombies {
            if g.remove(z).is_some() {
                socdbg(&format!("peers: 僵尸号清理 {}（同机新身份已存在）", &z[..8.min(z.len())]));
            }
        }
    }
    mark_peers_dirty();
}

/// 本会话内用户打开过社交页（托盘退出确认 / 关窗驻留的判据）
pub fn opened() -> bool {
    SOCIAL_OPENED.load(std::sync::atomic::Ordering::SeqCst)
}

/// 「下载」文件夹（SHGetKnownFolderPath；失败回退 data\social\files）
pub fn known_dir(guid: &windows::core::GUID) -> Option<std::path::PathBuf> {
    #[cfg(windows)]
    {
        use windows::Win32::System::Com::CoTaskMemFree;
        use windows::Win32::UI::Shell::SHGetKnownFolderPath;
        unsafe {
            let path = SHGetKnownFolderPath(guid, windows::Win32::UI::Shell::KNOWN_FOLDER_FLAG(0), None).ok()?;
            let s = path.to_string().ok();
            CoTaskMemFree(Some(path.as_ptr() as *const core::ffi::c_void));
            s.map(std::path::PathBuf::from)
        }
    }
    #[cfg(not(windows))]
    {
        let _ = guid;
        None
    }
}

pub fn downloads_dir() -> Option<std::path::PathBuf> {
    known_dir(&windows::Win32::UI::Shell::FOLDERID_Downloads)
}

/// 接收根目录：用户自定义（存在才生效）→ 下载\TexMantis → data\social\files
fn recv_root() -> std::path::PathBuf {
    let custom = crate::settings::config().social_recv_dir.trim().to_string();
    if !custom.is_empty() {
        let p = std::path::PathBuf::from(&custom);
        if p.is_dir() {
            return p;
        }
    }
    downloads_dir().unwrap_or_else(|| crate::settings::social_dir().join("files"))
}

fn xst(x: &Xfer) -> u8 { x.st.load(Ordering::SeqCst) }
fn set_st(x: &Xfer, v: u8) {
    x.st.store(v, Ordering::SeqCst);
    if v == 2 || v == 3 || v == 4 {
        x.t_end.store(now_ms(), Ordering::SeqCst);
    }
}

fn emit_ev(evt: &str, v: serde_json::Value) {
    if let Some(app) = crate::settings::APP.get() {
        use tauri::Emitter;
        let _ = app.emit(evt, v);
    }
}

fn xfer_out(x: &Xfer) -> XferOut {
    XferOut {
        id: x.id(), dir: x.dir.to_string(), peer: x.peer.clone(), gid: x.gid.clone(),
        name: x.name.clone(), size: x.size, done: x.done.load(Ordering::SeqCst), st: xst(x),
    }
}

impl Xfer {
    fn id(&self) -> String { self.fid.clone() }
}

/// v0.9.12：发送端 id 混入目的地 —— 同一文件发给不同的人/群互不冲突（修复群发 id 覆盖），
/// id 对接收端不透明（协议向后兼容：id 随 OFFER 原样传输）
fn xf_id_scoped(p: &std::path::Path, size: u64, scope: &str) -> String {
    // v0.9.20 修复：id 必须是 16 hex 字符（块帧 [id16] 契约）。此前取 d[..16] 字节
    // hex 出 32 字符 —— 发送端块帧写 32 字节 id，接收端只解析前 16 字符查表，
    // 永远查不到 → 每一块被静默丢弃 → "完成"但 0KB 且无文件记录。
    let mut h = Sha256::new();
    h.update(p.to_string_lossy().as_bytes());
    h.update(size.to_le_bytes());
    h.update(scope.as_bytes());
    let d = h.finalize();
    d[..8].iter().map(|b| format!("{b:02x}")).collect::<String>()
}

/// 稳定文件 id：SHA256(完整路径+大小+mtime) 前 16 hex —— 跨重启可续传
fn xf_id_of(path: &std::path::Path, size: u64) -> String {
    let mut h = Sha256::new();
    h.update(path.to_string_lossy().as_bytes());
    h.update(size.to_le_bytes());
    if let Ok(m) = std::fs::metadata(path) {
        if let Ok(t) = m.modified() {
            if let Ok(d) = t.duration_since(UNIX_EPOCH) { h.update(d.as_nanos().to_le_bytes()); }
        }
    }
    hex_enc(&h.finalize()[..8])
}

/// 文件落地目录：好友 = files/{对端uid8}/；群 = files/group_{gid}/
fn xf_landing(peer: &str, gid: &str) -> std::path::PathBuf {
    // v0.9.2：落在用户配置的接收根目录（默认 下载\TexMantis）；旧位置的部分文件续传不受影响
    let base = if gid.is_empty() {
        recv_root().join("TexMantis").join(&peer[..8.min(peer.len())])
    } else {
        recv_root().join("TexMantis").join(format!("group_{gid}"))
    };
    let _ = std::fs::create_dir_all(&base);
    base
}

/// 同名去重：name.ext → name(1).ext …（接收端防覆盖）
fn dedup_name(dir: &std::path::Path, name: &str) -> String {
    if !dir.join(name).exists() { return name.to_string(); }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (name.to_string(), String::new()),
    };
    for i in 1..999u32 {
        let cand = format!("{stem}({i}){ext}");
        if !dir.join(&cand).exists() { return cand; }
    }
    format!("{stem}_{}{ext}", hex_enc(&os_seed8()))
}

fn os_seed8() -> [u8; 8] {
    let mut b = [0u8; 8];
    let _ = os_rand(&mut b);
    b
}

/// 取/建与某成员的加密会话，返回会话流 + 密钥（所有群/文件/文字发送共用）
fn sess_get_or_open(uid: &str) -> Result<(Arc<Mutex<TcpStream>>, [u8; 32]), String> {
    {
        let g = SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = g.get(uid) {
            return Ok((s.stream.clone(), s.key));
        }
    }
    open_session(uid)?;
    let g = SESSIONS.lock().unwrap_or_else(|e| e.into_inner());
    g.get(uid).map(|s| (s.stream.clone(), s.key)).ok_or_else(|| "会话建立失败".to_string())
}

/// 会话帧发送：拿流 → 加密写 → 释放（绝不持锁跨等待）
fn sess_send(uid: &str, t: u8, payload: &[u8]) -> Result<(), String> {
    let (stream, key) = sess_get_or_open(uid)?;
    let mut s = stream.lock().unwrap_or_else(|e| e.into_inner());
    frame_send(&mut s, &key, t, payload).map_err(|e| format!("发送失败（对方可能已离线）: {e}"))
}

// ---------------------------- 文件传输 ----------------------------

/// 系统文件选择器（rfd 原生对话框；Web 端 input 拿不到真实路径）
#[tauri::command]
pub async fn social_pick_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use std::sync::atomic::Ordering;
    // v0.9.11：已有对话框在打开时直接返回（防连点叠窗）；对话框挂父窗口，前置显示不藏到主窗后面
    if PICKING.swap(true, Ordering::SeqCst) { return Ok(None); }
    let r = tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        let mut d = rfd::FileDialog::new();
        if let Some(w) = app.get_webview_window("main") { d = d.set_parent(&w); }
        let r = d.pick_file().map(|p| p.to_string_lossy().to_string());
        PICKING.store(false, Ordering::SeqCst);
        r
    })
    .await
    .map_err(|e| { PICKING.store(false, Ordering::SeqCst); format!("对话框任务失败: {e}") })?;
    Ok(r)
}

/// 发送文件（好友 or 群文件逐人发送的第一跳）：发 OFFER → 等 ACK（含续传偏移）→ 起发送线程
#[tauri::command]
pub async fn social_send_file(uid: String, path: String, gid: Option<String>) -> Result<XferOut, String> {
    let (uid2, path2, gid2) = (uid.clone(), path.clone(), gid.clone().unwrap_or_default());
    tauri::async_runtime::spawn_blocking(move || send_file_impl(&uid2, &path2, &gid2))
        .await
        .map_err(|_| "文件任务失败".to_string())?
}

fn send_file_impl(uid: &str, path: &str, gid: &str) -> Result<XferOut, String> {
    let p = std::path::PathBuf::from(path);
    let meta = std::fs::metadata(&p).map_err(|_| "文件不存在或不可读".to_string())?;
    if !meta.is_file() { return Err("请选择文件（暂不支持文件夹）".into()); }
    let size = meta.len();
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    // v0.9.12：id 混入目的地 —— 同文件发给不同人/群互不冲突（修复群发 id 覆盖的历史缺陷）
    let scope = if gid.is_empty() { uid.to_string() } else { format!("g:{gid}") };
    let id = xf_id_scoped(&p, size, &scope);
    // 已有同 id 传输在跑/暂停 → 直接复用（重复点击幂等）
    {
        let g = XFERS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(x) = g.get(&id) {
            let s = xst(x);
            if s == 0 || s == 5 { return Ok(xfer_out(x)); } // 进行中/等待对方接收：幂等返回
            if s == 1 {
                set_st(x, 0); // 暂停中：直接恢复发送
                emit_final(x);
                return Ok(xfer_out(x));
            }
        }
    }
    let xfer = Arc::new(Xfer {
        fid: id.clone(), dir: "out", peer: uid.to_string(), gid: gid.to_string(), name, size,
        path: p, done: AtomicU64::new(0), st: AtomicU8::new(5), t_end: AtomicU64::new(0),
        tx_in: Mutex::new(None),
    });
    XFERS.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), xfer.clone());
    // 发送 OFFER（会话不存在则现场建立；3s 连接 + 3s 握手超时保护）
    let (stream, key) = sess_get_or_open(uid).map_err(|e| {
        set_st(&xfer, 4);
        e
    })?;
    let offer = FileOffer { id: id.clone(), name: xfer.name.clone(), size, gid: gid.to_string() };
    {
        let mut s = stream.lock().unwrap_or_else(|e| e.into_inner());
        frame_send(&mut s, &key, T_FILE_OFFER, &serde_json::to_vec(&offer).unwrap_or_default())
            .map_err(|e| { set_st(&xfer, 4); format!("发送文件邀请失败: {e}") })?;
    }
    // v0.9.12 Telegram 式邀请：发出即生效、永不过期（对方随时决定）。
    // 注册待确认表，ACK 到达时由 route_ack 事件驱动启动发送线程 —— 不再阻塞 30 秒、
    // 不再超时报"邀请已过期"；对方未回应期间本机可随时取消
    PENDING_ACK.lock().unwrap_or_else(|e| e.into_inner()).insert(id.clone(), xfer.clone());
    emit_final(&xfer); // 前端立即出现"等待对方接收"卡片
    Ok(xfer_out(&xfer))
}

/// 发送线程：顺序读文件 → 256KiB AEAD 帧 → 随时响应 暂停/续传/取消
fn send_file_worker(xfer: Arc<Xfer>, stream: Arc<Mutex<TcpStream>>, key: [u8; 32], mut offset: u64) {
    let id = xfer.id();
    let mut seq = (offset / CHUNK_SZ as u64) as u32;
    let run = |x: &Xfer| xst(x) == 0;
    let mut file = match std::fs::File::open(&xfer.path) {
        Ok(f) => f,
        Err(_) => { set_st(&xfer, 4); return; }
    };
    use std::io::Seek;
    if std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(offset)).is_err() {
        set_st(&xfer, 4);
        return;
    }
    let mut reader = std::io::BufReader::with_capacity(1024 * 1024, file);
    let mut buf = vec![0u8; CHUNK_SZ];
    let mut since_emit = 0u64;
    loop {
        // 控制状态机：取消 / 暂停
        match xst(&xfer) {
            2 | 4 => {
                let _ = sess_send(&xfer.peer, T_FILE_END, &serde_json::to_vec(&FileEnd { id: id.clone(), ok: false }).unwrap_or_default());
                emit_final(&xfer);
                return;
            }
            1 => { std::thread::sleep(Duration::from_millis(250)); continue; }
            _ => {}
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => {
                // 文件读完 → 传输完成
                set_st(&xfer, 3);
                let _ = sess_send(&xfer.peer, T_FILE_END, &serde_json::to_vec(&FileEnd { id: id.clone(), ok: true }).unwrap_or_default());
                persist_file_record(&xfer, "out");
                emit_final(&xfer);
                socdbg(&format!("xfer: 发送完成 id={id} {} 字节", xfer.done.load(Ordering::SeqCst)));
                return;
            }
            Ok(n) => n,
            Err(_) => { set_st(&xfer, 4); emit_final(&xfer); return; }
        };
        let mut payload = Vec::with_capacity(20 + n);
        payload.extend_from_slice(id.as_bytes());
        payload.extend_from_slice(&seq.to_be_bytes());
        payload.extend_from_slice(&buf[..n]);
        if let Err(e) = sess_send(&xfer.peer, T_FILE_CHUNK, &payload) {
            socdbg(&format!("xfer: 发送块失败 id={id} err={e}"));
            set_st(&xfer, 4);
            emit_final(&xfer);
            return;
        }
        // 对端确认结束（先到 END 的竞态安全）：收到取消/失败立即停
        if xst(&xfer) == 2 || xst(&xfer) == 4 {
            emit_final(&xfer);
            return;
        }
        offset += n as u64;
        xfer.done.store(offset, Ordering::SeqCst);
        seq = seq.wrapping_add(1);
        since_emit += n as u64;
        if since_emit >= 8 * 1024 * 1024 {
            since_emit = 0;
            emit_ev("social-xfer", serde_json::to_value(xfer_out(&xfer)).unwrap_or_default());
        }
        let _ = run; // 状态机上方已处理
    }
}

/// 接收线程：块通道（有界）顺序落盘；暂停期间只空转；取消删部分文件
fn recv_file_worker(xfer: Arc<Xfer>, rx: std::sync::mpsc::Receiver<Vec<u8>>) {
    use std::io::{Seek, SeekFrom, Write as _};
    // v0.9.19 定位写替代 append：落盘长度严格等于已接收偏移（set_len 对齐），
    // 杜绝"缓冲未落盘 → 续传错位"与过期长文件造成的尾部错位
    let mut file = match std::fs::OpenOptions::new().create(true).write(true).open(&xfer.path) {
        Ok(f) => f,
        Err(_) => { set_st(&xfer, 4); emit_final(&xfer); return; }
    };
    let offset = xfer.done.load(Ordering::SeqCst);
    if file.set_len(offset).is_err() || file.seek(SeekFrom::Start(offset)).is_err() {
        set_st(&xfer, 4); emit_final(&xfer); return;
    }
    let mut writer = std::io::BufWriter::with_capacity(8 * 1024 * 1024, file);
    let mut since_flush = 0u64;
    let mut since_emit = 0u64;
    loop {
        match xst(&xfer) {
            2 => {
                let _ = writer.flush();
                let _ = std::fs::remove_file(&xfer.path); // 取消 = 删除部分文件
                emit_final(&xfer);
                return;
            }
            4 => { let _ = writer.flush(); emit_final(&xfer); return; } // v0.9.19 退出必 flush，缓冲不丢
            _ => {}
        }
        let timeout = if xst(&xfer) == 1 { Duration::from_secs(1) } else { Duration::from_secs(60) };
        match rx.recv_timeout(timeout) {
            Ok(data) => {
                // v0.9.22 尾块截断防御：done 不得越过 size（重复块/续传边界绝不污染文件尾）
                let mut data = data;
                let cur = xfer.done.load(Ordering::SeqCst);
                if cur >= xfer.size { continue; }
                if cur + data.len() as u64 > xfer.size { data.truncate((xfer.size - cur) as usize); }
                if writer.write_all(&data).is_err() {
                    set_st(&xfer, 4); emit_final(&xfer); return;
                }
                since_flush += data.len() as u64;
                xfer.done.fetch_add(data.len() as u64, Ordering::SeqCst);
                since_emit += data.len() as u64;
                if since_flush >= 8 * 1024 * 1024 {
                    use std::io::Write as _;
                    let _ = writer.flush();
                    since_flush = 0;
                }
                if since_emit >= 8 * 1024 * 1024 {
                    since_emit = 0;
                    emit_ev("social-xfer", serde_json::to_value(xfer_out(&xfer)).unwrap_or_default());
                }
                if xfer.done.load(Ordering::SeqCst) >= xfer.size {
                    use std::io::Write as _;
                    let _ = writer.flush();
                    set_st(&xfer, 3);
                    persist_file_record(&xfer, "in");
                    emit_final(&xfer);
                    socdbg(&format!("xfer: 接收完成 id={} {} 字节", xfer.id(), xfer.done.load(Ordering::SeqCst)));
                    return;
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if xst(&xfer) == 1 { continue; } // 暂停中，继续等
                socdbg("xfer: 接收超时（60 秒无数据）");
                let _ = writer.flush(); // v0.9.19 退出必 flush：已收字节全部落盘，续传偏移才真实
                set_st(&xfer, 4);
                emit_final(&xfer);
                return;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // 发送方会话结束/清理了通道 → 视作中断（已取消/已完成不降级）
                let _ = writer.flush(); // v0.9.19 退出必 flush
                let st = xst(&xfer);
                if st != 3 && st != 2 { set_st(&xfer, 4); emit_final(&xfer); }
                return;
            }
        }
    }
}

/// 传输落库：聊天记录里追加文件条目（记录器可在「记录」里搜到文件名）
fn persist_file_record(x: &Xfer, dir: &str) {
    let key = if x.gid.is_empty() { x.peer.clone() } else { format!("group_{}", x.gid) };
    // v0.9.17 幂等护栏收窄为 1.5 秒且只拦"写库"：真正的事件始终广播。
    // （旧实现 10 秒内同文件重发会整单跳过 —— 事件一并丢失，接收方聊天流永远不出文件卡）
    let ts = now_ms();
    let mut dup = false;
    if let Some(f) = msg_file(&key) {
        if let Ok(txt) = std::fs::read_to_string(&f) {
            if let Some(last) = txt.lines().rev().find(|l| !l.trim().is_empty()) {
                if let Ok(prev) = serde_json::from_str::<HistItem>(last) {
                    if prev.file == x.name && prev.size == x.size && ts.saturating_sub(prev.ts) < 1_500 {
                        dup = true;
                    }
                }
            }
        }
    }
    if !dup {
        let item = HistItem {
            d: dir.to_string(), ts, text: String::new(),
            file: x.name.clone(), size: x.size, path: x.path.to_string_lossy().to_string(),
            from: String::new(), from_name: String::new(),
        };
        if let Some(f) = msg_file(&key) {
            if let Ok(mut fh) = std::fs::OpenOptions::new().create(true).append(true).open(&f) {
                let _ = writeln!(fh, "{}", serde_json::to_string(&item).unwrap_or_default());
            }
        }
    }
    // v0.9.13：完成即广播 —— 前端聊天流实时出现文件卡（不再等重开会话）
    emit_ev("social-file-record", serde_json::json!({
        "key": key, "d": dir, "ts": ts, "file": x.name, "size": x.size,
        "path": x.path.to_string_lossy(), "gid": x.gid, "peer": x.peer
    }));
    if dir == "in" {
        social_bubble_notify(&key, "📎 文件已接收", &x.name, &key);
    }
}

fn emit_final(x: &Xfer) {
    emit_ev("social-xfer", serde_json::to_value(xfer_out(x)).unwrap_or_default());
}

/// session_loop 路由：收到文件邀请（入站）
fn handle_file_offer(from_uid: &str, payload: &[u8]) {
    let Ok(offer) = serde_json::from_slice::<FileOffer>(payload) else { return };
    if offer.name.len() > 240 || offer.size > 8 * 1024 * 1024 * 1024u64 { return; }
    // 断点续传：同 id 已有接收条目（暂停/失败）→ 偏移 = 已落盘字节数
    let existing = {
        let g = XFERS.lock().unwrap_or_else(|e| e.into_inner());
        g.get(&offer.id).filter(|x| x.dir == "in" && (xst(x) == 1 || xst(x) == 4)).map(|x| (xfer_out(x), x.path.clone()))
    };
    let dir = xf_landing(from_uid, &offer.gid);
    let (path, offset, st) = match existing {
        Some((x, p)) => (p, x.done, 1u8),
        None => {
            let p = dir.join(dedup_name(&dir, &offer.name));
            (p, 0, 5u8)
        }
    };
    let xfer = Arc::new(Xfer {
        fid: offer.id.clone(), dir: "in", peer: from_uid.to_string(), gid: offer.gid.clone(), name: offer.name.clone(),
        size: offer.size, path: path.clone(), done: AtomicU64::new(offset), st: AtomicU8::new(st),
        t_end: AtomicU64::new(0), tx_in: Mutex::new(None),
    });
    XFERS.lock().unwrap_or_else(|e| e.into_inner()).insert(offer.id.clone(), xfer);
    PENDING_IN.lock().unwrap_or_else(|e| e.into_inner()).insert(offer.id.clone(), (offer.clone(), from_uid.to_string()));
    let from_name = PEERS.lock().unwrap_or_else(|e| e.into_inner()).get(from_uid).map(|(p, _, _)| p.nickname.clone()).unwrap_or_default();
    emit_ev("social-file-offer", serde_json::json!({
        "id": offer.id, "from": from_uid, "from_name": from_name, "name": offer.name,
        "size": offer.size, "gid": offer.gid, "offset": offset
    }));
    socdbg(&format!("xfer: 收到文件邀请 id={} {} 字节（已存 {} 字节，可续传）", offer.id, offer.size, offset));
}

/// 接受文件（前端按钮）→ 回 ACK（携带续传偏移）→ 起接收线程
/// v0.9.11 修复"点了没反应、再点说过期"：改为先探测邀请、连接成功才移除 ——
/// 连接失败（最长 3s）时邀请保留可重试，绝不先删后连；全程 spawn_blocking 不阻塞运行时
#[tauri::command]
pub async fn social_file_accept(id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || file_accept_impl(id))
        .await
        .map_err(|_| "文件任务失败".to_string())?
}

fn file_accept_impl(id: String) -> Result<(), String> {
    let from = {
        let g = PENDING_IN.lock().unwrap_or_else(|e| e.into_inner());
        g.get(&id).map(|(_, f)| f.clone()).ok_or("邀请已过期")?
    };
    let xfer = XFERS.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned().ok_or("传输不存在")?;
    let offset = xfer.done.load(Ordering::SeqCst);
    // 会话可能已断：重建（open_session 有 3s 超时保护；失败时邀请仍在，可再点）
    let (stream, key) = sess_get_or_open(&from)?;
    // v0.9.19 接收就绪先行：ACK 是"开传"信号，块通道与运行态必须先于 ACK 就位 ——
    // 否则发送端首块可能早于通道建立到达而被 route_chunk 静默丢弃（st==5 / tx_in=None）→ 0KB 文件
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(64); // 有界 → 天然背压
    *xfer.tx_in.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    set_st(&xfer, 0);
    let ack = {
        let mut s = stream.lock().unwrap_or_else(|e| e.into_inner());
        frame_send(&mut s, &key, T_FILE_ACK, &serde_json::to_vec(&FileAck { id: id.clone(), ok: true, offset }).unwrap_or_default())
    };
    if let Err(e) = ack {
        // ACK 失败：回滚接收就绪态，邀请保留可重试（连接失败最长 3s）
        *xfer.tx_in.lock().unwrap_or_else(|e| e.into_inner()) = None;
        set_st(&xfer, 5);
        return Err(format!("接受失败: {e}"));
    }
    PENDING_IN.lock().unwrap_or_else(|e| e.into_inner()).remove(&id); // 成功才移除
    std::thread::spawn(move || recv_file_worker(xfer, rx));
    Ok(())
}

/// 拒绝文件（前端按钮）
#[tauri::command]
pub async fn social_file_reject(id: String) -> Result<(), String> {
    let (_, from) = PENDING_IN.lock().unwrap_or_else(|e| e.into_inner()).remove(&id).ok_or("邀请已过期")?;
    if let Some(x) = XFERS.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned() {
        if xst(&x) == 5 { set_st(&x, 2); XFERS.lock().unwrap_or_else(|e| e.into_inner()).remove(&id); }
    }
    let _ = sess_send(&from, T_FILE_ACK, &serde_json::to_vec(&FileAck { id, ok: false, offset: 0 }).unwrap_or_default());
    Ok(())
}

/// 暂停 / 续传 / 取消（发起方=本机 → 直接置位；对端发起 → CTRL 帧通知）
#[tauri::command]
pub async fn social_file_pause(id: String) -> Result<(), String> {
    let x = XFERS.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned().ok_or("传输不存在")?;
    set_st(&x, 1);
    if x.dir == "in" {
        let _ = sess_send(&x.peer, T_FILE_CTRL, &serde_json::to_vec(&FileCtrl { id, action: "pause".into() }).unwrap_or_default());
    }
    emit_final(&x);
    Ok(())
}

#[tauri::command]
pub async fn social_file_resume(id: String) -> Result<(), String> {
    let x = XFERS.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned().ok_or("传输不存在")?;
    match x.dir {
        "out" => set_st(&x, 0), // 发送方自续传：状态机直接恢复
        _ => {
            let _ = sess_send(&x.peer, T_FILE_CTRL, &serde_json::to_vec(&FileCtrl { id: id.clone(), action: "resume".into() }).unwrap_or_default());
            set_st(&x, 0);
        }
    }
    emit_final(&x);
    Ok(())
}

#[tauri::command]
pub async fn social_file_cancel(id: String) -> Result<(), String> {
    let x = XFERS.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned().ok_or("传输不存在")?;
    set_st(&x, 2);
    if x.dir == "in" {
        let _ = sess_send(&x.peer, T_FILE_CTRL, &serde_json::to_vec(&FileCtrl { id: id.clone(), action: "cancel".into() }).unwrap_or_default());
        let _ = std::fs::remove_file(&x.path);
        XFERS.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
    } else {
        // 发送方取消：等待中(st5)同时摘除待确认表（迟到 ACK 走失效回绝）；worker 退出时补发 END(ok=false)
        PENDING_ACK.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
        let _ = sess_send(&x.peer, T_FILE_CTRL, &serde_json::to_vec(&FileCtrl { id: id.clone(), action: "cancel".into() }).unwrap_or_default());
    }
    emit_final(&x);
    Ok(())
}

/// 重新发起（发送方）：同 id 再走一次 OFFER（接收端按已落盘字节续传）
#[tauri::command]
pub async fn social_file_retry(id: String) -> Result<XferOut, String> {
    let (path, uid, gid) = {
        let g = XFERS.lock().unwrap_or_else(|e| e.into_inner());
        let x = g.get(&id).ok_or("传输不存在")?;
        if x.dir != "out" { return Err("仅发送方可续传".into()); }
        (x.path.to_string_lossy().to_string(), x.peer.clone(), x.gid.clone())
    };
    let (uid2, gid2) = (uid.clone(), gid);
    tauri::async_runtime::spawn_blocking(move || send_file_impl(&uid2, &path, &gid2))
        .await
        .map_err(|_| "文件任务失败".to_string())?
}

/// 重新接受（接收方，发送方重发后收不到新邀请时）：同 id 重建接收
#[tauri::command]
pub async fn social_file_receive_again(id: String) -> Result<(), String> {
    let x = XFERS.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned().ok_or("传输不存在")?;
    if x.dir != "in" { return Err("仅接收方可续收".into()); }
    if xst(&x) == 0 || xst(&x) == 3 { return Ok(()); }
    let (stream, key) = sess_get_or_open(&x.peer)?;
    {
        let mut s = stream.lock().unwrap_or_else(|e| e.into_inner());
        frame_send(&mut s, &key, T_FILE_ACK, &serde_json::to_vec(&FileAck { id: id.clone(), ok: true, offset: x.done.load(Ordering::SeqCst) }).unwrap_or_default())
            .map_err(|e| format!("续收失败: {e}"))?;
    }
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(64);
    *x.tx_in.lock().unwrap_or_else(|e| e.into_inner()) = Some(tx);
    set_st(&x, 0);
    let x2 = x.clone();
    std::thread::spawn(move || recv_file_worker(x2, rx));
    Ok(())
}

// session_loop 路由：ACK（v0.9.12 事件驱动 —— 直接启动/终止发送，无阻塞等待）
fn route_ack(payload: &[u8], stream: Arc<Mutex<TcpStream>>, key: [u8; 32]) {
    let Ok(ack) = serde_json::from_slice::<FileAck>(payload) else { return };
    let xfer = PENDING_ACK.lock().unwrap_or_else(|e| e.into_inner()).remove(&ack.id);
    let Some(xfer) = xfer else {
        if ack.ok {
            // 本机已取消/重启导致邀请失效 → 明确回绝，接收端立即收场（不悬挂"接收中"）
            let mut s = stream.lock().unwrap_or_else(|e| e.into_inner());
            let _ = frame_send(&mut s, &key, T_FILE_ACK,
                &serde_json::to_vec(&FileAck { id: ack.id, ok: false, offset: 0 }).unwrap_or_default());
        } else if let Some(x) = XFERS.lock().unwrap_or_else(|e| e.into_inner()).get(&ack.id).cloned() {
            // 本机接受的邀请被对方告知失效 → 收场
            if x.dir == "in" && xst(&x) == 0 {
                *x.tx_in.lock().unwrap_or_else(|e| e.into_inner()) = None;
                set_st(&x, 2);
                emit_final(&x);
            }
        }
        return;
    };
    if !ack.ok {
        set_st(&xfer, 2); // 对方拒绝 → 卡片按终态退场
        emit_final(&xfer);
        return;
    }
    xfer.done.store(ack.offset.min(xfer.size), Ordering::SeqCst);
    set_st(&xfer, 0);
    let (x2, s2, k2, o2) = (xfer.clone(), stream.clone(), key, ack.offset);
    std::thread::spawn(move || send_file_worker(x2, s2, k2, o2));
}

fn route_chunk(payload: &[u8]) {
    if payload.len() < 20 { return; }
    let id = String::from_utf8_lossy(&payload[..16]).to_string();
    // v0.9.20 防御：id 契约 = 16 位 hex；不符（如历史 32 字符错位帧）直接丢弃并留痕
    if id.len() != 16 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        socdbg(&format!("xfer: 丢弃非法块帧 id={id:?} len={}", payload.len()));
        return;
    }
    let x = XFERS.lock().unwrap_or_else(|e| e.into_inner()).get(&id).cloned();
    let Some(x) = x else { return };
    if x.dir != "in" { return; }
    // 暂停期间在途块照收不误（暂停=请求发送端停止，不丢数据防错位）；取消/失败才丢弃
    let s = xst(&x);
    if s == 2 || s == 4 || s == 5 { return; }
    let tx = x.tx_in.lock().unwrap_or_else(|e| e.into_inner()).clone();
    if let Some(tx) = tx {
        let _ = tx.send(payload[20..].to_vec()); // 有界通道：磁盘慢时阻塞 session_loop → TCP 背压 → 发送端自动减速
    }
}

fn route_end(payload: &[u8]) {
    let Ok(end) = serde_json::from_slice::<FileEnd>(payload) else { return };
    let x = XFERS.lock().unwrap_or_else(|e| e.into_inner()).get(&end.id).cloned();
    if let Some(x) = x {
        if x.dir == "in" {
            let cur = xst(&x);
            if cur == 2 || cur == 3 { return; } // 已收场（取消/完成）：不覆盖，杜绝重复事件
            if cur == 5 {
                // v0.9.17 邀请未被接受即收到 END → 对方已撤回
                PENDING_IN.lock().unwrap_or_else(|e| e.into_inner()).remove(&end.id);
                XFERS.lock().unwrap_or_else(|e| e.into_inner()).remove(&end.id);
                emit_ev("social-offer-gone", serde_json::json!({ "id": end.id, "text": "对方取消了文件发送" }));
                return;
            }
            if end.ok {
                // v0.9.19 完成回执只更新状态：通道留给 worker 自然收尾（排空在途块 + 尾部缓冲落盘）
                set_st(&x, 3);
            } else {
                // 中止：掐断通道唤醒 worker（排空在途块 → flush 已收字节 → 按失败收场）
                *x.tx_in.lock().unwrap_or_else(|e| e.into_inner()) = None;
                set_st(&x, 4);
            }
            emit_final(&x);
            // v0.9.3：落库只在 recv_file_worker 写完最后一块时发生一次；
            // 此处再落一次曾导致接收端出现两条一模一样的文件记录（重复提示的根因）
        }
    }
}

fn route_ctrl(payload: &[u8]) {
    let Ok(c) = serde_json::from_slice::<FileCtrl>(payload) else { return };
    let x = XFERS.lock().unwrap_or_else(|e| e.into_inner()).get(&c.id).cloned();
    if let Some(x) = x {
        match c.action.as_str() {
            "pause" => set_st(&x, 1),
            "resume" => set_st(&x, 0),
            "cancel" => {
                if x.dir == "out" {
                    set_st(&x, 2); // worker 下轮退出并补发 END(ok=false)
                } else if xst(&x) == 5 {
                    // v0.9.17 对方撤回邀请：整卡退场（邀请卡 + 待接收登记一并清除）
                    PENDING_IN.lock().unwrap_or_else(|e| e.into_inner()).remove(&c.id);
                    XFERS.lock().unwrap_or_else(|e| e.into_inner()).remove(&c.id);
                    emit_ev("social-offer-gone", serde_json::json!({ "id": c.id, "text": "对方取消了文件发送" }));
                    return; // 卡片已整体退场，不再发终态事件
                } else {
                    // v0.9.17 传输中被对方取消：终态 + 明确提示（此前接收方向直接忽略）
                    *x.tx_in.lock().unwrap_or_else(|e| e.into_inner()) = None;
                    set_st(&x, 2);
                    emit_ev("social-xfer-note", serde_json::json!({ "id": c.id, "text": "对方取消了文件传输" }));
                }
            }
            _ => {}
        }
        emit_final(&x);
    }
}

// ---------------------------- 群聊 ----------------------------

fn groups_dir() -> std::path::PathBuf { crate::settings::social_dir().join("groups") }

fn group_path(gid: &str) -> Option<std::path::PathBuf> {
    if gid.is_empty() || !gid.chars().all(|c| c.is_ascii_alphanumeric()) { return None; }
    Some(groups_dir().join(format!("{gid}.json")))
}

fn save_group(info: &GroupInfo) -> bool {
    let Some(p) = group_path(&info.gid) else { return false };
    let _ = std::fs::create_dir_all(groups_dir());
    match serde_json::to_string_pretty(info) {
        Ok(s) => std::fs::write(&p, s).is_ok(),
        Err(_) => false,
    }
}

fn load_groups() {
    let dir = groups_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(rd) = std::fs::read_dir(&dir) {
        let mut map = GROUPS.lock().unwrap_or_else(|e| e.into_inner());
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.extension().map(|x| x == "json").unwrap_or(false) {
                if let Ok(s) = std::fs::read_to_string(&p) {
                    if let Ok(info) = serde_json::from_str::<GroupInfo>(&s) {
                        if !info.dissolve { map.insert(info.gid.clone(), info); }
                    }
                }
            }
        }
    }
}

fn group_of(gid: &str) -> Option<GroupInfo> {
    GROUPS.lock().unwrap_or_else(|e| e.into_inner()).get(gid).cloned()
}

fn group_hist_key(gid: &str) -> String { format!("group_{gid}") }

/// 群名单同步（群主/管理 → 每个成员逐人加密单播）
fn group_sync_fanout(info: &GroupInfo) {
    let payload = serde_json::to_vec(info).unwrap_or_default();
    for m in &info.members {
        if m.uid == my_profile().uid { continue; }
        let _ = sess_send(&m.uid, T_GROUP_SYNC, &payload);
    }
}

fn is_admin_of(info: &GroupInfo, uid: &str) -> bool {
    info.owner == uid || info.admins.iter().any(|a| a == uid)
}

fn member_names(info: &GroupInfo) -> String {
    info.members.iter().map(|m| m.nickname.clone()).collect::<Vec<_>>().join("、")
}

/// 建群：创建者默认群主兼管理员；成员快照来自雷达发现列表
#[tauri::command]
pub async fn social_create_group(name: String, uids: Vec<String>) -> Result<GroupInfo, String> {
    let name = name.trim().chars().take(24).collect::<String>();
    if name.is_empty() { return Err("群名称不能为空".into()); }
    if uids.is_empty() { return Err("请至少选择 1 位成员".into()); }
    let me = my_profile();
    let mut members = vec![GroupMember { uid: me.uid.clone(), nickname: me.nickname.clone(), avatar: me.avatar }];
    {
        let g = PEERS.lock().unwrap_or_else(|e| e.into_inner());
        for u in &uids {
            if u == &me.uid { continue; }
            if let Some((p, _, _)) = g.get(u) {
                if !members.iter().any(|m| &m.uid == u) {
                    members.push(GroupMember { uid: p.uid.clone(), nickname: p.nickname.clone(), avatar: p.avatar.min(19) });
                }
            }
        }
    }
    if members.len() < 2 { return Err("所选成员均不可用（需先雷达发现）".into()); }
    let gid = format!("G{:x}{:02x}", now_ms(), os_seed8()[0]);
    let info = GroupInfo { gid: gid.clone(), name, owner: me.uid.clone(), admins: vec![me.uid.clone()], members, dissolve: false };
    save_group(&info);
    GROUPS.lock().unwrap_or_else(|e| e.into_inner()).insert(gid, info.clone());
    socdbg(&format!("group: 建群 {} 成员 {}", info.name, member_names(&info)));
    let info2 = info.clone();
    std::thread::spawn(move || group_sync_fanout(&info2));
    Ok(info)
}

/// 群消息：本地落库 + 本地事件（发送者界面立即显示）+ 逐人加密单播
#[tauri::command]
pub async fn social_group_send(gid: String, text: String) -> Result<SendAck, String> {
    let text = text.trim().to_string();
    if text.is_empty() { return Err("消息为空".into()); }
    if text.chars().count() > MAX_TEXT_CHARS { return Err(format!("消息过长（≤{MAX_TEXT_CHARS} 字）")); }
    let info = group_of(&gid).ok_or("群不存在")?;
    let me = my_profile();
    if !info.members.iter().any(|m| m.uid == me.uid) { return Err("你已不在该群".into()); }
    let ts = now_ms();
    persist_group_msg(&gid, "out", &me.uid, &me.nickname, &text, ts);
    emit_ev("social-group-msg", serde_json::json!({ "gid": gid, "from": me.uid, "name": me.nickname, "text": text, "ts": ts }));
    let payload = serde_json::to_vec(&GroupText { gid: gid.clone(), from: me.uid.clone(), name: me.nickname.clone(), text, ts }).unwrap_or_default();
    let mut fails = Vec::new();
    for m in &info.members {
        if m.uid == me.uid { continue; }
        if let Err(e) = sess_send(&m.uid, T_GROUP_TEXT, &payload) {
            fails.push(format!("{}: {e}", m.nickname));
        }
    }
    if !fails.is_empty() { socdbg(&format!("group: 群消息部分失败 {}", fails.join("；"))); }
    Ok(SendAck { ts })
}

fn persist_group_msg(gid: &str, dir: &str, from: &str, from_name: &str, text: &str, ts: u64) {
    if let Some(f) = msg_file(&group_hist_key(gid)) {
        let item = HistItem {
            d: dir.to_string(), ts, text: text.to_string(), file: String::new(), size: 0,
            path: String::new(), from: from.to_string(), from_name: from_name.to_string(),
        };
        if let Ok(mut fh) = std::fs::OpenOptions::new().create(true).append(true).open(&f) {
            let _ = writeln!(fh, "{}", serde_json::to_string(&item).unwrap_or_default());
        }
    }
}

/// session_loop 路由：群消息（校验发送者必须是本群成员 —— 以会话握手 uid 为准）
fn handle_group_text(from_uid: &str, payload: &[u8]) {
    let Ok(gt) = serde_json::from_slice::<GroupText>(payload) else { return };
    let Some(info) = group_of(&gt.gid) else { socdbg("group: 收到未知群消息（名单未同步），忽略"); return };
    if !info.members.iter().any(|m| m.uid == from_uid) { socdbg("group: 群消息发送者不在成员表，忽略"); return; }
    let name = info.members.iter().find(|m| m.uid == from_uid).map(|m| m.nickname.clone()).unwrap_or_else(|| gt.name.clone());
    persist_group_msg(&gt.gid, "in", from_uid, &name, &gt.text, gt.ts);
    emit_ev("social-group-msg", serde_json::json!({ "gid": gt.gid, "from": from_uid, "name": name, "text": gt.text, "ts": gt.ts }));
    let gname = info.name.clone();
    social_bubble_notify(&format!("group:{}", gt.gid), &format!("💬 {} · {}", gname, name),
        &{ let mut t: String = gt.text.chars().take(60).collect(); if gt.text.chars().count() > 60 { t.push('…'); } t },
        &format!("group:{}", gt.gid));
}

/// session_loop 路由：名单同步（仅接受来自群主/管理员的同步；成员本地落盘）
fn handle_group_sync(from_uid: &str, payload: &[u8]) {
    let Ok(info) = serde_json::from_slice::<GroupInfo>(payload) else { return };
    let Some(p) = group_path(&info.gid) else { return };
    if info.dissolve {
        GROUPS.lock().unwrap_or_else(|e| e.into_inner()).remove(&info.gid);
        let _ = std::fs::remove_file(&p);
        emit_ev("social-group-gone", serde_json::json!({ "gid": info.gid }));
        socdbg(&format!("group: 群 {} 已被群主解散", info.gid));
        return;
    }
    // 防伪造：同步帧必须来自该群群主或管理员（会话 uid），且我必须是成员
    let ok = (info.owner == from_uid || info.admins.iter().any(|a| a == from_uid))
        && info.members.iter().any(|m| m.uid == my_profile().uid);
    if !ok { socdbg("group: 名单同步未通过校验，忽略"); return; }
    save_group(&info);
    let gid = info.gid.clone();
    GROUPS.lock().unwrap_or_else(|e| e.into_inner()).insert(gid.clone(), info);
    emit_ev("social-group-sync", serde_json::json!({ "gid": gid }));
}

/// 邀请成员（群主/管理员）
#[tauri::command]
pub async fn social_group_invite(gid: String, uids: Vec<String>) -> Result<GroupInfo, String> {
    let mut info = group_of(&gid).ok_or("群不存在")?;
    let me = my_profile();
    if !is_admin_of(&info, &me.uid) { return Err("仅群主/管理员可邀请".into()); }
    {
        let g = PEERS.lock().unwrap_or_else(|e| e.into_inner());
        for u in &uids {
            if info.members.iter().any(|m| &m.uid == u) { continue; }
            if let Some((p, _, _)) = g.get(u) {
                info.members.push(GroupMember { uid: p.uid.clone(), nickname: p.nickname.clone(), avatar: p.avatar.min(19) });
            }
        }
    }
    save_group(&info);
    let info2 = info.clone();
    std::thread::spawn(move || group_sync_fanout(&info2));
    Ok(info)
}

/// 移出成员：群主可移除除自己外任何人；管理员仅可移除普通成员（微信同逻辑）
#[tauri::command]
pub async fn social_group_kick(gid: String, uid: String) -> Result<GroupInfo, String> {
    let mut info = group_of(&gid).ok_or("群不存在")?;
    let me = my_profile();
    if !is_admin_of(&info, &me.uid) { return Err("仅群主/管理员可移出成员".into()); }
    if uid == info.owner { return Err("不能移除群主".into()); }
    if me.uid != info.owner && info.admins.iter().any(|a| a == &uid) { return Err("管理员只能由群主移除".into()); }
    if uid == me.uid { return Err("移除自己请用「退出群聊」".into()); }
    info.members.retain(|m| m.uid != uid);
    info.admins.retain(|a| a != &uid);
    save_group(&info);
    let info2 = info.clone();
    std::thread::spawn(move || group_sync_fanout(&info2));
    Ok(info)
}

/// 任命/撤销管理员（仅群主）
#[tauri::command]
pub async fn social_group_set_admin(gid: String, uid: String, admin: bool) -> Result<GroupInfo, String> {
    let mut info = group_of(&gid).ok_or("群不存在")?;
    let me = my_profile();
    if me.uid != info.owner { return Err("仅群主可设置管理员".into()); }
    if uid == info.owner { return Err("群主本身就是管理员".into()); }
    if admin {
        if !info.members.iter().any(|m| m.uid == uid) { return Err("对方不是群成员".into()); }
        if !info.admins.contains(&uid) { info.admins.push(uid); }
    } else {
        info.admins.retain(|a| a != &uid);
    }
    save_group(&info);
    let info2 = info.clone();
    std::thread::spawn(move || group_sync_fanout(&info2));
    Ok(info)
}

/// 群改名（群主/管理员）
#[tauri::command]
pub async fn social_group_rename(gid: String, name: String) -> Result<GroupInfo, String> {
    let mut info = group_of(&gid).ok_or("群不存在")?;
    let me = my_profile();
    if !is_admin_of(&info, &me.uid) { return Err("仅群主/管理员可修改群名".into()); }
    let name = name.trim().chars().take(24).collect::<String>();
    if name.is_empty() { return Err("群名称不能为空".into()); }
    info.name = name;
    save_group(&info);
    let info2 = info.clone();
    std::thread::spawn(move || group_sync_fanout(&info2));
    Ok(info)
}

/// 退出群聊：群主退出 = 解散；成员退出 → 通知群主/管理员更新名单
#[tauri::command]
pub async fn social_group_leave(gid: String) -> Result<(), String> {
    let info = group_of(&gid).ok_or("群不存在")?;
    let me = my_profile();
    if me.uid == info.owner {
        return group_dissolve_impl(&gid);
    }
    GROUPS.lock().unwrap_or_else(|e| e.into_inner()).remove(&gid);
    if let Some(p) = group_path(&gid) { let _ = std::fs::remove_file(&p); }
    let payload = serde_json::to_vec(&GroupLeave { gid: gid.clone(), from: me.uid.clone() }).unwrap_or_default();
    let targets: Vec<String> = info.members.iter().filter(|m| is_admin_of(&info, &m.uid) && m.uid != me.uid).map(|m| m.uid.clone()).collect();
    std::thread::spawn(move || {
        for t in targets { let _ = sess_send(&t, T_GROUP_LEAVE, &payload); }
    });
    Ok(())
}

/// 解散群聊（仅群主）：本地清除 + 全员同步 dissolve
fn group_dissolve_impl(gid: &str) -> Result<(), String> {
    let info = group_of(gid).ok_or("群不存在")?;
    let me = my_profile();
    if me.uid != info.owner { return Err("仅群主可解散群聊".into()); }
    GROUPS.lock().unwrap_or_else(|e| e.into_inner()).remove(gid);
    if let Some(p) = group_path(gid) { let _ = std::fs::remove_file(&p); }
    let mut d = info.clone();
    d.dissolve = true;
    std::thread::spawn(move || group_sync_fanout(&d));
    Ok(())
}

#[tauri::command]
pub async fn social_group_dissolve(gid: String) -> Result<(), String> {
    group_dissolve_impl(&gid)
}

/// session_loop 路由：退群通知（群主/管理员更新名单并同步）
fn handle_group_leave(from_uid: &str, payload: &[u8]) {
    let Ok(gl) = serde_json::from_slice::<GroupLeave>(payload) else { return };
    let mut info = match group_of(&gl.gid) { Some(i) => i, None => return };
    if !is_admin_of(&info, from_uid) { return; }
    info.members.retain(|m| m.uid != gl.from);
    info.admins.retain(|a| a != &gl.from);
    save_group(&info);
    let info2 = info.clone();
    std::thread::spawn(move || group_sync_fanout(&info2));
}

/// 已读回执（好友）：通知对端"其消息我已读至 up_to"
#[tauri::command]
pub async fn social_mark_read(uid: String, up_to: u64) -> Result<(), String> {
    let payload = serde_json::to_vec(&ReadUpTo { up_to }).unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || sess_send(&uid, T_READ, &payload))
        .await
        .map_err(|_| "回执任务失败".to_string())?
}

/// 已读表快照（uid → 对端读到我方消息的最大 ts），前端首屏渲染用
#[tauri::command]
pub async fn social_read_all() -> Result<std::collections::HashMap<String, u64>, String> {
    Ok(LAST_READ.lock().unwrap_or_else(|e| e.into_inner()).clone())
}

/// 抖一抖（好友）：让对方窗口原生抖动 2 秒
#[tauri::command]
pub async fn social_shake(uid: String) -> Result<(), String> {
    let me = my_profile();
    let payload = serde_json::to_vec(&PeerShake { from: me.uid.clone(), name: me.nickname.clone() }).unwrap_or_default();
    tauri::async_runtime::spawn_blocking(move || sess_send(&uid, T_PEER_SHAKE, &payload))
        .await
        .map_err(|_| "发送任务失败".to_string())?
}

/// 一键提醒（群）：全员弹出主窗到社交页
#[tauri::command]
pub async fn social_group_nudge(gid: String) -> Result<(), String> {
    let info = group_of(&gid).ok_or("群不存在")?;
    let me = my_profile();
    if !is_admin_of(&info, &me.uid) { return Err("仅群主/管理员可一键提醒".into()); }
    let payload = serde_json::to_vec(&GroupNudge { gid: gid.clone(), from: me.uid.clone(), name: me.nickname.clone() }).unwrap_or_default();
    std::thread::spawn(move || {
        for m in &info.members {
            if m.uid != me.uid { let _ = sess_send(&m.uid, T_GROUP_NUDGE, &payload); }
        }
    });
    emit_ev("social-nudge", serde_json::json!({ "gid": gid, "name": info.name }));
    Ok(())
}

/// session_loop 路由：收到一键提醒 → 弹出主窗到社交页（校验发送者必须是该群群主/管理员）
fn handle_group_nudge(from_uid: &str, payload: &[u8]) {
    let Ok(n) = serde_json::from_slice::<GroupNudge>(payload) else { return };
    let Some(info) = group_of(&n.gid) else { return };
    if !is_admin_of(&info, from_uid) { socdbg("group: 提醒发送者无管理权限，忽略"); return; }
    use tauri::Manager;
    if let Some(app) = crate::settings::APP.get() {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
    emit_ev("goto-social", serde_json::json!({ "gid": n.gid }));
    emit_ev("social-nudge", serde_json::json!({ "gid": n.gid, "name": info.name, "from_name": n.name }));
    sound_notify();
    crate::diag::write("I", &format!("社交群提醒：{} 在群「{}」发起一键提醒", n.name, info.name));
}

// v0.9.21 已读回执：对端读到我方消息的最大时间戳（uid = 对端）
static LAST_READ: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, u64>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

fn read_state_path() -> Option<std::path::PathBuf> {
    Some(crate::settings::social_dir().join("read_state.json"))
}

fn load_read_state() {
    let Some(p) = read_state_path() else { return };
    if let Ok(txt) = std::fs::read_to_string(&p) {
        if let Ok(m) = serde_json::from_str::<std::collections::HashMap<String, u64>>(&txt) {
            *LAST_READ.lock().unwrap_or_else(|e| e.into_inner()) = m;
        }
    }
}

fn save_read_state() {
    let Some(p) = read_state_path() else { return };
    let g = LAST_READ.lock().unwrap_or_else(|e| e.into_inner());
    if let Ok(txt) = serde_json::to_string(&*g) {
        let _ = std::fs::write(&p, txt);
    }
}

/// session_loop 路由：对方读到我方消息（至 up_to）→ 记忆 + 通知前端
fn handle_read(uid: &str, payload: &[u8]) {
    let Ok(r) = serde_json::from_slice::<ReadUpTo>(payload) else { return };
    {
        let mut g = LAST_READ.lock().unwrap_or_else(|e| e.into_inner());
        let cur = g.get(uid).copied().unwrap_or(0);
        if r.up_to <= cur { return; } // 幂等：只前进
        g.insert(uid.to_string(), r.up_to);
    }
    save_read_state();
    emit_ev("social-read", serde_json::json!({ "uid": uid, "up_to": r.up_to }));
    socdbg(&format!("read: {} 已读至 {}", &uid[..uid.len().min(8)], r.up_to));
}

/// session_loop 路由：收到好友抖一抖 → 弹出主窗 + 原生抖窗 2 秒
fn handle_peer_shake(from_uid: &str, payload: &[u8]) {
    let Ok(p) = serde_json::from_slice::<PeerShake>(payload) else { return };
    use tauri::Manager;
    if let Some(app) = crate::settings::APP.get() {
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
    window_shake();
    emit_ev("goto-social", serde_json::json!({ "uid": from_uid }));
    emit_ev("social-shake", serde_json::json!({ "from": from_uid, "name": p.name }));
    sound_notify();
    crate::diag::write("I", &format!("社交抖一抖：{} 发起窗口抖动", p.name));
}

static SHAKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 原生窗口抖动约 2 秒：正弦衰减 ±9px（11 个周期），绝不叠加（进行中忽略）
fn window_shake() {
    use std::sync::atomic::Ordering;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOSIZE, SWP_NOZORDER,
    };
    if SHAKING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    let Some(app) = crate::settings::APP.get() else {
        SHAKING.store(false, Ordering::SeqCst);
        return;
    };
    use tauri::Manager;
    let Some(w) = app.get_webview_window("main") else {
        SHAKING.store(false, Ordering::SeqCst);
        return;
    };
    let Ok(h) = w.hwnd() else {
        SHAKING.store(false, Ordering::SeqCst);
        return;
    };
    let hwnd_raw = h.0 as isize; // HWND 跨线程传递用 isize（原始句柄是 Send 的）
    std::thread::spawn(move || unsafe {
        let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &mut r).is_ok() {
            let (ox, oy) = (r.left, r.top);
            let steps = 66u32; // ~2s @ 30ms
            for i in 0..steps {
                let t = i as f32 / steps as f32;
                let off = (9.0f32 * (1.0 - t) * (t * std::f32::consts::PI * 22.0).sin()).round() as i32;
                let _ = SetWindowPos(hwnd, HWND(std::ptr::null_mut()), ox + off, oy, 0, 0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            let _ = SetWindowPos(hwnd, HWND(std::ptr::null_mut()), ox, oy, 0, 0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
        SHAKING.store(false, Ordering::SeqCst);
    });
}

/// 社交消息 → 右下角气泡（与威胁预警同通道、聊天样式）：
/// 仅当用户没在盯社交页（主窗隐藏 / 失焦 / 停留其他板块超 25 秒）才打扰；按会话 8 秒节流
fn social_bubble_notify(key: &str, title: &str, desc: &str, ctx: &str) {
    use std::sync::atomic::Ordering;
    let Some(app) = crate::settings::APP.get() else { return };
    let (hidden, focused) = {
        use tauri::Manager;
        match app.get_webview_window("main") {
            Some(w) => (
                w.is_visible().map(|v| !v).unwrap_or(true),
                w.is_focused().unwrap_or(false),
            ),
            None => (true, false),
        }
    };
    let stale = now_ms().saturating_sub(LAST_PAGE_POLL.load(Ordering::Relaxed)) > 25_000;
    if !hidden && focused && !stale {
        return; // 用户正在社交页聊天：页内有即时提示，气泡反而是打扰
    }
    {
        let mut g = BUBBLE_LAST.lock().unwrap_or_else(|e| e.into_inner());
        let last = g.get(key).copied().unwrap_or(0);
        if now_ms().saturating_sub(last) < 8_000 {
            return;
        }
        g.insert(key.to_string(), now_ms());
    }
    crate::monitor::notify_social(&app, title, desc, ctx);
    // v0.9.3：托盘图标闪烁 + 任务栏闪烁 + 系统提示音（点击托盘或回焦即停）
    crate::social_tray_flash_start(&app, ctx.to_string());
    sound_notify();
}

// ---------- v0.9.2 社交功能设置：接收目录 / 数据目录 / 打开文件 ----------

/// 选择接收文件夹（系统原生对话框）
#[tauri::command]
pub async fn social_pick_dir(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use std::sync::atomic::Ordering;
    if PICKING.swap(true, Ordering::SeqCst) { return Ok(None); }
    let r = tauri::async_runtime::spawn_blocking(move || {
        use tauri::Manager;
        let mut d = rfd::FileDialog::new().set_title("选择接收文件的保存位置");
        if let Some(w) = app.get_webview_window("main") { d = d.set_parent(&w); }
        let r = d.pick_folder().map(|p| p.to_string_lossy().to_string());
        PICKING.store(false, Ordering::SeqCst);
        r
    })
    .await
    .map_err(|e| { PICKING.store(false, Ordering::SeqCst); format!("文件对话框失败: {e}") })?;
    Ok(r)
}

#[derive(serde::Serialize)]
pub struct SocialDirs {
    pub recv: String,    // 当前生效的接收根目录
    pub custom: String,  // 用户自定义（空 = 默认）
    pub chat: String,    // 聊天数据（历史/群名单）目录：data\social
    pub zombie: bool,    // v0.9.3 僵尸号自动清理开关
    pub font: u8,        // v0.9.5 聊天字号（px）
}

/// 读取社交目录设置
#[tauri::command]
pub async fn social_dirs() -> Result<SocialDirs, String> {
    let custom = crate::settings::config().social_recv_dir.clone();
    let chat = crate::settings::social_dir();
    let recv = recv_root().join("TexMantis");
    // v0.9.7 关键修复：config 锁单次获取 —— 同一语句两次 config()（结构体字面量的
    // 临时守卫活到语句末）= 自锁死锁，且死锁线程永远持有 CONFIG 锁，拖垮全部命令
    // （v0.8.x 同类历史教训；v0.9.5 加 font 字段时引入，v0.9.6 开机即挂载社交页引爆）
    let (zombie, font) = {
        let c = crate::settings::config();
        (c.social_zombie, c.social_font_size.clamp(12, 17))
    };
    Ok(SocialDirs {
        recv: recv.to_string_lossy().to_string(),
        custom,
        chat: chat.to_string_lossy().to_string(),
        zombie,
        font,
    })
}

/// 设置接收根目录（空串 = 恢复默认「下载」）；目录会被创建
#[tauri::command]
pub async fn social_set_recv_dir(path: String) -> Result<SocialDirs, String> {
    let path = path.trim().to_string();
    if !path.is_empty() {
        std::fs::create_dir_all(&path).map_err(|e| format!("无法创建目录: {e}"))?;
    }
    crate::settings::update_config(|c| c.social_recv_dir = path.clone());
    socdbg(&format!("cmd set_recv_dir → {}", if path.is_empty() { "<默认下载>" } else { &path }));
    social_dirs().await
}

/// 手动移除联系人：从列表与历史存档中移除（对方来信会自动恢复；删除不等于拉黑）
#[tauri::command]
pub async fn social_remove_peer(uid: String) -> Result<(), String> {
    crate::settings::update_config(|c| {
        if !c.social_removed.contains(&uid) {
            c.social_removed.push(uid.clone());
        }
    });
    PEERS.lock().unwrap_or_else(|e| e.into_inner()).remove(&uid);
    mark_peers_dirty();
    save_peers_if_dirty_force();
    socdbg(&format!("cmd remove_peer {}", &uid[..8.min(uid.len())]));
    Ok(())
}

/// 聊天字号设置（12-17px；前端实时预览，松手落盘）
#[tauri::command]
pub async fn social_set_font_size(size: u8) -> Result<SocialDirs, String> {
    crate::settings::update_config(|c| c.social_font_size = size.clamp(12, 17));
    social_dirs().await
}

/// 僵尸号自动清理开关（同机多身份合并：保留最活跃者、备注迁移）
#[tauri::command]
pub async fn social_set_zombie(on: bool) -> Result<SocialDirs, String> {
    crate::settings::update_config(|c| c.social_zombie = on);
    if on { zombie_sweep("setting-on"); }
    social_dirs().await
}

/// 立即落盘（绕过节流；移除等用户显式动作要求即时生效）
fn save_peers_if_dirty_force() {
    LAST_PEERS_SAVE.store(0, std::sync::atomic::Ordering::Relaxed);
    save_peers_if_dirty();
}

/// 在资源管理器中定位文件（打开所在文件夹并选中）
#[tauri::command]
pub async fn social_reveal_path(path: String) -> Result<(), String> {
    let p = std::path::PathBuf::from(path.trim().replace('/', "\\"));
    if !p.exists() {
        return Err("文件不存在或已被移动".to_string());
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        Command::new("explorer")
            .raw_arg(format!("/select,\"{}\"", p.to_string_lossy()))
            .spawn()
            .map_err(|e| format!("打开失败: {e}"))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("仅支持 Windows".into())
    }
}

/// 双击打开文件/文件夹（聊天流文件气泡、设置页「打开目录」共用）
#[tauri::command]
pub async fn social_open_path(path: String) -> Result<(), String> {
    let p = std::path::PathBuf::from(path.trim());
    if !p.exists() {
        return Err("文件不存在或已被移动".to_string());
    }
    #[cfg(windows)]
    {
        use windows::core::{HSTRING, PCWSTR};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let wide = HSTRING::from(p.as_os_str().to_string_lossy().as_ref());
        unsafe {
            ShellExecuteW(None, windows::core::w!("open"), PCWSTR(wide.as_ptr()), None, None, SW_SHOWNORMAL);
        }
    }
    Ok(())
}

/// 提示音命令（浮漂窗/提醒用）：走系统通知音，绕开 WebView 自动播放限制
#[tauri::command]
pub async fn social_notify_sound() -> Result<(), String> {
    sound_notify();
    Ok(())
}

/// 提示音：系统通知音（MessageBeep，无 WebView 自动播放限制），2 秒节流
pub fn sound_notify() {
    let now = now_ms();
    if now.saturating_sub(LAST_SOUND.swap(now, Ordering::SeqCst)) < 2000 { return; }
    #[cfg(windows)]
    {
        use windows::Win32::System::Diagnostics::Debug::MessageBeep;
        use windows::Win32::UI::WindowsAndMessaging::MB_ICONASTERISK;
        unsafe {
            let _ = MessageBeep(MB_ICONASTERISK);
        }
    }
    #[cfg(not(windows))]
    let _ = ();
}

/// 群文件列表（接收落地的 group_{gid} 目录 + 群记录中的文件条目）
#[derive(Serialize, Clone)]
pub struct GroupFile { pub name: String, pub size: u64, pub ts: u64 }
#[tauri::command]
pub async fn social_group_files(gid: String) -> Result<Vec<GroupFile>, String> {
    let dir = xf_landing("", &gid);
    let mut out: Vec<GroupFile> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(&dir) {
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if !p.is_file() { continue; }
            let meta = e.metadata().ok();
            let ts = meta.as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            out.push(GroupFile { name: p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(), size: meta.map(|m| m.len()).unwrap_or(0), ts });
        }
    }
    out.sort_by(|a, b| b.ts.cmp(&a.ts));
    Ok(out)
}

// ---------------------------- 广播 ----------------------------

#[derive(Serialize, Clone)]
pub struct BcastOut { pub uid: String, pub ok: bool, pub err: String }

/// 广播：对所选（或全部在线）联系人逐人加密单播 —— 每条都是一次普通的
/// 端到端加密私聊，无组播/全网段包，不产生任何风暴特征
#[tauri::command]
pub async fn social_broadcast(uids: Vec<String>, text: String) -> Result<Vec<BcastOut>, String> {
    let text = text.trim().to_string();
    if text.is_empty() { return Err("消息为空".into()); }
    if text.chars().count() > MAX_TEXT_CHARS { return Err(format!("消息过长（≤{MAX_TEXT_CHARS} 字）")); }
    tauri::async_runtime::spawn_blocking(move || {
        let mut out = Vec::new();
        for uid in &uids {
            match send_text_impl(uid.clone(), text.clone()) {
                Ok(_) => out.push(BcastOut { uid: uid.clone(), ok: true, err: String::new() }),
                Err(e) => out.push(BcastOut { uid: uid.clone(), ok: false, err: e }),
            }
            std::thread::sleep(Duration::from_millis(120)); // 温和间隔，绝不突发
        }
        out
    })
    .await
    .map_err(|_| "广播任务失败".to_string())
}

// ---------------------------- 好友备注 ----------------------------

#[tauri::command]
pub async fn social_set_remark(uid: String, remark: String) -> Result<(), String> {
    if uid.is_empty() { return Err("uid 为空".into()); }
    let remark = remark.trim().chars().take(16).collect::<String>();
    crate::settings::update_config(|c| {
        if remark.is_empty() { c.social_remarks.remove(&uid); } else { c.social_remarks.insert(uid.clone(), remark); }
    });
    Ok(())
}
