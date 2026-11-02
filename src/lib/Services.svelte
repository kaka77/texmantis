<script lang="ts">
  import { onMount } from "svelte";
  import { getServices, serviceAction } from "./api";
  import type { SvcInfo } from "./api";

  let detail: SvcInfo | null = null;
  function svcSections(v: SvcInfo): DetailSection[] {
    return [
      { title: "基本信息", items: [
        { k: "服务名", v: v.name }, { k: "显示名", v: v.display },
        { k: "运行状态", v: v.state }, { k: "启动类型", v: v.start_mode },
      ]},
      { title: "账户与进程", items: [
        { k: "登录账户", v: v.account || "LocalSystem" }, { k: "宿主 PID", v: v.pid ?? "（未运行）" },
      ]},
      { title: "可执行文件", items: [
        { k: "路径", v: v.exe, wide: true }, { k: "厂商", v: v.company },
        { k: "厂商甄别", v: v.is_microsoft ? "微软体系组件" : "第三方/未知厂商（重点关注）" },
      ]},
    ];
  }
  import { toast, askConfirm, rowCtx, copyText } from "./stores";
  import DetailModal from "./DetailModal.svelte";
  import type { DetailSection } from "./DetailModal.svelte";

  let rows: SvcInfo[] = [];
  let q = "";
  let thirdOnly = true;
  let loading = false;
  let err = "";

  async function load() {
    loading = true; err = "";
    try { rows = await getServices(); } catch (e) { err = String(e); } finally { loading = false; }
  }
  onMount(load);

  $: filtered = rows
    .filter((s) => !thirdOnly || !s.is_microsoft)
    .filter((s) => !q || s.name.toLowerCase().includes(q.toLowerCase()) || s.display.toLowerCase().includes(q.toLowerCase()) || s.exe.toLowerCase().includes(q.toLowerCase()))
    .sort((a, b) => (a.is_microsoft === b.is_microsoft ? a.name.localeCompare(b.name) : a.is_microsoft ? 1 : -1));

  const startLabel = (s: SvcInfo) => ({ Auto: "自动", Manual: "手动", Disabled: "已禁用" } as Record<string, string>)[s.start_mode] ?? s.start_mode;
  const stateChip = (s: SvcInfo) =>
    s.state === "Running" ? `<span class="chip running">运行中</span>` : `<span class="chip disabled">已停止</span>`;

  async function act(s: SvcInfo, action: string) {
    const labels: Record<string, string> = { stop: "停止", start: "启动", disabled: "禁用", auto: "设为自动", manual: "设为手动", delete: "删除" };
    if (action === "delete" && !(await askConfirm(`⚠️ 将删除服务「${s.name}」，此操作不可恢复！确定？`, true))) return;
    try { toast(await serviceAction(s.name, action), "ok"); load(); }
    catch (e) { toast(String(e), "err"); }
  }
</script>

<div class="view">
  <div class="toolbar">
    <input class="search" placeholder="搜索 服务名 / 显示名 / 路径…" bind:value={q} />
    <label class="chk"><input type="checkbox" bind:checked={thirdOnly} /> 仅显示非微软服务</label>
    <button class="btn primary" on:click={load} disabled={loading}>{loading ? "加载中…" : "刷新"}</button>
    <span class="dim">第三方 {filtered.length} / 全部 {rows.length}</span>
  </div>

  {#if err}<div class="banner">加载失败：{err}</div>{/if}

  <div class="card">
    <div class="tbl-wrap">
      <table class="tbl fix">
        <thead>
          <tr>
            <th style="width:32%">服务</th><th style="width:11%">状态</th><th style="width:11%">启动类型</th><th style="width:13%">厂商</th><th>可执行文件</th><th style="width:44px"></th>
          </tr>
        </thead>
        <tbody>
          {#each filtered as s (s.name)}
            {@const items = [
              { label: "查看详细信息", action: () => (detail = s) },
              ...(s.state === "Running"
                ? [{ label: "停止服务", action: () => act(s, "stop") }]
                : [{ label: "启动服务", action: () => act(s, "start") }]),
              { label: "设为自动启动", action: () => act(s, "auto") },
              { label: "设为手动启动", action: () => act(s, "manual") },
              { label: "禁用服务", action: () => act(s, "disabled") },
              { label: "删除服务", action: () => act(s, "delete"), danger: true },
              { label: "复制服务名", action: () => copyText(s.name) },
              { label: "复制可执行文件路径", action: () => copyText(s.exe) }
            ]}
            <tr on:contextmenu={(e) => rowCtx(e, items)} on:dblclick={() => (detail = s)}>
              <td title={`${s.name} · ${s.display} · ${s.account} · PID ${s.pid ?? "-"}`}>
                <b>{s.name}</b><span class="dim"> · {s.display}</span>
              </td>
              <td>
                {#if s.state === "Running"}<span class="chip ok">运行中</span>
                {:else if s.state === "Stopped"}<span class="chip">已停止</span>
                {:else}<span class="chip ms">{s.state}</span>{/if}
              </td>
              <td class="dim">{startLabel(s)}</td>
              <td>
                {#if s.is_microsoft}<span class="chip ms">Microsoft</span>
                {:else}<span class="chip third">第三方</span>{/if}
              </td>
              <td class="mono" title={s.exe}>{s.exe || "-"}</td>
              <td><button class="btn mini" title="操作菜单（或右键行）" on:click={(e) => rowCtx(e, items)}>⋮</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if filtered.length === 0}<div class="empty">没有匹配的服务</div>{/if}
    </div>
    <div class="footer">
      <span>默认只看第三方服务，重点排查：路径异常 / 无签名 / 自启可疑服务</span>
    </div>
  </div>
</div>
<DetailModal
  open={!!detail}
  title={`服务详情${detail ? " · " + detail.name : ""}`}
  sections={detail ? svcSections(detail) : []}
  on:close={() => (detail = null)}
/>
