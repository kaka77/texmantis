import { invoke } from "@tauri-apps/api/core";

export interface ProcInfo {
  pid: number;
  ppid: number | null;
  name: string;
  cpu: number;
  mem_mb: number;
  status: string;
  exe: string;
  cmd: string;
  started: number;
}
export interface SigInfo { path: string; status: string; signer: string; }
export interface SvcInfo {
  name: string;
  display: string;
  state: string;
  start_mode: string;
  account: string;
  pid: number | null;
  exe: string;
  company: string;
  is_microsoft: boolean;
}
export interface StartupInfo { hive: string; name: string; command: string; kind: string; }
export interface EventInfo { time: string; id: number; kind: string; result: string; user: string; ip: string; detail: string; }
export interface AlertInfo { id?: string; ts_ms: number; level: number; event_id: number; rule: string; title: string; mitre: string; desc: string; src: string; target: string; detail: string; kind?: string; ctx?: string; }
export interface AlertStatus {
  mode: string; running: boolean; sysmon: boolean; muted: boolean; text: string;
  version: string; chans: Array<[string, boolean]>; cfg_current: boolean; last_alert_min: number; health: string;
}
export interface RuleDef { id: string; event_id: number; name: string; level: number; mitre: string; desc: string; note: string; def_on: boolean; }
export interface AppSettings { auto_start: boolean; close_to_tray: boolean; pause_on_blur: boolean; float_dock: boolean; diag_log: boolean; alert_monitor: boolean; alert_retention_days: number; }
export interface TaskInfo { path: string; name: string; state: string; author: string; exe: string; args: string; trigger_count: number; }

export const getProcesses = () => invoke<ProcInfo[]>("get_processes");
export const killProcess = (pid: number) => invoke<string>("kill_process", { pid });
export const killTree = (pid: number) => invoke<string>("kill_tree", { pid });
export const getServices = () => invoke<SvcInfo[]>("get_services");
export const serviceAction = (name: string, action: string) => invoke<string>("service_action", { name, action });
export const getStartup = () => invoke<StartupInfo[]>("get_startup");
export const removeStartup = (kind: string, hive: string, name: string, command: string) =>
  invoke<string>("remove_startup", { kind, hive, name, command });
export const getTasks = () => invoke<TaskInfo[]>("get_tasks");
export const taskAction = (path: string, name: string, action: string) =>
  invoke<string>("task_action", { path, name, action });
export const checkSignatures = (paths: string[]) => invoke<SigInfo[]>("check_signatures", { paths });
export const isElevated = () => invoke<boolean>("is_elevated");
export const openFolder = (path: string) => invoke<void>("open_folder", { path });

export interface LockEntry { pid: number; name: string; exe: string; }
export interface ScanReport { locks: LockEntry[]; file_count: number; truncated: boolean; }
export interface DeleteFail { path: string; error: string; }
export interface DeleteReport { killed: string[]; deleted_files: number; failed: DeleteFail[]; }
export const unlockScan = (paths: string[]) => invoke<ScanReport>("unlock_scan", { paths });
export const forceDelete = (paths: string[]) => invoke<DeleteReport>("force_delete", { paths });

// ---------------- 系统优化 ----------------
export interface OptState {
  desktop_this_pc: boolean;
  desktop_recycle_bin: boolean;
  desktop_control_panel: boolean;
  desktop_network: boolean;
  taskbar_dbl_desktop: boolean;
  ctx_power_shell: boolean;
  ctx_cmd: boolean;
  ctx_unlock: boolean;
  ctrl_window_ms: number;
}
export const optGetAll = () => invoke<OptState>("opt_get_all");

// ---------------- 网络监控 ----------------
export interface NetConn { local: string; remote: string; state: string; pid: number; proc_name: string; }
export interface GeoStatus { found: boolean; kind: string; path: string; msg: string; }
export const netConnections = () => invoke<NetConn[]>("net_connections");
export const geoGetStatus = () => invoke<GeoStatus>("geo_get_status");
export const geoSetDbPath = (path: string) => invoke<GeoStatus>("geo_set_db_path", { path });
export const geoLookupIps = (ips: string[]) => invoke<Record<string, string>>("geo_lookup_ips", { ips });

// ---------------- 工具设置 ----------------
export interface AppSettings { auto_start: boolean; close_to_tray: boolean; pause_on_blur: boolean; float_dock: boolean; diag_log: boolean; }
export const settingsGet = () => invoke<AppSettings>("settings_get");
export const settingsSetAutoStart = (on: boolean) => invoke<void>("settings_set_auto_start", { on });
export const settingsSetFlag = (name: "CloseToTray" | "PauseOnBlur" | "DiagLog" | "AlertMonitor", on: boolean) =>
  invoke<void>("settings_set_flag", { name, on });
