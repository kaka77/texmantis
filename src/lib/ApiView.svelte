<script lang="ts">
  // v0.9.26 API 消息中心：开放推送接口的唯一管理入口（右上角铃铛进入）。
  // 开关 / 密钥 / 用法 / 推送历史全部自洽于此。
  // 样式完全自包含（Svelte 样式按组件作用域，跨组件类名不会生效 —— v0.9.25 排版崩坏的根因）。
  import { onMount, onDestroy } from "svelte";
  import { apiEnabled, toast, askConfirm, copyText as storeCopy } from "./stores";
  import { apiState, apiKeyGen, apiKeyClear, type ApiState } from "./api";

  let st: ApiState | null = null;
  let busy = false;
  let timer: ReturnType<typeof setInterval> | null = null;

  async function refresh() {
    try {
      st = await apiState();
      apiEnabled.set(st.enabled);
    } catch {}
  }

  async function toggleApi(on: boolean) {
    if (busy) return;
    busy = true;
    try {
      st = on ? await apiKeyGen() : await apiKeyClear();
      apiEnabled.set(!!st?.enabled);
      toast(on ? "接口已启用 · 密钥已生成" : "接口已关闭", "ok");
    } catch (e) {
      toast(String(e), "err");
      await refresh();
    } finally {
      busy = false;
    }
  }

  async function regen() {
    if (busy) return;
    busy = true;
    try {
      st = await apiKeyGen();
      apiEnabled.set(true);
      toast("密钥已重新生成（旧密钥立即作废）", "ok");
    } catch (e) {
      toast(String(e), "err");
    } finally {
      busy = false;
    }
  }

  async function disable() {
    if (busy) return;
    if (!(await askConfirm("确定关闭开放推送接口？关闭后脚本推送将被拒绝，当前密钥作废（历史保留）。", true))) return;
    busy = true;
    try {
      st = await apiKeyClear();
      apiEnabled.set(false);
      toast("接口已关闭", "ok");
    } catch (e) {
      toast(String(e), "err");
    } finally {
      busy = false;
    }
  }

  function fmtT(ts: number): string {
    return new Date(ts).toLocaleString("zh-CN", { hour12: false, month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit" });
  }

  onMount(() => {
    refresh();
    timer = setInterval(refresh, 10000);
  });
  onDestroy(() => {
    if (timer) clearInterval(timer);
  });
</script>

<div class="view">
  <!-- 页头 -->
  <div class="page-head">
    <div class="page-title">
      <span class="title-dot"></span>
      <b>API 消息</b>
      <span class="dim page-sub">脚本 · 自动化监控的统一消息入口</span>
    </div>
    <span class="chip" class:on={st?.enabled}>{st?.enabled ? "已启用" : "未启用"}</span>
  </div>

  <!-- 接口控制卡：开关 + 密钥 -->
  <section class="card">
    <div class="ctl">
      <div class="ctl-txt">
        <b>开放推送接口</b>
        <span class="dim">仅本机回环 127.0.0.1 · 脚本推送统一气泡提醒</span>
      </div>
      <input type="checkbox" class="ios-sw" checked={st?.enabled ?? false} disabled={busy}
        title="开启将自动生成 10 位访问密钥" on:change={(e) => toggleApi(e.currentTarget.checked)} />
    </div>

    {#if st?.enabled}
    <div class="fields">
      <div class="field">
        <span class="fkey">地址</span>
        <code class="fval">http://127.0.0.1:{st.port}/push</code>
        <button type="button" class="fbtn" on:click={() => storeCopy(`http://127.0.0.1:${st?.port}/push`)}>复制</button>
      </div>
      <div class="field">
        <span class="fkey">密钥</span>
        <code class="fval">{st.key}</code>
        <button type="button" class="fbtn" on:click={() => st && storeCopy(st.key)}>复制</button>
        <button type="button" class="fbtn" disabled={busy} on:click={regen}>重新生成</button>
      </div>
      <div class="usage dim">GET ?key=密钥&amp;title=标题&amp;body=正文&emsp;·&emsp;POST JSON（&#123;"key","title","body"&#125;）</div>
    </div>
    {/if}
  </section>

  <!-- 推送历史 -->
  {#if st?.enabled}
  <div class="page-head">
    <div class="page-title">
      <span class="title-dot"></span>
      <b>最近推送</b>
      <span class="dim page-sub">{st.recent.length}/10 条</span>
    </div>
  </div>
  <section class="card">
    {#if st.recent.length === 0}
      <div class="empty">
        <div class="empty-ico">🔔</div>
        <div class="empty-t">暂无推送</div>
        <div class="dim empty-d">复制上面的地址，用任意脚本发一条试试。</div>
      </div>
    {:else}
      <div class="msgs">
        {#each st.recent as p (p.ts + p.title)}
          <article class="msg">
            <div class="msg-head">
              <span class="msg-dot"></span>
              <b class="msg-title">{p.title}</b>
              <time class="msg-time">{fmtT(p.ts)}</time>
            </div>
            {#if p.body}
              <div class="msg-body">{p.body}</div>
            {/if}
          </article>
        {/each}
      </div>
    {/if}
  </section>
  {/if}
</div>

<style>
  /* ===== 自包含基础骨架（与其它视图同值，但必须在本组件内声明）===== */
  .view {
    max-width: 780px; width: 100%;
    margin: 0 auto;
    padding: 4px 18px 14px;
    display: flex; flex-direction: column; gap: 10px;
  }
  .page-head { display: flex; align-items: center; gap: 8px; padding: 2px 2px 0; }
  .page-title { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .page-title b { font-size: 13px; color: var(--text); }
  .title-dot { width: 4px; height: 14px; border-radius: 2px; background: var(--accent); flex: none; }
  .page-sub { font-size: 11px; }
  .page-head > .chip { margin-left: auto; }
  .chip { flex: none; font-size: 10.5px; font-weight: 600; color: var(--text-dim); background: rgba(15, 23, 42, 0.06); border-radius: 999px; padding: 3px 11px; }
  .chip.on { color: #ffffff; background: #34c759; }
  .card {
    background: var(--panel); border: 1px solid var(--stroke-soft);
    border-radius: 12px; overflow: hidden;
  }

  /* ===== 接口控制卡 ===== */
  .ctl { display: flex; align-items: center; gap: 12px; padding: 12px 14px; }
  .ctl-txt { flex: 1; display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .ctl-txt b { font-size: 12.5px; color: var(--text); }
  .ctl-txt .dim { font-size: 10.5px; line-height: 1.5; }
  .ctl .ios-sw { width: 40px; height: 24px; flex: none; }
  .fields { display: flex; flex-direction: column; gap: 9px; border-top: 1px solid var(--stroke-soft); padding: 11px 14px 13px; }
  .field { display: flex; align-items: center; gap: 8px; min-width: 0; }
  .fkey { flex: none; width: 34px; font-size: 11.5px; color: var(--text-dim); font-weight: 600; }
  .fval {
    flex: 1; min-width: 0;
    font-family: ui-monospace, "Cascadia Mono", Consolas, monospace;
    font-size: 11.5px; color: var(--text);
    background: rgba(15, 23, 42, 0.045); border: 1px solid var(--stroke-soft);
    border-radius: 8px; padding: 6px 9px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .fbtn {
    flex: none; padding: 5px 12px; border-radius: 8px; cursor: pointer;
    border: 1px solid var(--stroke-soft); background: rgba(15, 23, 42, 0.035);
    color: var(--text); font-size: 11.5px; font-weight: 600;
    transition: border-color 0.15s, color 0.15s, transform 0.06s;
  }
  .fbtn:hover { border-color: var(--accent); color: var(--accent); }
  .fbtn:active { transform: scale(0.95); }
  .fbtn:disabled { opacity: 0.5; cursor: default; }
  .usage { font-size: 10.5px; line-height: 1.7; padding: 0 2px; }

  /* ===== 空状态 ===== */
  .empty { display: flex; flex-direction: column; align-items: center; gap: 7px; padding: 30px 16px; text-align: center; }
  .empty-ico { font-size: 26px; line-height: 1; opacity: 0.75; }
  .empty-t { font-size: 12.5px; font-weight: 650; color: var(--text); }
  .empty-d { font-size: 11px; line-height: 1.7; max-width: 360px; }

  /* ===== 消息列表：时间轴式卡片行 ===== */
  .msgs { display: flex; flex-direction: column; }
  .msg { position: relative; padding: 11px 14px 11px 30px; border-bottom: 1px solid var(--stroke-soft); transition: background 0.12s; }
  .msg:hover { background: rgba(15, 23, 42, 0.025); }
  .msg:last-child { border-bottom: none; }
  /* 左缘时间轴竖线 + 节点圆点 */
  .msg::before {
    content: ""; position: absolute; left: 18px; top: 0; bottom: 0;
    width: 1px; background: var(--stroke-soft);
  }
  .msg:first-child::before { top: 14px; }
  .msg:last-child::before { bottom: auto; height: 14px; }
  .msg:first-child:last-child::before { height: 14px; }
  .msg-dot {
    position: absolute; left: 15px; top: 16px;
    width: 7px; height: 7px; border-radius: 50%;
    background: var(--accent); box-shadow: 0 0 0 3px var(--accent-bg);
  }
  .msg-head { display: flex; align-items: baseline; gap: 8px; min-width: 0; }
  .msg-title { flex: 1; min-width: 0; font-size: 12.5px; font-weight: 650; color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .msg-time { flex: none; font-size: 10px; color: var(--text-dim); font-variant-numeric: tabular-nums; }
  .msg-body {
    margin-top: 4px; font-size: 11.5px; color: var(--text-dim); line-height: 1.7;
    word-break: break-all; white-space: pre-wrap;
    background: rgba(15, 23, 42, 0.032); border-radius: 8px; padding: 7px 10px;
  }
</style>
