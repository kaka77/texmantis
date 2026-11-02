<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  let hover = false;
  const win = getCurrentWindow();

  // v0.9.0 新消息提醒：高对比闪烁 + 未读角标 + 系统提示音；点击浮漂即清除
  let flashing = false;
  let unreadCount = 0;
  let unlistenMsg: (() => void) | null = null;
  let unlistenNudge: (() => void) | null = null;
  let unlistenGroup: (() => void) | null = null;
  let flashTimer: ReturnType<typeof setTimeout> | null = null;
  function poke(count = 1) {
    unreadCount += count;
    flashing = false;
    if (flashTimer) clearTimeout(flashTimer);
    // 触发重启动画（class 重挂）：先摘再戴
    requestAnimationFrame(() => (flashing = true));
    flashTimer = setTimeout(() => (flashing = false), 6000);
  }

  // 自愈看门狗：浮漂可见期间低频核对主窗焦点 —— Rust 侧 Focused 事件偶发丢失导致
  // "回焦后浮漂不收起"时兜底隐藏；浮漂隐藏期间定时器停止（零开销）
  let watchdog: ReturnType<typeof setInterval> | null = null;
  let onVis: (() => void) | null = null;

  function startWatch() {
    if (watchdog === null) watchdog = setInterval(checkHide, 900);
  }
  function stopWatch() {
    if (watchdog) {
      clearInterval(watchdog);
      watchdog = null;
    }
  }
  async function checkHide() {
    try {
      const focused = await invoke<boolean>("main_focused");
      if (focused) {
        stopWatch();
        await win.hide();
      }
    } catch {}
  }

  // 透明窗体（浮标窗口不渲染主应用外壳）
  onMount(async () => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
    document.body.style.overflow = "hidden";
    startWatch();
    document.addEventListener("visibilitychange", (onVis = () => (document.visibilityState === "visible" ? startWatch() : stopWatch())));
    try {
      unlistenMsg = await listen("social-msg", () => poke(1));
      unlistenNudge = await listen("social-nudge", () => poke(2));
      unlistenGroup = await listen("social-group-msg", () => poke(1));
    } catch {}
  });
  onDestroy(() => {
    stopWatch();
    if (onVis) document.removeEventListener("visibilitychange", onVis);
    unlistenMsg?.(); unlistenNudge?.(); unlistenGroup?.();
    if (flashTimer) clearTimeout(flashTimer);
  });

  async function summon() {
    flashing = false;
    unreadCount = 0;
    try {
      await invoke("float_summon");
    } catch (e) {
      console.error(e);
    }
  }
</script>

