<script lang="ts">
  import { onDestroy } from "svelte";
  import { eventQuery } from "./api";
  import type { EventInfo } from "./api";
  import { toast, rowCtx, copyText } from "./stores";
  import DetailModal from "./DetailModal.svelte";
  import type { DetailSection } from "./DetailModal.svelte";

  let rows: EventInfo[] = [];
  let loading = false;
  let queried = false;
  let hours = 72;
  let detail: EventInfo | null = null;

  const ranges = [
    { h: 24, t: "近 24 小时" },
    { h: 72, t: "近 3 天" },
    { h: 168, t: "近 7 天" },
    { h: 720, t: "近 30 天" },
  ];

  // 应急视角 5 类：只拉取关键事件，滤除噪音
  const catLabel: Record<string, string> = {
    logon: "用户登录",
    account: "账户变更",
    service: "服务安装",
    task: "计划任务",
    clearlog: "日志清除",
  };
  let show: Record<string, boolean> = { logon: true, account: true, service: true, task: true, clearlog: true };
  let onlyAbnormal = true; // 默认过滤正常日志：只显示 失败/变更/高风险
  let userSel = ""; // "" = 全部用户
  let q = ""; // 关键字搜索
  let page = 1;
  let pageSize = 100;

  const NORMAL = new Set(["成功", "注销", "信息"]);
  const isAbn = (r: EventInfo) => !NORMAL.has(r.result);

  // 全部派生数据均在内存一次查询结果上计算，零后端往返
  $: users = [...new Set(rows.map((r) => r.user).filter(Boolean))].sort((a, b) => a.localeCompare(b));
  $: base = rows.filter((r) => show[r.kind] && (!onlyAbnormal || isAbn(r)) && (!userSel || r.user === userSel));
  $: kw = q.trim().toLowerCase();
  $: filtered = kw
    ? base.filter((r) => `${r.user} ${r.ip} ${r.detail} ${r.time} ${r.result}`.toLowerCase().includes(kw))
    : base;
  $: failed = filtered.filter((r) => r.result === "失败").length;
  $: risky = filtered.filter((r) => r.result === "高风险").length;

  $: totalPages = Math.max(1, Math.ceil(filtered.length / pageSize));
  // 筛选条件变化 → 回到第一页
  $: kw, userSel, onlyAbnormal, show, pageSize, rows, (page = 1);
  // 越界保护
  $: if (page > totalPages) page = totalPages;
  $: view = filtered.slice((page - 1) * pageSize, page * pageSize);

  const kindLabel = (k: string) => catLabel[k] ?? k;
  const fmtIp = (ip: string) => (!ip || ip === "-" ? "—" : ip === "127.0.0.1" ? "本机回环" : ip);
  const resClass = (r: EventInfo) =>
    r.result === "高风险" ? "sig bad" : r.result === "失败" || r.result === "变更" ? "sig warn" : "sig dim";

  async function load() {
    loading = true;
    try {
      rows = await eventQuery(hours);
      queried = true;
      if (!rows.length) toast("未查到事件：日志可能为空，或相关审核策略未开启", "info");
    } catch (e) {
      toast(String(e), "err");
    } finally {
      loading = false;
    }
  }

  function sections(r: EventInfo): DetailSection[] {
    return [
      { title: "事件标识", items: [
        { k: "发生时间", v: r.time }, { k: "事件 ID", v: r.id },
        { k: "分类", v: kindLabel(r.kind) }, { k: "结果", v: r.result },
        { k: "来源 IP", v: fmtIp(r.ip) },
      ]},
      { title: "主体与关键信息", items: [
        { k: "用户/主体", v: r.user || "（无）" },
        { k: "关键信息", v: r.detail || "（无）", wide: true },
      ]},
    ];
  }

  // 离开本页即清空：查询全程内存完成，无临时文件，无驻留
  onDestroy(() => {
    rows = [];
    detail = null;
    page = 1;
  });
</script>

