<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { getProcesses, killProcess, killTree, checkSignatures, openFolder } from "./api";
  import type { ProcInfo, SigInfo } from "./api";
  import { toast, askConfirm, rowCtx, copyText } from "./stores";
  import DetailModal from "./DetailModal.svelte";
  import type { DetailSection } from "./DetailModal.svelte";
  import { sigText } from "./DetailModal.svelte";
  import { hashFile, openUrl } from "./api";

  let rows: ProcInfo[] = [];
  let q = "";
  let sortKey: "cpu" | "mem_mb" | "name" | "pid" | "started" = "cpu";
  let asc = false;
  let auto = true;
  let timer: ReturnType<typeof setInterval>;

  const load = async () => {
    try { rows = await getProcesses(); } catch (e) { console.error(e); }
  };
  onMount(() => {
    load();
    timer = setInterval(load, 2000);
  });
  onDestroy(() => clearInterval(timer));
  function toggleAuto() {
    auto = !auto;
    clearInterval(timer);
    if (auto) timer = setInterval(load, 2000);
  }

  $: pidSet = new Set(rows.map((r) => r.pid));
  $: filtered = rows
    .filter((r) => !q || r.name.toLowerCase().includes(q.toLowerCase()) || r.exe.toLowerCase().includes(q.toLowerCase()) || String(r.pid).includes(q))
    .sort((a, b) => {
      const va = a[sortKey], vb = b[sortKey];
      const c = typeof va === "number" && typeof vb === "number" ? va - vb : String(va).localeCompare(String(vb));
      return asc ? c : -c;
    });

  let sig: Record<string, SigInfo> = {};
  const pending = new Set<string>();
  $: todo = [...new Set(filtered.slice(0, 120).map((r) => r.exe).filter((p) => p && !sig[p] && !pending.has(p)))];
  $: if (todo.length) check(todo);
  async function check(paths: string[]) {
    paths.forEach((p) => pending.add(p));
    try {
      const res = await checkSignatures(paths);
      const next = { ...sig };
      for (const r of res) next[r.path] = r;
      sig = next;
    } catch { /* 网络/签名服务失败时静默 */ }
    const next = { ...sig };
    for (const p of paths) if (!next[p]) next[p] = { path: p, status: "Unknown", signer: "" };
    sig = next;
    paths.forEach((p) => pending.delete(p));
  }

  function sortBy(k: typeof sortKey) {
    if (sortKey === k) asc = !asc;
    else { sortKey = k; asc = false; }
  }
  const arrow = (k: typeof sortKey) => (sortKey === k ? (asc ? "↑" : "↓") : "");

  async function kill(p: ProcInfo, tree: boolean) {
    if (p.pid < 10) return toast(`拒绝操作关键系统进程 (PID ${p.pid})`, "err");
    if (!(await askConfirm(`确定${tree ? "结束整个进程树" : "结束"}「${p.name}」(PID ${p.pid})？`, true))) return;
    try {
      toast(await (tree ? killTree(p.pid) : killProcess(p.pid)), "ok");
      load();
    } catch (e) { toast(String(e), "err"); }
  }
  async function locate(p: ProcInfo) {
    if (!p.exe) return;
    try { await openFolder(p.exe); } catch (e) { toast(String(e), "err"); }
  }
  const fmtTime = (s: number) =>
    s ? new Date(s * 1000).toLocaleString([], { month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit" }) : "-";
  const fmtMem = (mb: number) => (mb < 1 ? mb.toFixed(2) : mb.toFixed(0)) + " MB";
  let detail: ProcInfo | null = null;
  function procSections(r: ProcInfo): DetailSection[] {
    const st = r.exe ? sig[r.exe] : undefined;
    const ts = r.started > 1e12 ? r.started : r.started * 1000;
    return [
      { title: "基本信息", items: [
        { k: "进程名", v: r.name }, { k: "PID", v: r.pid },
        { k: "父进程 PID", v: r.ppid ?? "（已退出）" }, { k: "状态", v: r.status },
        { k: "启动时间", v: ts > 0 ? new Date(ts).toLocaleString("zh-CN", { hour12: false }) : "" },
      ]},
      { title: "资源占用", items: [
        { k: "CPU 占用", v: r.cpu.toFixed(1) + " %" }, { k: "内存", v: fmtMem(r.mem_mb) },
      ]},
      { title: "路径与签名", items: [
        { k: "可执行路径", v: r.exe, wide: true }, { k: "命令行", v: r.cmd, wide: true },
        ...(st ? [{ k: "签名状态", v: sigText(st.status) }, { k: "签名者", v: st.signer || "（无）" }] : []),
      ]},
    ];
  }
</script>

<div class="view">
  <div class="toolbar">
    <input class="search" placeholder="搜索 进程名 / PID / 路径…" bind:value={q} />
    <button type="button" class="btn" class:primary={auto} on:click={toggleAuto}>
      {auto ? "⏸ 停止刷新" : "▶ 自动刷新(2s)"}
    </button>
    <span class="dim">共 {filtered.length} / {rows.length} 个进程</span>
  </div>

  <div class="card">
    <div class="tbl-wrap">
      <table class="tbl fix">
        <thead>
          <tr>
            <th class="sortable" style="width:34%" on:click={() => sortBy("name")}>进程 / PID {arrow("name")}</th>
            <th class="sortable" style="width:9%" on:click={() => sortBy("cpu")}>CPU% {arrow("cpu")}</th>
            <th class="sortable" style="width:10%" on:click={() => sortBy("mem_mb")}>内存 {arrow("mem_mb")}</th>
            <th style="width:11%">签名</th>
            <th class="sortable" style="width:15%" on:click={() => sortBy("started")}>启动时间 {arrow("started")}</th>
            <th style="width:44px"></th>
          </tr>
        </thead>
        <tbody>
          {#each filtered as r (r.pid)}
            {@const items = [
              { label: "查看详细信息", action: () => (detail = r) },
              { label: "结束进程", action: () => kill(r, false), danger: true },
              { label: "结束进程树", action: () => kill(r, true), danger: true },
              { label: "打开文件位置", action: () => locate(r) },
              { label: "复制进程名", action: () => copyText(r.name) },
              { label: "复制 PID", action: () => copyText(String(r.pid)) },
              { label: "复制可执行文件路径", action: () => copyText(r.exe) },
              { label: "🔴 微步查询文件（SHA256）", action: async () => {
                  if (!r.exe) return toast("该进程无可执行文件路径", "err");
                  try { const h = (await hashFile(r.exe)).toLowerCase(); await openUrl(`https://s.threatbook.com/report/file/${h}`); toast("已在浏览器打开微步查询", "ok"); }
                  catch (e) { toast(String(e), "err"); }
                } },
              { label: "🟢 VirusTotal 查询文件", action: async () => {
                  if (!r.exe) return toast("该进程无可执行文件路径", "err");
                  try { const h = (await hashFile(r.exe)).toLowerCase(); await openUrl(`https://www.virustotal.com/gui/file/${h}`); toast("已在浏览器打开 VirusTotal", "ok"); }
                  catch (e) { toast(String(e), "err"); }
                } }
            ]}
            <tr on:contextmenu={(e) => rowCtx(e, items)} on:dblclick={() => (detail = r)}>
              <td title={(r.cmd || r.exe) + `  ·  PID ${r.pid}`}>
                {#if r.ppid && pidSet.has(r.ppid)}<span class="dim">└ </span>{/if}<b>{r.name}</b>
                <span class="dim mono" style="margin-left:6px">{r.pid}</span>
              </td>
              <td class="mono">{r.cpu.toFixed(1)}</td>
              <td class="mono">{fmtMem(r.mem_mb)}</td>
              <td>
                {#if r.exe && sig[r.exe]}
                  {#if sig[r.exe].status === "Valid"}<span class="sig ok" title={`签名者：${sig[r.exe].signer}`}>✔ 已签名</span>
                  {:else if sig[r.exe].status === "NotSigned"}<span class="sig warn" title="该文件没有数字签名">⚠ 未签名</span>
                  {:else if sig[r.exe].status === "Invalid"}<span class="sig bad" title={`签名无效/不受信任：${sig[r.exe].signer}`}>✗ 签名无效</span>
                  {:else}<span class="sig bad" title={sig[r.exe].signer}>✖ {sig[r.exe].status}</span>{/if}
                {:else}<span class="sig dim">…</span>{/if}
              </td>
              <td class="mono" title={r.status}>{fmtTime(r.started)}</td>
              <td><button class="btn mini" title="操作菜单（或右键行）" on:click={(e) => rowCtx(e, items)}>⋮</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if filtered.length === 0}
        <div class="empty">没有匹配的进程</div>
      {/if}
    </div>
    <div class="footer">
      <span>点击列头排序 · 2 秒自动刷新</span>
      <span class="dim" style="margin-left:auto">恶意进程排查：关注 <b>未签名</b> / 非常规路径 / 高 CPU 可疑项</span>
    </div>
  </div>
</div>
<DetailModal
  open={!!detail}
  title={`进程详情${detail ? " · " + detail.name : ""}`}
  sections={detail ? procSections(detail) : []}
  on:close={() => (detail = null)}
/>