<div class="dock" class:flash={flashing} on:mouseenter={() => (hover = true)} on:mouseleave={() => (hover = false)}
  on:click={summon} role="button" tabindex="0" on:keydown={(e) => e.key === "Enter" && summon()}>
  <div class="aura"></div>
  <!-- 三角迷你 UFO：鼻尖朝左、尾部趴在屏幕右缘 -->
  <svg class="ufo" class:up={hover} viewBox="0 0 96 96" width="44" height="44">
    <defs>
      <linearGradient id="ufobody" x1="0" y1="0" x2="1" y2="0">
        <stop offset="0" stop-color="#17c98d"/>
        <stop offset="1" stop-color="#0a7a5c"/>
      </linearGradient>
    </defs>
    <!-- 三角艇身 -->
    <path d="M8 44 Q5 48 8 52 L66 86 Q76 92 76 81 L76 15 Q76 4 66 10 Z"
      fill="url(#ufobody)" stroke="#7dffd0" stroke-width="3" stroke-linejoin="round"/>
    <!-- 背线高光 -->
    <path d="M12 45 L62 13" stroke="#d7fff0" stroke-width="2.4" stroke-linecap="round" opacity="0.9"/>
    <!-- 座舱呼吸灯 -->
    <circle class="halo" cx="34" cy="48" r="13" fill="#a7ffdc"/>
    <circle class="cockpit" cx="34" cy="48" r="8" fill="#d9fff1"/>
    <!-- 尾部航行灯 -->
    <circle cx="68" cy="26" r="3.2" fill="#eafff6"/>
    <circle cx="71" cy="48" r="3.2" fill="#ffd83d"/>
    <circle cx="68" cy="70" r="3.2" fill="#eafff6"/>
  </svg>
  <div class="brand">青刃</div>
  {#if unreadCount > 0}
    <div class="badge">{unreadCount > 99 ? "99+" : unreadCount}</div>
  {/if}
</div>

<style>
  .dock {
    position: fixed; right: 0; top: 50%; transform: translateY(-50%);
    width: 56px; height: 116px; cursor: pointer; z-index: 9999;
    display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 2px;
    background: #334155;
    border-radius: 12px 0 0 12px;
    border: none; /* v0.9.8 去灯带：整块统一色，不再有近白描边竖线 */
    transition: filter 0.15s ease;
    user-select: none; overflow: visible;
    animation: beacon 7.2s ease-in-out infinite;
  }
  .dock:hover { filter: brightness(1.15); }
  /* v0.9.0 新消息高对比闪烁：橙白强反差快闪（盖过待机 beacon 动画）+ 同步强光晕 */
  .dock.flash {
    animation: msgflash 0.42s steps(1) 14, msgglow 0.84s ease-in-out 7;
  }
  @keyframes msgflash {
    0%, 100% { background-color: #f59e0b; }
    50% { background-color: #ffdf9e; }
  }
  @keyframes msgglow {
    0%, 100% { box-shadow: 0 0 34px 6px rgba(245, 158, 11, 0.95); }
    50% { box-shadow: 0 0 40px 9px rgba(255, 255, 255, 0.95); }
  }
  .badge {
    position: absolute; top: -7px; left: -7px; min-width: 20px; height: 20px;
    padding: 0 5px; border-radius: 999px; background: #ef4444; color: #fff;
    font-size: 11px; font-weight: 800; display: flex; align-items: center; justify-content: center;
    border: 2px solid #ffffff; box-shadow: 0 2px 8px rgba(0, 0, 0, 0.35); z-index: 2;
  }
  /* 整块警灯：深饱和四色实底 + 同族边框 + 同色强外发光同步轮转（零透明、白字恒可读） */
  @keyframes beacon {
    0%, 100% { background-color: #334155; box-shadow: 0 0 22px 2px rgba(148, 163, 184, 0.55); }
    25% { background-color: #b91c1c; box-shadow: 0 0 26px 3px rgba(239, 68, 68, 0.8); }
    50% { background-color: #1d4ed8; box-shadow: 0 0 26px 3px rgba(59, 130, 246, 0.8); }
    75% { background-color: #b45309; box-shadow: 0 0 26px 3px rgba(240, 180, 41, 0.85); }
  }

  .ufo {
    transition: transform 0.18s ease, filter 0.18s ease;
    filter: drop-shadow(0 0 7px rgba(255, 255, 255, 0.35));
  }
  .ufo.up {
    transform: translateX(-4px) scale(1.07);
    filter: drop-shadow(0 0 13px rgba(255, 255, 255, 0.55));
  }
  .cockpit { animation: pulse 2.2s ease-in-out infinite; }
  .halo { animation: halo 2.2s ease-in-out infinite; }
  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.45; }
  }
  @keyframes halo {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 0.12; }
  }
  /* 四色旋转光环：衬在 UFO 背后，模糊柔化，独立呼吸明灭 */
  .aura {
    position: absolute; left: 50%; top: 46%;
    width: 72px; height: 72px; border-radius: 50%;
    background: conic-gradient(#ffffff, #ef4444, #3b82f6, #d4af37, #ffffff);
    filter: blur(12px);
    pointer-events: none;
    animation: aurarot 8s linear infinite, aurabreathe 2.8s ease-in-out infinite;
  }
  @keyframes aurabreathe {
    0%, 100% { opacity: 0.55; }
    50% { opacity: 0.22; }
  }
  .ufo { position: relative; z-index: 1; }
  @keyframes aurarot {
    from { transform: translate(-50%, -50%) rotate(0deg); }
    to { transform: translate(-50%, -50%) rotate(360deg); }
  }
  .brand {
    font-size: 10px; color: #e8edf3; letter-spacing: 4px; text-indent: 4px;
    opacity: 0.85; transition: opacity 0.15s;
  }
  .dock:hover .brand { opacity: 1; }
</style>
