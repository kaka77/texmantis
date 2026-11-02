<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { invoke } from "@tauri-apps/api/core";
  import type { AlertInfo } from "./api";

  // 系统右下角透明气泡窗口：贴屏幕工作区（托盘正上方），新告警从底部推入、旧的向上顶起。
  // 窗口背景全透明（卡片自绘半透明），空白区域不可见 —— 无白底残留；
  // 窗口高度动态贴合内容仅为缩小点击热区，贴合失败也无害。
  type Item = AlertInfo & { nid: number }; // 本地气泡键（避开 AlertInfo.id 字符串唯一键）
  let items: Item[] = [];
  let seq = 0;
  let contentH = 0;
  let lastFit = 0;
  const win = getCurrentWindow();
  let unlisten: (() => void) | null = null;
  let unlistenClear: (() => void) | null = null;
  let onVis: (() => void) | null = null;
  const timers = new Map<number, ReturnType<typeof setTimeout>>();

  // 合成提示音（WebAudio，零资源文件）：高风险双音上扬 / 中风险单音
  function playDing(level: number) {
    try {
      const AC = window.AudioContext || (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
      if (!AC) return;
      const ctx = new AC();
      if (ctx.state === "suspended") ctx.resume().catch(() => {});
      const t0 = ctx.currentTime + 0.01;
      const notes = level >= 3 ? [1318.5, 1760] : [1046.5]; // 高风险 E6→A6 / 中风险 C6
      notes.forEach((f, i) => {
        const o = ctx.createOscillator();
        const g = ctx.createGain();
        o.type = "sine";
        o.frequency.value = f;
        const st = t0 + i * 0.13;
        g.gain.setValueAtTime(0.0001, st);
        g.gain.linearRampToValueAtTime(0.14, st + 0.02);
        g.gain.exponentialRampToValueAtTime(0.0001, st + 0.5);
        o.connect(g);
        g.connect(ctx.destination);
        o.start(st);
        o.stop(st + 0.55);
      });
      setTimeout(() => { ctx.close().catch(() => {}); }, 1500);
    } catch {}
  }

  function push(a: AlertInfo) {
    // 去重（实时监听与兜底拉取可能重叠）
    if (items.some((x) => x.ts_ms === a.ts_ms && x.rule === a.rule && x.title === a.title)) return;
    const nid = ++seq;
    items = [...items.slice(-2), { ...a, nid }]; // 最多 3 条，最新在底部
    playDing(a.level);
    timers.set(nid, setTimeout(() => drop(nid), 8000));
  }
  function drop(nid: number) {
    timers.delete(nid);
    items = items.filter((x) => x.nid !== nid);
    if (!items.length) { lastFit = 0; win.hide().catch(() => {}); }
  }
  function clearLocal() {
    timers.forEach((t) => clearTimeout(t));
    timers.clear();
    items = [];
    lastFit = 0;
  }
  async function muteAll() {
    try { await invoke("alert_mute_set", { on: true }); } catch {}
    clearLocal();
    await win.hide().catch(() => {});
  }
  async function openMain(it?: Item) {
    try { await invoke("notifier_open_main", it ? { alert: { ...it } } : {}); } catch {}
    clearLocal();
    await win.hide().catch(() => {});
  }

  // 窗口高度贴合内容：测量列高 → 通知后端调整窗口并保持底部锚定
  $: fitHeight(items, contentH);
  function fitHeight(list: Item[], h: number) {
    if (!list.length || !h) return;
    const target = Math.round(h);
    if (Math.abs(target - lastFit) < 6) return;
    lastFit = target;
    invoke("notifier_fit", { height: target }).catch(() => {});
  }

  // 兜底拉取：窗口每次由隐藏转可见时，主动补拉队列（防后台节流/事件竞态漏单）
  async function pullPending() {
    try {
      const p: AlertInfo[] = await invoke("notifier_take_pending");
      p.forEach(push);
    } catch {}
  }

  onMount(async () => {
    try {
      const { listen } = await import("@tauri-apps/api/event");
      await pullPending();
      unlisten = await listen<AlertInfo>("threat-alert", (ev) => push(ev.payload));
      unlistenClear = await listen("threat-clear", clearLocal);
      document.addEventListener("visibilitychange", onVis = () => {
        if (document.visibilityState === "visible") pullPending();
      });
    } catch {}
  });
  onDestroy(() => {
    unlisten?.(); unlistenClear?.();
    if (onVis) document.removeEventListener("visibilitychange", onVis);
    timers.forEach((t) => clearTimeout(t));
  });

  const fmtT = (ms: number) => new Date(ms).toLocaleTimeString("zh-CN", { hour12: false });
</script>

<div class="col" bind:clientHeight={contentH}>
  {#each items as t (t.nid)}
    <div class="tbox" class:mid={t.level < 3 && t.kind !== "social"} class:soc={t.kind === "social"} role="button" tabindex="0"
      title={t.kind === "social" ? "点击打开社交聊天" : "点击打开威胁预警中心"}
      on:click={() => openMain(t)} on:keydown={(e) => e.key === "Enter" && openMain(t)}>
      {#if t.kind === "social"}
        <!-- v0.9.2 社交消息样式：聊天气泡观感，与预警卡明显区分 -->
        <div class="thead">
          <span class="tlv soc">💬 消息</span>
          <b>{t.title}</b>
        </div>
        <div class="tdesc">{t.desc}</div>
        <div class="tfoot">
          <span class="ttime">{fmtT(t.ts_ms)}</span>
          <span class="topen">打开 ›</span>
        </div>
      {:else}
        <div class="thead">
          <span class="tlv">{t.level >= 3 ? "高风险" : "中风险"}</span>
          <b>{t.title}</b>
          {#if t.mitre}<span class="tmitre">{t.mitre}</span>{/if}
        </div>
        <div class="tdesc">{t.desc}</div>
        <div class="tdet" title={t.detail}>{t.detail}</div>
        <div class="tfoot">
          <span class="ttime">{fmtT(t.ts_ms)}</span>
          <button class="tmute" title="停止气泡通知；告警记录不受影响，可在威胁预警页随时开启"
            on:click|stopPropagation={muteAll}>免打扰</button>
        </div>
      {/if}
    </div>
  {/each}
</div>

<style>
  :global(html, body) { background: transparent; margin: 0; overflow: hidden; }
  .col { position: fixed; left: 0; right: 0; bottom: 0; display: flex; flex-direction: column; gap: 8px; }
  .tbox { background: rgba(255, 255, 255, 0.95); border: 1px solid rgba(15, 23, 42, 0.1); border-left: 3px solid #e11d48; border-radius: 12px; padding: 10px 12px 4px; cursor: pointer; animation: tin 0.24s ease-out; }
  .tbox.mid { border-left-color: #d97706; }
  /* 社交消息卡：iMessage 蓝系，点击直达聊天 */
  .tbox.soc { border-left-color: #007aff; }
  .tlv.soc { background: rgba(234, 243, 255, 0.95); color: #007aff; }
  .topen { font-size: 11.5px; color: #007aff; font-weight: 700; padding: 2px 8px; border-radius: 6px; }
  .tbox.soc:hover .topen { background: rgba(0, 122, 255, 0.1); }
  .tbox:hover { background: rgba(255, 255, 255, 0.99); }
  .thead { display: flex; align-items: center; gap: 7px; font-size: 13px; color: #0f172a; }
  .tlv { flex: none; font-size: 10.5px; padding: 1px 8px; border-radius: 999px; background: rgba(255, 241, 242, 0.95); color: #e11d48; font-weight: 700; }
  .tbox.mid .tlv { background: rgba(255, 251, 235, 0.95); color: #b45309; }
  .tmitre { flex: none; margin-left: auto; font-size: 10.5px; font-family: var(--mono, monospace); color: #0369a1; background: rgba(240, 249, 255, 0.95); padding: 1px 7px; border-radius: 5px; }
  .tdesc { font-size: 12.5px; color: #475569; margin-top: 4px; line-height: 1.55; }
  .tdet { font-size: 11px; color: #94a3b8; margin-top: 2px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .tfoot { display: flex; align-items: center; justify-content: flex-end; gap: 6px; margin-top: 4px; }
  .ttime { font-size: 10.5px; color: #cbd5e1; margin-right: auto; font-family: var(--mono, monospace); }
  .tmute { border: none; background: transparent; color: var(--text-dim); font-weight: 700; font-size: 11.5px; cursor: pointer; padding: 2px 8px; border-radius: 6px; }
  .tmute:hover { color: var(--accent); background: var(--accent-bg); }
  @keyframes tin { from { transform: translateY(24px); opacity: 0; } to { transform: none; opacity: 1; } }
</style>
