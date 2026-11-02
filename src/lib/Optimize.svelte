<script lang="ts">
  import { onMount } from "svelte";
  import { optGetAll, optApply, optSetCtrlWindow } from "./api";
  import type { OptState } from "./api";
  import { toast, askConfirm } from "./stores";

  let st: OptState | null = null;
  let loadErr = "";
  let busy = false;
  // 双击 Ctrl 触发时限（ms）：独立参数，滑杆调整即时生效并持久保存
  let ctrlWin = 600;

  let sel = {
    this_pc: false,
    recycle_bin: false,
    control_panel: false,
    network: false,
    taskbar: false,
    ctx_ps: false,
    ctx_cmd: false,
    unlock_ctx: false
  };

  function syncFromState(s: OptState) {
    sel = {
      this_pc: s.desktop_this_pc,
      recycle_bin: s.desktop_recycle_bin,
      control_panel: s.desktop_control_panel,
      network: s.desktop_network,
      taskbar: s.taskbar_dbl_desktop,
      ctx_ps: s.ctx_power_shell,
      ctx_cmd: s.ctx_cmd,
      unlock_ctx: s.ctx_unlock
    };
  }

  function collectDesktop(): string[] {
    const out: string[] = [];
    if (sel.this_pc) out.push("this_pc");
    if (sel.recycle_bin) out.push("recycle_bin");
    if (sel.control_panel) out.push("control_panel");
    if (sel.network) out.push("network");
    return out;
  }

  // 开关即时生效：任一切换即按当前整套选择应用（幂等小操作，无重启代价）
  async function applyAll() {
    if (busy) return;
    busy = true;
    try {
      st = await optApply(collectDesktop(), sel.taskbar, sel.ctx_ps, sel.ctx_cmd, sel.unlock_ctx);
      syncFromState(st);
      toast("优化已应用，立即生效", "ok");
    } catch (e) {
      toast(String(e), "err");
    } finally {
      busy = false;
    }
  }

  async function saveWin() {
    try {
      await optSetCtrlWindow(ctrlWin);
      toast(`双击 Ctrl 触发时限已设为 ${ctrlWin}ms`, "ok");
    } catch (e) {
      toast(String(e), "err");
    }
  }

  async function restoreDefault() {
    if (
      !(await askConfirm(
        "确定恢复默认设置？将移除已创建的桌面图标、停用任务栏双击、移除右键终端入口。",
        true
      ))
    )
      return;
    busy = true;
    try {
      st = await optApply([], false, false, false, false);
      syncFromState(st);
      toast("已恢复默认设置", "ok");
    } catch (e) {
      toast(String(e), "err");
    } finally {
      busy = false;
    }
  }

  onMount(async () => {
    try {
      st = await optGetAll();
      syncFromState(st);
      ctrlWin = st.ctrl_window_ms;
    } catch (e) {
      loadErr = String(e);
    }
  });
</script>

