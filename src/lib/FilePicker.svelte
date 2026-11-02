<script lang="ts">
  // v0.9.12 应用内轻量资源管理器：免原生对话框（无 COM 开销、响应即时、iOS 统一风格）
  import { createEventDispatcher } from "svelte";
  import { socialListDir, type DirEntryOut, type DirListing } from "./api";

  export let open = false;

  const dispatch = createEventDispatcher<{ pick: string; close: void }>();

  let cwd = ""; // "" = 此电脑（驱动器层）
  let parentPath: string | null = null;
  let entries: DirEntryOut[] = [];
  let sel = ""; // 选中文件完整路径
  let selName = "";
  let loading = false;
  let err = "";
  let loaded = false;
  let pathInput = ""; // 地址栏（可输入路径直达；支持直接指向文件）

  $: if (open && !loaded) {
    loaded = true;
    void go(cwd);
  }

  async function go(p: string) {
    loading = true;
    err = "";
    sel = "";
    selName = "";
    try {
      const r: DirListing = await socialListDir(p);
      cwd = r.path;
      parentPath = r.parent;
      entries = r.entries;
      pathInput = r.path;
      if (r.sel) {
        sel = r.sel;
        selName = r.sel.split(/[\\/]/).pop() ?? r.sel;
      }
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
  }

  function join(base: string, name: string): string {
    if (!base) return `${name}\\`;
    return base.endsWith("\\") ? base + name : `${base}\\${name}`;
  }

  function goPath() {
    const raw = pathInput.trim().replace(/^"+|"+$/g, "");
    if (!raw) return void go("");
    void go(raw);
  }
  function rowClick(e: DirEntryOut) {
    if (!e.is_dir) {
      sel = join(cwd, e.name);
      selName = e.name;
    }
  }
  function rowDbl(e: DirEntryOut) {
    if (e.is_dir) void go(join(cwd, e.name));
    else dispatch("pick", join(cwd, e.name));
  }
  function confirmPick() {
    if (sel) dispatch("pick", sel);
  }
  function close() {
    dispatch("close");
  }

  const fmtSize = (n: number) =>
    n >= 1073741824 ? `${(n / 1073741824).toFixed(2)} GB` : n >= 1048576 ? `${(n / 1048576).toFixed(1)} MB` : n >= 1024 ? `${(n / 1024).toFixed(0)} KB` : `${n} B`;
  function fmtT(ms: number): string {
    if (!ms) return "";
    const d = new Date(ms);
    return `${d.getMonth() + 1}/${d.getDate()} ${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  }
  function onKey(ev: KeyboardEvent) {
    if (!open) return;
    if (ev.key === "Escape") close();
    else if (ev.key === "Enter" && sel) confirmPick();
  }
</script>

<svelte:window on:keydown={onKey} />

{#if open}
  <div class="fpm" role="presentation" on:click|self={close}>
    <div class="fpp" role="dialog" aria-label="选择要发送的文件">
      <div class="fph">
        <button type="button" class="fpnav" disabled={parentPath === null} title="上一级"
          on:click={() => parentPath !== null && go(parentPath)}>‹</button>
        <b>选择要发送的文件</b>
        <input class="fppath" type="text" bind:value={pathInput} spellcheck="false"
          placeholder="输入路径回车直达（如 C:\Users\me\Desktop），可直接指向文件"
          title="输入路径后回车访问；输入文件路径直接预选"
          on:keydown={(e) => e.key === "Enter" && goPath()} />
        <span style="flex:1"></span>
        <button type="button" class="fpx" title="关闭" on:click={close}>✕</button>
      </div>
      <div class="fpquick">
        <button type="button" on:click={() => go("")}>🖥 此电脑</button>
        <button type="button" on:click={() => go("@desktop")}>桌面</button>
        <button type="button" on:click={() => go("@downloads")}>下载</button>
        <button type="button" on:click={() => go("@docs")}>文档</button>
      </div>
      <div class="fplist">
        {#if loading}
          <div class="fpempty">读取中…</div>
        {:else if err}
          <div class="fpempty">{err}</div>
        {:else if !entries.length}
          <div class="fpempty">此文件夹为空</div>
        {:else}
          {#each entries as e (e.name)}
            <button type="button" class="fprow" class:sel={!e.is_dir && selName === e.name}
              on:click={() => rowClick(e)} on:dblclick={() => rowDbl(e)}
              title={e.is_dir ? "双击进入" : "单击选择 · 双击直接发送"}>
              <span class="fico">{e.is_dir ? "📁" : "📄"}</span>
              <span class="fpname">{e.name}</span>
              <span class="fpsub">{e.is_dir ? "" : `${fmtSize(e.size)}${e.mtime ? ` · ${fmtT(e.mtime)}` : ""}`}</span>
            </button>
          {/each}
        {/if}
      </div>
      <div class="fpfoot">
        <span class="fpsel">{sel ? selName : "未选择文件"}</span>
        <button type="button" class="fpbtn" on:click={close}>取消</button>
        <button type="button" class="fpbtn ok" disabled={!sel} on:click={confirmPick}>发送</button>
      </div>
    </div>
  </div>
{/if}

<style>
  /* 与 Social me-mask/me-pop.mid 同语言：深色蒙层 + #f5f5f7 卡片 + 8vh 悬停 */
  .fpm { position: fixed; inset: 0; z-index: 70; background: rgba(2, 6, 23, 0.52); display: flex; align-items: flex-start; justify-content: center; padding: 8vh 16px 16px; }
  .fpp { width: min(640px, 94vw); max-width: 100%; max-height: 82vh; background: #f5f5f7; border-radius: 18px; box-shadow: 0 24px 60px rgba(0, 0, 0, 0.28); display: flex; flex-direction: column; gap: 10px; padding: 14px 16px 16px; overflow: hidden; }
  .fph { display: flex; align-items: center; gap: 8px; }
  .fph b { font-size: 16px; font-weight: 700; color: #1c1c1e; letter-spacing: 0.2px; white-space: nowrap; }
  .fpnav { width: 26px; height: 26px; border-radius: 50%; border: 1px solid #e3e3e8; background: #fff; font-size: 15px; line-height: 1; color: #3a3a3c; cursor: pointer; flex: none; }
  .fpnav:disabled { opacity: 0.35; cursor: default; }
  .fppath {
    flex: 1; min-width: 0; height: 30px; padding: 0 10px;
    border: 1px solid #e3e3e8; border-radius: 9px; background: #fff;
    font-size: 12px; color: #1c1c1e; outline: none;
  }
  .fppath:focus { border-color: #007aff; box-shadow: 0 0 0 3px rgba(0, 122, 255, 0.12); }
  .fppath::placeholder { color: #b9b9c0; }
  .fpx { width: 26px; height: 26px; border-radius: 50%; border: 1px solid #e3e3e8; background: #fff; color: #3a3a3c; font-size: 11px; cursor: pointer; flex: none; }
  .fpquick { display: flex; gap: 6px; flex-wrap: wrap; }
  .fpquick button { border: 1px solid #e3e3e8; background: #fff; border-radius: 999px; padding: 4px 12px; font-size: 12px; color: #3a3a3c; font-weight: 600; cursor: pointer; }
  .fpquick button:hover { background: #f2f2f7; }
  .fplist { border: 1px solid #e3e3e8; border-radius: 12px; overflow-y: auto; min-height: 300px; max-height: 48vh; background: #fff; }
  .fprow { display: flex; align-items: center; gap: 10px; width: 100%; padding: 9px 12px; border: 0; border-bottom: 1px solid #f0f0f2; background: transparent; cursor: pointer; text-align: left; font: inherit; }
  .fprow:last-child { border-bottom: 0; }
  .fprow:hover { background: #f7f7fa; }
  .fprow.sel { background: rgba(0, 122, 255, 0.1); }
  .fprow .fico { font-size: 17px; flex: none; }
  .fpname { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 13px; color: #1c1c1e; font-weight: 550; }
  .fpsub { font-size: 11px; color: #86868b; flex: none; }
  .fpempty { padding: 44px 0; text-align: center; font-size: 12px; color: #86868b; }
  .fpfoot { display: flex; align-items: center; gap: 8px; }
  .fpsel { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: 12px; color: #3a3a3c; }
  .fpbtn { border-radius: 999px; background: #fff; border: 1px solid #d1d1d6; color: #3a3a3c; font-weight: 600; padding: 6px 16px; font-size: 12px; cursor: pointer; }
  .fpbtn:hover { background: #f2f2f7; }
  .fpbtn.ok { background: #007aff; border-color: #007aff; color: #fff; }
  .fpbtn.ok:hover { background: #0071eb; }
  .fpbtn:disabled { opacity: 0.45; cursor: default; }
</style>
