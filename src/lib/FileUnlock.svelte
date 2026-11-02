<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { unlockScan, forceDelete } from "./api";
  import type { ScanReport } from "./api";
  import { toast, askConfirm, dropQueue, rowCtx, copyText, unlockRequest } from "./stores";
  import { unlockTakeRequest } from "./api";

  let paths: string[] = [];
  let input = "";
  let scanning = false;
  let deleting = false;
  let report: ScanReport | null = null;
  let unsubDrop: (() => void) | null = null;
  let unsubReq: (() => void) | null = null;

  // 右键菜单直达：加入路径并立即扫描
  function handleUnlockRequest(p: string) {
    if (addPath(p)) {
      scan();
      toast("已从右键菜单载入目标", "ok");
    }
  }

  function addPath(raw: string) {
    const p = raw.trim().replace(/^"(.*)"$/, "$1");
    if (!p) return false;
    if (paths.includes(p)) return false;
    paths = [...paths, p];
    report = null;
    return true;
  }
  function removePath(p: string) {
    paths = paths.filter((x) => x !== p);
    report = null;
  }
  function addFromInput() {
    let added = 0;
    input.split(/\r?\n/).forEach((line) => { if (addPath(line)) added++; });
    if (added) toast(`已添加 ${added} 个路径`, "ok");
    input = "";
  }

  onMount(() => {
    // App 级统一监听拖放，这里只消费队列（组件切换零监听器变动）
    unsubDrop = dropQueue.subscribe((batch) => {
      if (!batch.length) return;
      let added = 0;
      batch.forEach((p) => { if (addPath(p)) added++; });
      dropQueue.set([]);
      if (added) toast(`已添加 ${added} 个拖入项`, "ok");
    });
    // 首启直达（事件可能早于监听，拉取兜底）
    (async () => {
      try {
        const req = await unlockTakeRequest();
        if (req) handleUnlockRequest(req);
      } catch {}
    })();
    // 运行中直达（单实例转发）
    unsubReq = unlockRequest.subscribe((p) => {
      if (p) {
        unlockRequest.set(null);
        handleUnlockRequest(p);
      }
    });
  });
  onDestroy(() => { unsubDrop?.(); unsubReq?.(); });

  async function scan() {
    if (!paths.length) return toast("请先添加文件或文件夹", "info");
    scanning = true; report = null;
    try {
      report = await unlockScan(paths);
      if (report.locks.length === 0) toast("未发现占用，可以直接删除", "ok");
    } catch (e) { toast(String(e), "err"); }
    finally { scanning = false; }
  }

  async function forceDel() {
    if (!paths.length) return toast("请先添加文件或文件夹", "info");
    const lockHint = report?.locks.length
      ? `将强制结束 ${report.locks.length} 个占用进程，`
      : "";
    const ok = await askConfirm(
      `${lockHint}确定强制删除以下 ${paths.length} 个路径（含全部内容）？\n\n${paths.join("\n")}\n\n此操作不可恢复！`,
      true
    );
    if (!ok) return;
    deleting = true;
    try {
      const r = await forceDelete(paths);
      let msg = `已删除 ${r.deleted_files} 个文件`;
      if (r.killed.length) msg += `，结束 ${r.killed.length} 个进程`;
      if (r.failed.length) {
        toast(`${msg}；${r.failed.length} 项失败：\n${r.failed.map((f) => `${f.path}（${f.error}）`).join("\n")}`, "err");
      } else {
        toast(msg, "ok");
        paths = [];
        report = null;
      }
    } catch (e) { toast(String(e), "err"); }
    finally { deleting = false; }
  }
</script>

<div class="view">
  <div class="toolbar">
    <input class="search" style="max-width:520px" placeholder="输入文件或文件夹完整路径后回车，或直接拖入…"
      bind:value={input} on:keydown={(e) => e.key === "Enter" && addFromInput()} />
    <button class="btn" on:click={addFromInput} disabled={!input.trim()}>添加</button>
    <button class="btn primary" on:click={scan} disabled={scanning || deleting || !paths.length}>
      {scanning ? "检测中…" : "🔍 检测占用"}
    </button>
    <button class="btn danger" on:click={forceDel} disabled={scanning || deleting || !paths.length}>
      {deleting ? "删除中…" : "💥 强制解除并删除"}
    </button>
  </div>

  <div class="dropzone">将文件 / 文件夹从资源管理器拖到窗口任意位置即可添加（支持多选）</div>

  <div class="card">
    <div class="pathlist" style="padding:10px">
      {#if paths.length === 0}
        <div class="empty" style="min-height:60px">暂无待处理路径</div>
      {:else}
        {#each paths as p (p)}
          <div class="pathrow" on:contextmenu={(e) => rowCtx(e, [
            { label: "移除该路径", action: () => removePath(p), danger: true },
            { label: "复制路径", action: () => copyText(p) }
          ])}>
            <span>📄</span>
            <span class="p mono" title={p}>{p}</span>
            <button class="btn mini danger" on:click={() => removePath(p)}>移除</button>
          </div>
        {/each}
      {/if}
    </div>

    <div class="footer">
      <span>共 {paths.length} 个路径</span>
      {#if report}
        <span style="margin-left:auto">
          已扫描 {report.file_count} 个文件{report.truncated ? "（数量超出上限，仅扫描前 4000 个）" : ""}
          · 占用进程 {report.locks.length} 个
        </span>
      {/if}
    </div>
  </div>

  {#if report}
    <div class="card" style="flex:1">
      <div class="tbl-wrap">
        <table class="tbl">
          <thead><tr><th>进程</th><th>PID</th><th>可执行文件路径</th></tr></thead>
          <tbody>
            {#each report.locks as l (l.pid)}
              <tr>
                <td><b>{l.name || "未知进程"}</b></td>
                <td class="mono">{l.pid}</td>
                <td class="mono dim" title={l.exe}>{l.exe || "-"}</td>
              </tr>
            {/each}
          </tbody>
        </table>
        {#if report.locks.length === 0}
          <div class="empty">✅ 没有进程占用这些文件，可直接强制删除</div>
        {/if}
      </div>
      <div class="footer">
        <span>强制删除 = 结束上方占用进程 → 清除只读属性 → 递归删除（含只读/系统属性文件）</span>
      </div>
    </div>
  {/if}
</div>
