<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { flip } from "svelte/animate";
  import { fade } from "svelte/transition";
  import { view, toasts, toast, confirmBox, askConfirm, dropQueue, ctxMenu, closeCtx, appPaused, pauseOnBlurEnabled, unlockRequest, pendingAlertDetail, pendingSocialOpen, focusMode, apiEnabled, type View } from "./lib/stores";
import { T } from "./lib/i18n";
  import { invoke } from "@tauri-apps/api/core";
  import { isElevated } from "./lib/api";
  import type { AlertInfo } from "./lib/api";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import Processes from "./lib/Processes.svelte";
  import Services from "./lib/Services.svelte";
  import Startup from "./lib/Startup.svelte";
  import Tasks from "./lib/Tasks.svelte";
  import FileUnlock from "./lib/FileUnlock.svelte";
  import Codec from "./lib/Codec.svelte";
  import Extract from "./lib/Extract.svelte";
  import Optimize from "./lib/Optimize.svelte";
  import Social from "./lib/Social.svelte";
  import Network from "./lib/Network.svelte";
  import Settings from "./lib/Settings.svelte";
  import ApiView from "./lib/ApiView.svelte";
  import FloatDock from "./lib/FloatDock.svelte";
  import Home from "./lib/Home.svelte";
  import Events from "./lib/Events.svelte";
  import Alerts from "./lib/Alerts.svelte";
  import NotifierWindow from "./lib/NotifierWindow.svelte";
  import { BUILD_STAMP, APP_VERSION } from "./lib/build-stamp";

  // 浮标/气泡窗口加载同一入口，按窗口 label 识别（不依赖 URL 参数）
  const isFloat = getCurrentWindow().label === "float";
  const isNotifier = getCurrentWindow().label === "notifier";
  const isMain = !isFloat && !isNotifier; // 主窗口（唯一带左侧菜单的壳层）

  // v0.9.5 导航栏：QQ/Telegram 式窄栏（默认 64px 图标栏），拖右缘把手可加宽（>112px 显示文字）
  // 并持久化；聊天模式折叠时把手一并隐藏
  $: navHidden = isMain && $focusMode && $view === "social";
  let navW = 64;
  let navDragging = false;
  {
    const v = Number(localStorage.getItem("tz-nav-w"));
    if (v >= 56 && v <= 280) navW = v;
  }
  function navGripDown(e: MouseEvent) {
    e.preventDefault();
    navDragging = true;
    const move = (ev: MouseEvent) => {
      navW = Math.max(56, Math.min(280, Math.round(ev.clientX - 10)));
    };
    const up = () => {
      navDragging = false;
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      try { localStorage.setItem("tz-nav-w", String(navW)); } catch {}
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }

  const win = getCurrentWindow();
  let admin = false;

  // v0.9.6 启动欢迎画面：上次是聊天模式 → 首帧即遮罩（品牌瞬间 2.2 秒）。
  // 这 2 秒不是白等：社交页在闪屏之下同步挂载，雷达发现与历史联系人恢复并行完成，
  // 闪屏揭开的瞬间联系人名单已经就绪 —— 雷达在后台早已干完活
  const startInFocus = isMain && localStorage.getItem("tz-ui-focus") === "1";
  let splash = startInFocus;
  if (startInFocus) view.set("social");

  const _navBase = [
    { id: "home", icon: "⌂", label: "简介" },
    { id: "processes", icon: "◈", label: "进程" },
    { id: "services", icon: "⚙", label: "服务" },
    { id: "startup", icon: "⏻", label: "启动项" },
    { id: "tasks", icon: "⏱", label: "计划任务" },
    { id: "network", icon: "🌐", label: "网络监控" },
    { id: "events", icon: "📜", label: "事件日志" },
    { id: "alerts", icon: "⚠", label: "威胁预警" },
    { id: "unlock", icon: "🔓", label: "文件解锁" },
    { id: "codec", icon: "⇆", label: "编解码" },
    { id: "extract", icon: "⌗", label: "数据提取" },
    { id: "optimize", icon: "⚡", label: "系统优化" },
    { id: "social", icon: "💬", label: "社交" }
  ] as const;

  // v0.9.6 导航自定义排序：长按 350ms 激活（iOS 同款），移入相邻项实时换位（FLIP 弹性动画），
  // 松手 2 秒自动保存；新版本新增菜单项自动补尾、未知项剔除
  type NavItem = { id: View; icon: string; label: string };
  const NAV_BASE: NavItem[] = _navBase.map((n) => ({ ...n }));
  function loadNav(): NavItem[] {
    try {
      const saved = JSON.parse(localStorage.getItem("tz-nav-order") || "[]") as string[];
      if (Array.isArray(saved) && saved.length) {
        const byId = new Map(NAV_BASE.map((n) => [n.id, n]));
        const out: NavItem[] = [];
        for (const id of saved) {
          const n = byId.get(id as View);
          if (n && !out.includes(n)) out.push(n);
        }
        for (const n of NAV_BASE) if (!out.includes(n)) out.push(n);
        return out;
      }
    } catch {}
    return [...NAV_BASE];
  }
  let navItems = loadNav();
  let dragIdx = -1;
  let dragArmed = false;
  let didDrag = false;
  let pressTimer: ReturnType<typeof setTimeout> | null = null;
  let saveTimer: ReturnType<typeof setTimeout> | null = null;
  function navPress(e: MouseEvent, i: number) {
    if (e.button !== 0) return;
    if (pressTimer) clearTimeout(pressTimer);
    pressTimer = setTimeout(() => {
      dragIdx = i;
      dragArmed = true; // 长按达成 → 项浮起，进入排序态
      didDrag = false;
    }, 350);
  }
  function navEnter(i: number) {
    if (!dragArmed || dragIdx < 0 || dragIdx === i) return;
    const [item] = navItems.splice(dragIdx, 1);
    navItems.splice(i, 0, item);
    dragIdx = i;
    didDrag = true;
    navItems = navItems;
  }
  function navRelease() {
    if (pressTimer) { clearTimeout(pressTimer); pressTimer = null; }
    if (!dragArmed) return;
    dragArmed = false;
    dragIdx = -1;
    if (didDrag) {
      if (saveTimer) clearTimeout(saveTimer);
      saveTimer = setTimeout(() => {
        try { localStorage.setItem("tz-nav-order", JSON.stringify(navItems.map((n) => n.id))); } catch {}
      }, 2000); // 放置后两秒自动保存（再次拖动则顺延）
      setTimeout(() => (didDrag = false), 60);
    }
  }
  function navClick(id: View) {
    if (didDrag) { didDrag = false; return; } // 拖拽落点不触发导航
    view.set(id);
  }

  // v0.9.6 聊天模式顶部标语：主标语开场，10 秒一轮循环（只换副标题，主名恒定）
  const SLOGANS = [
    "连接美好，自在沟通。",
    "言出由心，阅后即安。",
    "安全，是沟通的底线。",
    "懂你的人，都在圈里。",
    "让沟通回归纯粹。"
  ];
  let sloganIdx = 0;
  let sloganTimer: ReturnType<typeof setInterval> | null = null;
  $: chatModeOn = isMain && $focusMode && $view === "social";
  $: if (chatModeOn) startSlogans(); else stopSlogans();
  function startSlogans() {
    if (sloganTimer) return;
    sloganIdx = 0;
    sloganTimer = setInterval(() => (sloganIdx = (sloganIdx + 1) % SLOGANS.length), 10000);
  }
  function stopSlogans() {
    if (sloganTimer) { clearInterval(sloganTimer); sloganTimer = null; }
  }
  onDestroy(stopSlogans);

  // v0.9.10 阅读即停：打开社交面板 = 开始查看消息，托盘闪烁立刻熄灭
  $: if ($view === "social") invoke("social_flash_stop").catch(() => {});

  // 全局右键：非文本输入区域禁用 WebView2 默认菜单（页面行右键已 stopPropagation）
  function globalCtx(e: MouseEvent) {
    const t = e.target as HTMLElement;
    const editing = t instanceof HTMLInputElement || t instanceof HTMLTextAreaElement;
    if (!editing) e.preventDefault();
    closeCtx();
  }
  // 上下文菜单位置修正（避免超出窗口）
  $: ctxPos = (() => {
    const m = $ctxMenu;
    if (!m) return { left: 0, top: 0 };
    const W = 180, H = Math.min(m.items.length * 34 + 8, 400);
    return {
      left: Math.min(m.x, window.innerWidth - W - 8),
      top: Math.min(m.y, window.innerHeight - H - 8)
    };
  })();

  onMount(async () => {
    try { admin = await isElevated(); } catch { admin = false; }
    // v0.9.25 API 启用态初载（导航条件项显隐；此后由 ApiView 开关即时同步）
    try {
      const { apiState } = await import("./lib/api");
      apiEnabled.set((await apiState()).enabled);
    } catch {}
    // v0.9.6 闪屏 2.2 秒后揭开：期间社交页已完成挂载 + 雷达发现（后台早已干完活）
    if (splash) setTimeout(() => (splash = false), 2200);
    // v0.9.10 焦点模式兜底：挂载后再次确认落在社交面板（上次是聊天模式）
    if (isMain && $focusMode && $view !== "social") view.set("social");
    // 后台线程通知（如双击 Ctrl 的捕获/失败诊断）
    try {
      const { listen } = await import("@tauri-apps/api/event");
      await listen<string>("app-toast", (ev) => toast(ev.payload, "info"));
      // 右键菜单「强制解锁/删除 - 青刃」直达
      await listen<string>("unlock-request", (ev) => {
        unlockRequest.set(ev.payload);
        view.set("unlock");
      });
      // v0.9.0 一键提醒：自动弹出工具面板到社交界面
      if (!isFloat && !isNotifier) {
        await listen("goto-social", () => view.set("social"));
      }
      // 气泡窗口「点击 → 主窗口威胁预警中心」（携带告警时直接打开详情）
      if (isNotifier || !isFloat) {
        await listen("goto-alerts", () => view.set("alerts"));
        await listen<AlertInfo>("goto-alert", (ev) => {
          const p = ev.payload;
          // v0.9.2 社交气泡与预警同一通道：kind=social → 社交板块并自动打开对应会话
          if (p?.kind === "social") {
            view.set("social");
            pendingSocialOpen.set(p.ctx ?? null);
          } else {
            view.set("alerts");
            if (p) pendingAlertDetail.set(p);
          }
        });
        // v0.9.3 托盘闪烁点击直达：设置待打开的会话上下文
        await listen<string>("social-open-ctx", (ev) => {
          pendingSocialOpen.set(ev.payload);
        });
        // v0.9.2 托盘退出确认（社交激活后关窗驻留，退出必须用户点头）
        await listen("app-exit-request", async () => {
          const ok = await askConfirm($T("app.exit.confirm"));
          if (ok) {
            const { invoke } = await import("@tauri-apps/api/core");
            invoke("app_exit_now").catch(() => {});
          }
        });
      }
    } catch {}
    try {
      const { settingsGet } = await import("./lib/api");
      pauseOnBlurEnabled.set((await settingsGet()).pause_on_blur);
    } catch {}
    // 拖放监听在整个应用生命周期只注册一次（App 级），不随页面切换反复注册/注销
    try {
      await getCurrentWebview().onDragDropEvent((ev: any) => {
        if (ev?.payload?.type === "drop") {
          const ps: string[] = ev.payload.paths ?? [];
          if (ps.length) dropQueue.update((q) => [...q, ...ps]);
        }
      });
    } catch (e) {
      console.error("拖放监听注册失败", e);
    }
  });

  function closeConfirm(v: boolean) {
    const box = $confirmBox;
    confirmBox.set(null);
    box?.resolve(v);
  }
</script>

{#if isFloat}
  <FloatDock />
{:else if isNotifier}
  <NotifierWindow />
{:else}
<div class="shell">
  <div class="titlebar" data-tauri-drag-region>
    <div class="brand" data-tauri-drag-region><b>青刃 TexMantis</b>
      {#if chatModeOn}
        {#key sloganIdx}<span class="dim slogan" in:fade={{ duration: 420 }}>{SLOGANS[sloganIdx]}</span>{/key}
      {:else}
        <span class="dim">{$T("app.tagline")}</span>
      {/if}
      <span class="stamp">v{APP_VERSION} · build {BUILD_STAMP}</span></div>
    <div class="lights">
      <button class="light" title={$T("app.btn.api")} on:click={() => view.set($view === "api" ? "processes" : "api")}>
        <svg width="13" height="13" viewBox="0 0 16 16" fill="none">
          <path d="M8 1.8a4.6 4.6 0 0 0-4.6 4.6c0 2.2-.5 3.4-1.1 4.3-.3.4 0 1 .5 1h10.4c.5 0 .8-.6.5-1-.6-.9-1.1-2.1-1.1-4.3A4.6 4.6 0 0 0 8 1.8Z" stroke="currentColor" stroke-width="1.25" stroke-linejoin="round"/>
          <path d="M6.6 13.6a1.5 1.5 0 0 0 2.8 0" stroke="currentColor" stroke-width="1.25" stroke-linecap="round"/>
        </svg>
      </button>
      <button class="light" title={$T("app.btn.settings")} on:click={() => view.set($view === "settings" ? "processes" : "settings")}>
        <svg width="13" height="13" viewBox="0 0 16 16" fill="none">
          <path d="M8 10.2a2.2 2.2 0 1 0 0-4.4 2.2 2.2 0 0 0 0 4.4Z" stroke="currentColor" stroke-width="1.3"/>
          <path d="M13.3 8c0-.4.5-.8.5-1.2 0-.4-.6-.7-.7-1.1-.1-.4.2-1-.1-1.3-.3-.3-.9 0-1.3-.1-.4-.1-.7-.7-1.1-.7-.4 0-.8.5-1.2.5h-.8C8.2 4.1 7.8 3.6 7.4 3.6c-.4 0-.7.6-1.1.7-.4.1-1-.2-1.3.1-.3.3 0 .9-.1 1.3-.1.4-.7.7-.7 1.1 0 .4.5.8.5 1.2s-.5.8-.5 1.2c0 .4.6.7.7 1.1.1.4-.2 1 .1 1.3.3.3.9 0 1.3.1.4.1.7.7 1.1.7.4 0 .8-.5 1.2-.5h.8c.4 0 .8.5 1.2.5.4 0 .7-.6 1.1-.7.4-.1 1 .2 1.3-.1.3-.3 0-.9.1-1.3.1-.4.7-.7.7-1.1 0-.4-.5-.8-.5-1.2Z" stroke="currentColor" stroke-width="1.2"/>
        </svg>
      </button>
      <button class="light" title={$T("app.btn.minimize")} on:click={() => win.minimize()}>
        <svg width="11" height="11" viewBox="0 0 12 12"><path d="M1.5 6h9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>
      </button>
      <button class="light" title={$T("app.btn.maximize")} on:click={() => win.toggleMaximize()}>
        <svg width="11" height="11" viewBox="0 0 12 12"><rect x="2" y="2" width="8" height="8" rx="1.4" stroke="currentColor" stroke-width="1.4" fill="none"/></svg>
      </button>
      <button class="light close" title={$T("app.btn.close")} on:click={() => win.close()}>
        <svg width="11" height="11" viewBox="0 0 12 12"><path d="M2.2 2.2l7.6 7.6M9.8 2.2l-7.6 7.6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>
      </button>
    </div>
  </div>

  <div class="body">
    <nav class="side" class:nav-hidden={navHidden}
      class:compact={navW < 112} class:dragging={navDragging} class:ordering={dragArmed}
      style="width:{navHidden ? 0 : navW}px">
      {#each navItems as n, i (n.id)}
        <button type="button" class="nav-item" class:active={$view === n.id}
          class:lift={dragArmed && dragIdx === i} title={n.label}
          on:click={() => navClick(n.id)}
          on:mousedown={(e) => navPress(e, i)}
          on:mouseenter={() => navEnter(i)}
          animate:flip={{ duration: 200 }}>
          <span class="icon">{n.icon}</span><span class="lbl">{$T(`nav.${n.id}`)}</span>
        </button>
      {/each}
      {#if isMain && $apiEnabled}
        <button type="button" class="nav-item" class:active={$view === "api"} title={$T("app.btn.api")}
          on:click={() => navClick("api")}>
          <span class="icon">🔔</span><span class="lbl">{$T("nav.api")}</span>
        </button>
      {/if}
      <div class="spacer"></div>
      <div class="badge {admin ? 'ok' : 'warn'}">{admin ? $T("app.badge.admin") : $T("app.badge.user")}</div>
      {#if isMain && !($focusMode && $view === "social")}
        <div class="nav-grip" role="presentation" title="拖动调整导航宽度" on:mousedown={navGripDown}></div>
      {/if}
    </nav>

    <main class="content">
      {#if !admin}
        <div class="banner banner-warn">{$T("app.banner.nonAdmin")}</div>
      {/if}
      {#if $view === "home"}
        <Home />
      {:else if $view === "processes"}
        <Processes />
      {:else if $view === "services"}
        <Services />
      {:else if $view === "startup"}
        <Startup />
      {:else if $view === "tasks"}
        <Tasks />
      {:else if $view === "unlock"}
        <FileUnlock />
      {:else if $view === "codec"}
        <Codec />
      {:else if $view === "extract"}
        <Extract />
      {:else if $view === "optimize"}
        <Optimize />
      {:else if $view === "social"}
        <Social />
      {:else if $view === "api"}
        <ApiView />
      {:else if $view === "events"}
      <Events />
    {:else if $view === "alerts"}
      <Alerts />
    {:else if $view === "network"}
        <Network />
      {:else if $view === "settings"}
        <Settings />
      {/if}
    </main>
  </div>

  <div class="toasts">
    {#each $toasts as t (t.id)}
      <div class="toast {t.kind}">{t.text}</div>
    {/each}
  </div>



  {#if $confirmBox}
    <div class="modal-mask">
      <div class="modal">
        <div class="modal-title">{$confirmBox.danger ? $T("app.confirm.danger") : $T("app.confirm.title")}</div>
        <div class="modal-text">{$confirmBox.text}</div>
        <div class="modal-actions">
          <button type="button" class="btn" on:click={() => closeConfirm(false)}>{$T("app.confirm.cancel")}</button>
          <button type="button" class="btn {$confirmBox.danger ? 'danger' : 'primary'}" on:click={() => closeConfirm(true)}>{$T("app.confirm.ok")}</button>
        </div>
      </div>
    </div>
  {/if}

  {#if $ctxMenu}
    <div class="ctx" style="left:{ctxPos.left}px; top:{ctxPos.top}px" on:click|stopPropagation on:contextmenu|stopPropagation|preventDefault>
      {#each $ctxMenu.items as it, i (i)}
        <button type="button" class="ctx-item" class:danger={it.danger}
          on:click|stopPropagation={() => { closeCtx(); it.action(); }}>{it.label}</button>
      {/each}
    </div>
  {/if}
</div>
{/if}

<svelte:window
  on:click={() => closeCtx()}
  on:contextmenu={globalCtx}
  on:keydown={(e) => e.key === "Escape" && closeCtx()}
  on:focus={() => appPaused.set(false)}
  on:blur={() => { if ($pauseOnBlurEnabled) appPaused.set(true); }}
  on:mouseup={() => navRelease()}
  on:visibilitychange={() => {
    // v0.9.10 从托盘/最小化恢复 = "打开工具"：上次是聊天模式 → 默认落在社交面板
    if (document.visibilityState === "visible" && isMain && $focusMode && $view !== "social") {
      view.set("social");
    }
  }} />

<!-- v0.9.6 启动欢迎画面：logo（与托盘徽章同源的黄色气泡）+ 名称 + 主标语；
     2.2 秒的停留正是雷达后台发现同伴的窗口，揭开即见已就绪的联系人列表 -->
{#if splash}
  <div class="splash" transition:fade={{ duration: 480 }}>
    <svg class="s-logo" width="84" height="84" viewBox="0 0 32 32" aria-hidden="true">
      <defs>
        <linearGradient id="spg" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stop-color="#ffd24d" />
          <stop offset="1" stop-color="#ffc914" />
        </linearGradient>
      </defs>
      <circle cx="16" cy="16" r="15" fill="url(#spg)" />
      <rect x="8" y="9" width="16" height="11" rx="3.5" fill="#fff" />
      <path d="M10.5 19.5 L8.5 24 L13.5 20.5 Z" fill="#fff" />
      <circle cx="12.5" cy="14.5" r="1.5" fill="#ffc914" />
      <circle cx="16" cy="14.5" r="1.5" fill="#ffc914" />
      <circle cx="19.5" cy="14.5" r="1.5" fill="#ffc914" />
    </svg>
    <div class="s-name">青刃 TexMantis</div>
    <div class="s-tag">{$T("splash.tagline")}</div>
  </div>
{/if}