<div class="page">
  <div class="toolbar">
    <h2>事件日志</h2>
    <span class="dim">应急五类关键事件 · 默认仅异常 · 内存筛选零开销</span>
    <span style="flex:1"></span>
    <select bind:value={hours} disabled={loading}>
      {#each ranges as rg}
        <option value={rg.h}>{rg.t}</option>
      {/each}
    </select>
    <button class="btn" on:click={load} disabled={loading}>{loading ? "查询中…" : "查询"}</button>
  </div>

  <div class="filters">
    <label class="check strong"><input type="checkbox" bind:checked={onlyAbnormal} /> 仅看异常（失败/变更/高风险）</label>
    <span class="sep"></span>
    {#each Object.entries(catLabel) as [k, label]}
      <label class="check"><input type="checkbox" bind:checked={show[k]} /> {label}</label>
    {/each}
    <span class="sep"></span>
    <select class="us" bind:value={userSel} disabled={!rows.length}>
      <option value="">全部用户</option>
      {#each users as u}
        <option value={u}>{u}</option>
      {/each}
    </select>
    <input class="q" type="search" placeholder="搜索 用户 / IP / 关键字 / 时间…" bind:value={q} />
    {#if queried}
      <span class="dim counts">
        <span class="bad-t">失败 {failed}</span> · <span class="bad-t">高风险 {risky}</span>
      </span>
    {/if}
  </div>

  <div class="card">
    <table>
      <thead>
        <tr>
          <th style="width:150px">时间</th>
          <th style="width:88px">分类</th>
          <th style="width:76px">结果</th>
          <th style="width:130px">用户/主体</th>
          <th style="width:130px">来源 IP</th>
          <th>关键信息</th>
        </tr>
      </thead>
      <tbody>
        {#each view as r, i (r.time + "|" + r.id + "|" + r.user + "|" + i)}
          {@const items = [
            { label: "查看详细信息", action: () => (detail = r) },
            { label: "复制时间", action: () => copyText(r.time) },
            { label: "复制用户", action: () => copyText(r.user) },
            ...(r.ip && r.ip !== "-" ? [{ label: "复制来源 IP", action: () => copyText(r.ip) }] : []),
            { label: "复制关键信息", action: () => copyText(r.detail) },
          ]}
          <tr on:contextmenu={(e) => rowCtx(e, items)} on:dblclick={() => (detail = r)}>
            <td class="mono">{r.time}</td>
            <td>{kindLabel(r.kind)}</td>
            <td><span class={resClass(r)}>{r.result}</span></td>
            <td class="mono" title={r.user}>{r.user || "—"}</td>
            <td class="mono">{fmtIp(r.ip)}</td>
            <td class="dim" title={r.detail}>{r.detail || "—"}</td>
          </tr>
        {:else}
          <tr><td colspan="6" class="dim" style="text-align:center;padding:18px 0">
            {loading ? "正在查询事件日志…" : queried ? "当前筛选下没有事件（可关闭「仅看异常」或调整筛选）" : "选择时间范围后点击「查询」"}
          </td></tr>
        {/each}
      </tbody>
    </table>
  </div>

  <div class="pager">
    <span class="dim">显示 {view.length} 条 / 共 {filtered.length} 条</span>
    <span style="flex:1"></span>
    <select bind:value={pageSize} disabled={!filtered.length}>
      <option value={50}>每页 50 条</option>
      <option value={100}>每页 100 条</option>
      <option value={200}>每页 200 条</option>
    </select>
    <button class="pg" on:click={() => (page = 1)} disabled={page <= 1}>«</button>
    <button class="pg" on:click={() => (page -= 1)} disabled={page <= 1}>上一页</button>
    <span class="dim">第 {page} / {totalPages} 页</span>
    <button class="pg" on:click={() => (page += 1)} disabled={page >= totalPages}>下一页</button>
    <button class="pg" on:click={() => (page = totalPages)} disabled={page >= totalPages}>»</button>
  </div>

  {#if rows.length >= 2000}
    <div class="hint">已达单次查询上限（2000 条），建议缩小时间范围或按用户筛选后查看更早记录</div>
  {/if}
  <div class="hint">说明：登录/账户/计划任务类事件依赖 Windows 审核策略；本查询全程内存完成、无临时文件，离开本页结果即自动清空；筛选/搜索/翻页均在本地内存完成，不重复查询、不占用系统资源。</div>
</div>

<DetailModal
  open={!!detail}
  title={`事件详情${detail ? ` · ${kindLabel(detail.kind)} · ${detail.time}` : ""}`}
  sections={detail ? sections(detail) : []}
  on:close={() => (detail = null)}
/>

<style>
  .page { display: flex; flex-direction: column; gap: 12px; }
  .toolbar { display: flex; align-items: center; gap: 10px; }
  .toolbar h2 { margin: 0; font-size: 16px; }
  .toolbar select { padding: 5px 8px; border: 1px solid var(--stroke); border-radius: 8px; background: var(--panel-strong); }
  .btn { padding: 6px 16px; border: none; border-radius: 8px; background: var(--accent); color: #fff; cursor: pointer; font-size: 13px; }
  .btn:disabled { opacity: 0.6; cursor: default; }
  .filters { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; }
  .check { display: inline-flex; align-items: center; gap: 5px; font-size: 12.5px; cursor: pointer; }
  .check.strong { font-weight: 600; }
  .sep { width: 1px; height: 16px; background: var(--stroke); }
  .filters select, .filters input { padding: 5px 8px; border: 1px solid var(--stroke); border-radius: 8px; background: var(--panel-strong); font-size: 12.5px; }
  .filters .q { width: 230px; }
  .counts { font-size: 12.5px; }
  .bad-t { color: var(--bad); font-weight: 600; }
  .card { overflow: auto; max-height: 58vh; border: 1px solid var(--stroke-soft); border-radius: 12px; background: var(--panel); }
  table { width: 100%; border-collapse: collapse; font-size: 12.5px; }
  th { text-align: left; padding: 8px 10px; border-bottom: 1px solid var(--stroke); position: sticky; top: 0; background: var(--panel-strong); z-index: 1; }
  td { padding: 7px 10px; border-bottom: 1px solid var(--stroke-soft); vertical-align: top; }
  tbody tr:hover { background: var(--accent-bg); cursor: default; }
  .pager { display: flex; align-items: center; gap: 8px; font-size: 12.5px; }
  .pg { padding: 4px 10px; border: 1px solid var(--stroke); border-radius: 8px; background: var(--panel-strong); cursor: pointer; font-size: 12.5px; }
  .pg:disabled { opacity: 0.45; cursor: default; }
  .pg:not(:disabled):hover { background: var(--accent-bg); }
  .hint { font-size: 12px; color: var(--text-dim); }
</style>