export const floatDockSet = (on: boolean) => invoke<string>("float_dock_set", { on });
export const hashFile = (path: string) => invoke<string>("hash_file", { path });
export const openUrl = (url: string) => invoke<void>("open_url", { url });
export const optApply = (
  desktop: string[],
  taskbar: boolean,
  ctxPs: boolean,
  ctxCmd: boolean,
  unlockCtx: boolean
) => invoke<OptState>("opt_apply", { desktop, taskbar, ctxPs, ctxCmd, unlockCtx });
export const unlockTakeRequest = () => invoke<string | null>("unlock_take_request");
export const eventQuery = (hours: number) => invoke<EventInfo[]>("event_query", { hours });
export const alertsGet = () => invoke<AlertInfo[]>("alerts_get");
export const alertsClear = () => invoke<void>("alerts_clear");
export const alertSetRetention = (days: number) => invoke<void>("alert_set_retention", { days });
export const alertRuleSet = (id: string, on: boolean) => invoke<void>("alert_rule_set", { id, on });
export const alertRulesState = () => invoke<string[]>("alert_rules_state");
export const alertMonitorStatus = () => invoke<AlertStatus>("alert_monitor_status");
export const alertSysmonInstall = () => invoke<string>("alert_sysmon_install");
export const alertImportConfig = (xml: string) => invoke<string>("alert_import_config", { xml });
export const alertTest = () => invoke<string>("alert_test");
export const alertSelftest = () => invoke<string>("alert_selftest");
export const alertSysmonResync = () => invoke<string>("alert_sysmon_resync");
export const diagRead = () => invoke<string>("diag_read");
export const diagClear = () => invoke<void>("diag_clear");
export interface AlertWl { sys_src: boolean; procs: string[]; targets: string[]; }
export const alertWlGet = () => invoke<AlertWl>("alert_wl_get");
export interface CheckItem { name: string; ok: boolean; detail: string; fixed: boolean; }
export const deepSelfcheck = () => invoke<CheckItem[]>("deep_selfcheck");
export const alertWlSet = (sys_src: boolean, procs: string[], targets: string[]) =>
  // Tauri2 命令参数默认按驼峰匹配：Rust sys_src ↔ JS sysSrc（下划线键会报 missing required key）
  invoke<void>("alert_wl_set", { sysSrc: sys_src, procs, targets });
export const alertMuteSet = (on: boolean) => invoke<void>("alert_mute_set", { on });
export const notifierTakePending = () => invoke<AlertInfo[]>("notifier_take_pending");
export const notifierFit = (height: number) => invoke<void>("notifier_fit", { height });
export const alertEngineToggle = (on: boolean) => invoke<string>("alert_engine_toggle", { on });
export const optSetCtrlWindow = (ms: number) => invoke<void>("opt_set_ctrl_window", { ms });

// ----- 社交模块「青刃·知己」（局域网即时通讯 · P1 服务与发现） -----
export interface SocialPeer {
  uid: string;
  nickname: string;
  remark: string; // v0.9.0 好友备注（空 = 未设置）
  gender: string; // m / f / x
  avatar: number; // 0..=19
  host: string;
  ip: string;
  port: number;
  online: boolean;
  last_seen_ms: number;
  fav: boolean;
}
export interface GroupMember { uid: string; nickname: string; avatar: number; }
export interface GroupInfo {
  gid: string;
  name: string;
  owner: string;
  admins: string[];
  members: GroupMember[];
  dissolve: boolean;
}
export interface XferOut {
  id: string;
  dir: "out" | "in";
  peer: string;
  gid: string;
  name: string;
  size: number;
  done: number;
  st: number; // 0 传输中 1 暂停 2 已取消 3 完成 4 失败 5 等待确认
}
export interface SocialStatus {
  running: boolean;
  port: number;
  reason: string;
  uid: string;
  nickname: string;
  gender: string;
  avatar: number;
  host: string;
  fp: string;       // 本机加密指纹（16 hex）
  named: boolean;   // 是否已生成/设置过昵称
  chat_left: boolean; // 两栏布局：聊天在左
  firewall: string; // ok / guest / fail
  peers: SocialPeer[];
  favs: string[];
  groups: GroupInfo[];
  xfers: XferOut[];
  sessions?: string[]; // 当前活跃加密会话 uid（v0.9.9 会话状态自愈）
}
export interface SocialMsgEvt { uid: string; nickname: string; text: string; ts: number; }
export interface GroupMsgEvt { gid: string; from: string; name: string; text: string; ts: number; }
export interface FileOfferEvt { id: string; from: string; from_name: string; name: string; size: number; gid: string; offset: number; }
export interface SocialHistItem {
  d: string; ts: number; text: string;
  file: string; size: number; path: string;
  from: string; from_name: string;
}
export interface GroupFile { name: string; size: number; ts: number; }
export interface BcastOut { uid: string; ok: boolean; err: string; }
export const socialStatus = () => invoke<SocialStatus>("social_get_status");
export const socialScan = () => invoke<SocialStatus>("social_scan");
export const socialSetProfile = (nickname: string, gender: string, avatar: number) =>
  invoke<SocialStatus>("social_set_profile", { nickname, gender, avatar });
