import { get, writable } from "svelte/store";
import type { AlertInfo } from "./api";

export type View = "home" | "processes" | "services" | "startup" | "tasks" | "unlock" | "codec" | "extract" | "optimize" | "network" | "events" | "alerts" | "settings" | "social" | "api";

// v0.9.24 开放推送接口启用态（社交设置与 App 壳层共享：右上角按钮 / 导航项条件显示）
export const apiEnabled = writable(false);

// 资源占用策略：窗口失焦/最小化时自动暂停所有页面轮询（设置页可关）
export const appPaused = writable(false);
export const pauseOnBlurEnabled = writable(true);
export const view = writable<View>("home");

export interface Toast { id: number; text: string; kind: "ok" | "err" | "info"; }
export const toasts = writable<Toast[]>([]);

let tid = 0;
export function toast(text: string, kind: Toast["kind"] = "info") {
  const id = ++tid;
  toasts.update((t) => [...t, { id, text, kind }]);
  setTimeout(() => toasts.update((t) => t.filter((x) => x.id !== id)), 3600);
}

// ---------------------------------------------------------------------------
// 应用内确认对话框（替代 window.confirm —— WebView2 原生脚本对话框在
// 无边框透明窗口下可能不可见并同步阻塞 JS 主线程，导致整个 UI 冻结）
// ---------------------------------------------------------------------------
export interface ConfirmState { text: string; danger?: boolean; resolve: (v: boolean) => void; }
export const confirmBox = writable<ConfirmState | null>(null);
export function askConfirm(text: string, danger = false): Promise<boolean> {
  return new Promise((resolve) => confirmBox.set({ text, danger, resolve }));
}

// ---------------------------------------------------------------------------
// 拖放路径队列：App 级监听一次拖放事件，页面通过订阅消费（组件切换零监听器变动）
// ---------------------------------------------------------------------------
export const dropQueue = writable<string[]>([]);
// v0.9.2 气泡点击 → 社交板块自动打开对应会话（"peer:UID" / "group:GID"）
export const pendingSocialOpen = writable<string | null>(null);

// v0.9.4 社交专注模式（聊天模式）：隐藏全局左侧菜单，社交全宽展示。
// 持久化到 localStorage —— 下次启动仍进入上次的模式
// v0.9.5 聊天字号（px）——社交设置实时预览 + 全聊天即时生效；持久化在后端 config
export const chatFontSize = writable<number>(13);

export const focusMode = writable<boolean>(
  typeof localStorage !== "undefined" && localStorage.getItem("tz-ui-focus") === "1"
);
focusMode.subscribe((v) => {
  try { localStorage.setItem("tz-ui-focus", v ? "1" : "0"); } catch {}
});

// ---------------------------------------------------------------------------
// 应用内右键菜单（替代 WebView2 默认浏览器右键菜单）
// ---------------------------------------------------------------------------
export interface CtxItem { label: string; action: () => void; danger?: boolean; }
export interface CtxState { x: number; y: number; items: CtxItem[]; }
export const ctxMenu = writable<CtxState | null>(null);
export function openCtx(x: number, y: number, items: CtxItem[]) {
  ctxMenu.set({ x, y, items });
}
export function closeCtx() {
  ctxMenu.set(null);
}

// 行右键统一入口：阻止默认浏览器菜单 + 打开应用内菜单
export function rowCtx(e: MouseEvent, items: CtxItem[]) {
  e.preventDefault();
  e.stopPropagation();
  openCtx(e.clientX, e.clientY, items);
}

export async function copyText(s: string) {
  if (!s) return;
  try {
    await navigator.clipboard.writeText(s);
    toast("已复制到剪贴板", "ok");
  } catch {
    toast("复制失败", "err");
  }
}

// 右键菜单「强制解锁/删除 - 青刃」直达请求
export const unlockRequest = writable<string | null>(null);

// 气泡点击 → 告警详情直达（预警页订阅消费，消费后清空）
export const pendingAlertDetail = writable<AlertInfo | null>(null);

// ---------------------------------------------------------------------------
// 威胁预警气泡（右下角方框，瑞星式简约风格）
// ---------------------------------------------------------------------------