<div class="view">
  {#if loadErr}
    <div class="banner banner-warn">加载失败：{loadErr}</div>
  {/if}

  {#if st}
    <div class="group">
      <div class="pol-head"><span class="pol-bar"></span><b>桌面显示</b></div>
      <div class="group-card">
        <div class="opt-item">
          <div class="opt-txt"><span class="opt-name">此电脑</span><span class="opt-desc">在桌面创建指向系统命名空间的快捷方式，关闭开关即移除</span></div>
          <input type="checkbox" class="ios-sw" bind:checked={sel.this_pc} disabled={busy} on:change={applyAll} />
        </div>
        <div class="opt-item">
          <div class="opt-txt"><span class="opt-name">回收站</span><span class="opt-desc">同上，不写注册表、无需重启资源管理器</span></div>
          <input type="checkbox" class="ios-sw" bind:checked={sel.recycle_bin} disabled={busy} on:change={applyAll} />
        </div>
        <div class="opt-item">
          <div class="opt-txt"><span class="opt-name">控制面板</span><span class="opt-desc">同上</span></div>
          <input type="checkbox" class="ios-sw" bind:checked={sel.control_panel} disabled={busy} on:change={applyAll} />
        </div>
        <div class="opt-item">
          <div class="opt-txt"><span class="opt-name">网络</span><span class="opt-desc">同上</span></div>
          <input type="checkbox" class="ios-sw" bind:checked={sel.network} disabled={busy} on:change={applyAll} />
        </div>
      </div>
    </div>

    <div class="group">
      <div class="pol-head"><span class="pol-bar"></span><b>全局快捷键</b><span class="dim">工具运行期间生效</span></div>
      <div class="group-card">
        <div class="opt-item">
          <div class="opt-txt"><span class="opt-name">双击 Ctrl 显示 / 恢复桌面</span><span class="opt-desc">依赖本工具运行（无驱动、无常驻）；按键全程透传，不影响 Ctrl 组合键</span></div>
          <input type="checkbox" class="ios-sw" bind:checked={sel.taskbar} disabled={busy} on:change={applyAll} />
        </div>
        <div class="opt-item">
          <div class="opt-txt">
            <span class="opt-name">触发时限</span>
            <span class="opt-desc">两次 Ctrl 按下的最大间隔，超出视为两次独立按键</span>
          </div>
          <span class="win-ctl">
            <input type="range" min="200" max="1500" step="50" bind:value={ctrlWin} on:change={saveWin} />
            <b>{ctrlWin}ms</b>
          </span>
        </div>
      </div>
    </div>

    <div class="group">
      <div class="pol-head"><span class="pol-bar"></span><b>资源管理器右键菜单</b><span class="dim">定位到对应目录 · Win11 在「显示更多选项」中</span></div>
      <div class="group-card">
        <div class="opt-item">
          <div class="opt-txt"><span class="opt-name">在此处打开 PowerShell</span><span class="opt-desc">写入用户级注册表，可随时移除</span></div>
          <input type="checkbox" class="ios-sw" bind:checked={sel.ctx_ps} disabled={busy} on:change={applyAll} />
        </div>
        <div class="opt-item">
          <div class="opt-txt"><span class="opt-name">在此处打开 CMD</span><span class="opt-desc">同上</span></div>
          <input type="checkbox" class="ios-sw" bind:checked={sel.ctx_cmd} disabled={busy} on:change={applyAll} />
        </div>
        <div class="opt-item">
          <div class="opt-txt"><span class="opt-name">强制解锁 / 删除 · 青刃</span><span class="opt-desc">文件与文件夹右键直达解锁页；即使工具未运行也会自动拉起并载入目标</span></div>
          <input type="checkbox" class="ios-sw" bind:checked={sel.unlock_ctx} disabled={busy} on:change={applyAll} />
        </div>
      </div>
    </div>

    <div class="restore-row">
      <button type="button" class="restore" disabled={busy} on:click={restoreDefault}>恢复默认设置</button>
    </div>
  {:else}
    <div class="group-card"><div class="opt-item"><div class="opt-txt"><span class="opt-desc">加载中…</span></div></div></div>
  {/if}
</div>

<style>
  /* iOS 设置式：居中限宽 + 分组内嵌卡片（解决内容贴左缘） */
  .view {
    max-width: 780px; width: 100%;
    margin: 0 auto;
    padding: 4px 18px 12px;
  }
  .group { display: flex; flex-direction: column; gap: 7px; }
  .group-card {
    background: var(--panel); border: 1px solid var(--stroke-soft);
    border-radius: 12px; overflow: hidden;
  }
  .pol-head {
    display: flex; align-items: center; gap: 8px;
    padding-bottom: 2px;
  }
  .pol-bar { width: 4px; height: 14px; border-radius: 2px; background: var(--accent); flex: none; }
  .pol-head b { font-size: 13px; }
  .restore-row { display: flex; justify-content: flex-end; margin-top: 2px; }
  .restore {
    padding: 7px 16px; border-radius: 10px; cursor: pointer;
    border: 1px solid rgba(229, 72, 77, 0.35); background: rgba(255, 255, 255, 0.7);
    color: #c53035; font-size: 12.5px; font-weight: 600;
    transition: background 0.15s;
  }
  .restore:hover { background: rgba(229, 72, 77, 0.08); }
  .restore:disabled { opacity: 0.55; cursor: wait; }
  .win-ctl { display: inline-flex; align-items: center; gap: 8px; flex: none; }
  .win-ctl input[type="range"] { width: 130px; accent-color: var(--accent); cursor: pointer; }
  .win-ctl b { font-size: 12px; color: var(--accent); min-width: 52px; text-align: right; }
</style>