export const socialSetFavs = (favs: string[]) => invoke<SocialStatus>("social_set_favs", { favs });
export const socialOpenSession = (uid: string) => invoke<{ fp: string }>("social_open_session", { uid });
export const socialSendText = (uid: string, text: string) => invoke<{ ts: number }>("social_send_text", { uid, text });
export const socialHistory = (uid: string) => invoke<SocialHistItem[]>("social_history", { uid });
export const socialGenName = (gender: string) => invoke<string>("social_gen_name", { gender });
export const socialSetLayout = (chatLeft: boolean) => invoke<SocialStatus>("social_set_layout", { chatLeft });
export const socialFixFirewall = () => invoke<SocialStatus>("social_fix_firewall");
// ----- v0.9.0 文件传输 / 群聊 / 广播 / 备注 -----
export const socialPickFile = () => invoke<string | null>("social_pick_file");
export const socialSendFile = (uid: string, path: string, gid?: string) =>
  invoke<XferOut>("social_send_file", { uid, path, gid: gid ?? null });
export const socialFileAccept = (id: string) => invoke<void>("social_file_accept", { id });
export const socialFileReject = (id: string) => invoke<void>("social_file_reject", { id });
export const socialFilePause = (id: string) => invoke<void>("social_file_pause", { id });
export const socialFileResume = (id: string) => invoke<void>("social_file_resume", { id });
export const socialFileCancel = (id: string) => invoke<void>("social_file_cancel", { id });
export const socialFileRetry = (id: string) => invoke<XferOut>("social_file_retry", { id });
export const socialFileReceiveAgain = (id: string) => invoke<void>("social_file_receive_again", { id });
export const socialBroadcast = (uids: string[], text: string) => invoke<BcastOut[]>("social_broadcast", { uids, text });
export const socialSetRemark = (uid: string, remark: string) => invoke<void>("social_set_remark", { uid, remark });
export const socialCreateGroup = (name: string, uids: string[]) => invoke<GroupInfo>("social_create_group", { name, uids });
export const socialGroupSend = (gid: string, text: string) => invoke<{ ts: number }>("social_group_send", { gid, text });
export const socialGroupInvite = (gid: string, uids: string[]) => invoke<GroupInfo>("social_group_invite", { gid, uids });
export const socialGroupKick = (gid: string, uid: string) => invoke<GroupInfo>("social_group_kick", { gid, uid });
export const socialGroupSetAdmin = (gid: string, uid: string, admin: boolean) =>
  invoke<GroupInfo>("social_group_set_admin", { gid, uid, admin });
export const socialGroupRename = (gid: string, name: string) => invoke<GroupInfo>("social_group_rename", { gid, name });
export const socialGroupLeave = (gid: string) => invoke<void>("social_group_leave", { gid });
export const socialGroupDissolve = (gid: string) => invoke<void>("social_group_dissolve", { gid });
export const socialGroupNudge = (gid: string) => invoke<void>("social_group_nudge", { gid });
export const socialShake = (uid: string) => invoke<void>("social_shake", { uid });
// v0.9.23 开放推送接口
export interface ApiPush { ts: number; title: string; body: string; }
export interface ApiState { enabled: boolean; key: string; port: number; recent: ApiPush[]; }
export const apiKeyGen = () => invoke<ApiState>("api_key_gen");
export const apiKeyClear = () => invoke<ApiState>("api_key_clear");
export const apiState = () => invoke<ApiState>("api_state");
export const socialMarkRead = (uid: string, upTo: number) => invoke<void>("social_mark_read", { uid, upTo });
export const socialReadAll = () => invoke<Record<string, number>>("social_read_all");
export const socialGroupFiles = (gid: string) => invoke<GroupFile[]>("social_group_files", { gid });
// ----- v0.9.2 社交功能设置 / 打开文件 -----
export interface SocialDirs { recv: string; custom: string; chat: string; zombie: boolean; font: number; }
export const socialDirs = () => invoke<SocialDirs>("social_dirs");
export const socialSetRecvDir = (path: string) => invoke<SocialDirs>("social_set_recv_dir", { path });
export const socialPickDir = () => invoke<string | null>("social_pick_dir");
export const socialOpenPath = (path: string) => invoke<void>("social_open_path", { path });
export const socialRevealPath = (path: string) => invoke<void>("social_reveal_path", { path });
export const socialRemovePeer = (uid: string) => invoke<void>("social_remove_peer", { uid });
export const socialSetZombie = (on: boolean) => invoke<SocialDirs>("social_set_zombie", { on });
export const socialSetFontSize = (size: number) => invoke<SocialDirs>("social_set_font_size", { size });
export const socialClearHistory = (key: string) => invoke<void>("social_clear_history", { key });
export interface DataUsage { msgs_bytes: number; files_bytes: number; files_count: number; }
export const socialDataUsage = () => invoke<DataUsage>("social_data_usage");
export interface DirEntryOut { name: string; is_dir: boolean; size: number; mtime: number; }
export interface DirListing { path: string; parent: string | null; entries: DirEntryOut[]; sel?: string | null; }
export const socialListDir = (path: string) => invoke<DirListing>("social_list_dir", { path });
export const socialClearAllHistory = () => invoke<void>("social_clear_all_history");
export const socialClearRecvFiles = () => invoke<[number, number]>("social_clear_recv_files");
export const socialClearChatFiles = (key: string) => invoke<number>("social_clear_chat_files", { key });
