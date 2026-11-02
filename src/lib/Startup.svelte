<script lang="ts">
  import { onMount } from "svelte";
  import { getStartup, removeStartup, checkSignatures, openFolder } from "./api";
  import type { StartupInfo, SigInfo } from "./api";

  let detail: StartupInfo | null = null;
  function startupSections(v: StartupInfo): DetailSection[] {
    const exe = exeOf(v.command);
    const st = exe ? sig[exe] : undefined;
    return [
      { title: "基本信息", items: [
        { k: "名称", v: v.name }, { k: "类型", v: v.kind },
        { k: "来源位置", v: v.hive, wide: true },
      ]},
      { title: "启动命令", items: [
        { k: "命令", v: v.command, wide: true },
      ]},
      ...(exe
        ? [{ title: "签名甄别", items: [
              { k: "目标文件", v: exe, wide: true },
              { k: "签名状态", v: st ? sigText(st.status) : "…（检测中）" },
              { k: "签名者", v: st?.signer || "（无）" },
            ] }]
        : []),
    ];
  }
  import { toast, askConfirm, rowCtx, copyText } from "./stores";
  import DetailModal, { sigText } from "./DetailModal.svelte";
  import type { DetailSection } from "./DetailModal.svelte";

  let rows: StartupInfo[] = [];
  let q = "";
  let loading = false;
  let err = "";

  async function load() {
    loading = true; err = "";
    try { rows = await getStartup(); } catch (e) { err = String(e); } finally { loading = false; }
  }
  onMount(load);

  $: filtered = rows
    .filter((s) => !q || s.name.toLowerCase().includes(q.toLowerCase()) || s.command.toLowerCase().includes(q.toLowerCase()))
    .sort((a, b) => a.kind.localeCompare(b.kind) || a.name.localeCompare(b.name));

  function exeOf(cmd: string) {
    const m = cmd.match(/^"([^"]+)"|(\S+\.exe)/i);
    return m ? (m[1] || m[2]) : "";
  }

  let sig: Record<string, SigInfo> = {};
  const pending = new Set<string>();
  $: todo = [...new Set(filtered.map((s) => exeOf(s.command)).filter((p) => p && !sig[p] && !pending.has(p)))];
  $: if (todo.length) check(todo);
  async function check(paths: string[]) {
    paths.forEach((p) => pending.add(p));
    try {
      const res = await checkSignatures(paths);
      const next = { ...sig };
      for (const r of res) next[r.path] = r;
      sig = next;
    } catch { /* 忽略，下面统一兜底 */ }
    // 与 Processes 一致：失败也落一条 Unknown，避免响应式语句无限重试
    const next = { ...sig };
    for (const p of paths) if (!next[p]) next[p] = { path: p, status: "Unknown", signer: "" };
    sig = next;
    paths.forEach((p) => pending.delete(p));
  }

  async function del(s: StartupInfo) {
    if (!(await askConfirm(`确定删除启动项「${s.name}」？\n来源：${s.hive}\n命令：${s.command}`, true))) return;
    try { toast(await removeStartup(s.kind, s.hive, s.name, s.command), "ok"); load(); }
    catch (e) { toast(String(e), "err"); }
  }
  async function locate(s: StartupInfo) {
    const p = exeOf(s.command) || s.command;
    try { await openFolder(p); } catch (e) { toast(String(e), "err"); }
  }
</script>

<div class="view">
  <div class="toolbar">
    <input class="search" placeholder="搜索 启动项名称 / 命令…" bind:value={q} />
    <button class="btn primary" on:click={load} disabled={loading}>{loading ? "加载中…" : "刷新"}</button>
    <span class="dim">共 {filtered.length} 项</span>
  </div>

  {#if err}<div class="banner">加载失败：{err}</div>{/if}

  <div class="card">
    <div class="tbl-wrap">
      <table class="tbl fix">
        <thead>
          <tr>
            <th style="width:22%">名称</th><th style="width:24%">来源</th><th>命令</th><th style="width:11%">签名</th><th style="width:44px"></th>
          </tr>
        </thead>
        <tbody>
          {#each filtered as s (s.hive + "|" + s.name + "|" + s.kind)}
            {@const items = [
              { label: "查看详细信息", action: () => (detail = s) },
              { label: "删除启动项", action: () => del(s), danger: true },
              { label: "打开文件位置", action: () => locate(s) },
              { label: "复制名称", action: () => copyText(s.name) },
              { label: "复制命令", action: () => copyText(s.command) }
            ]}
            <tr on:contextmenu={(e) => rowCtx(e, items)} on:dblclick={() => (detail = s)}>
              <td title={s.name}><b>{s.name}</b></td>
              <td>
                <span class="chip">{s.kind}</span>
                <span class="dim" title={s.hive}>{s.hive.replace("HKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\", "HKLM\\").replace("HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\", "HKCU\\")}</span>
              </td>
              <td class="mono" title={s.command}>{s.command}</td>
              <td>
                {#if exeOf(s.command) && sig[exeOf(s.command)]}
                  {#if sig[exeOf(s.command)].status === "Valid"}<span class="sig ok" title={`签名者：${sig[exeOf(s.command)].signer}`}>✔ 已签名</span>
                  {:else if sig[exeOf(s.command)].status === "NotSigned"}<span class="sig warn" title="该文件没有数字签名">⚠ 未签名</span>
                  {:else if sig[exeOf(s.command)].status === "Invalid"}<span class="sig bad" title="签名无效或不受信任">✗ 签名无效</span>
                  {:else}<span class="sig bad">✖ {sig[exeOf(s.command)].status}</span>{/if}
                {:else}<span class="sig dim">—</span>{/if}
              </td>
              <td><button class="btn mini" title="操作菜单（或右键行）" on:click={(e) => rowCtx(e, items)}>⋮</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if loading && rows.length === 0}
        <div class="empty">正在加载启动项…</div>
      {:else if filtered.length === 0}
        <div class="empty">没有匹配的启动项</div>
      {/if}
    </div>
    <div class="footer">
      <span>覆盖 Run / RunOnce（HKLM+HKCU×WOW6432Node）与 启动文件夹</span>
      <span class="dim" style="margin-left:auto">可疑项：无签名、位于 Temp/AppData、名称伪装系统文件</span>
    </div>
  </div>
</div>
<DetailModal
  open={!!detail}
  title={`启动项详情${detail ? " · " + detail.name : ""}`}
  sections={detail ? startupSections(detail) : []}
  on:close={() => (detail = null)}
/>
