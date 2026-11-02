<script lang="ts">
  import { onMount } from "svelte";
  import { getTasks, taskAction } from "./api";
  import type { TaskInfo } from "./api";

  let detail: TaskInfo | null = null;
  function taskSections(v: TaskInfo): DetailSection[] {
    return [
      { title: "基本信息", items: [
        { k: "任务名", v: v.name }, { k: "完整路径", v: v.path, wide: true },
        { k: "状态", v: v.state }, { k: "触发器数", v: v.trigger_count },
        { k: "作者", v: v.author || "（未记录）" },
      ]},
      { title: "执行动作", items: [
        { k: "程序", v: v.exe, wide: true }, { k: "参数", v: v.args, wide: true },
      ]},
    ];
  }
  import { toast, askConfirm, rowCtx, copyText } from "./stores";
  import DetailModal from "./DetailModal.svelte";
  import type { DetailSection } from "./DetailModal.svelte";

  let rows: TaskInfo[] = [];
  let q = "";
  let loading = false;
  let err = "";

  async function load() {
    loading = true; err = "";
    try { rows = await getTasks(); } catch (e) { err = String(e); } finally { loading = false; }
  }
  onMount(load);

  $: filtered = rows
    .filter((t) => !q || (t.name || "").toLowerCase().includes(q.toLowerCase()) || (t.exe || "").toLowerCase().includes(q.toLowerCase()) || (t.path || "").toLowerCase().includes(q.toLowerCase()))
    .sort((a, b) => (a.path || "").localeCompare(b.path || "") || (a.name || "").localeCompare(b.name || ""));

  async function act(t: TaskInfo, action: string) {
    if (action === "delete" && !(await askConfirm(`确定删除计划任务「${t.path}${t.name}」？`, true))) return;
    try { toast(await taskAction(t.path, t.name, action), "ok"); load(); }
    catch (e) { toast(String(e), "err"); }
  }
</script>

<div class="view">
  <div class="toolbar">
    <input class="search" placeholder="搜索 任务名 / 命令 / 路径…" bind:value={q} />
    <button class="btn primary" on:click={load} disabled={loading}>{loading ? "加载中…" : "刷新"}</button>
    <span class="dim">非 Microsoft 任务：{filtered.length}</span>
  </div>

  {#if err}<div class="banner">加载失败：{err}</div>{/if}

  <div class="card">
    <div class="tbl-wrap">
      <table class="tbl fix">
        <thead>
          <tr>
            <th style="width:24%">任务</th><th style="width:10%">状态</th><th style="width:9%">触发器</th><th style="width:16%">作者</th><th>命令</th><th style="width:44px"></th>
          </tr>
        </thead>
        <tbody>
          {#each filtered as t (t.path + t.name)}
            {@const items = [
              { label: "查看详细信息", action: () => (detail = t) },
              { label: "运行任务", action: () => act(t, "run") },
              { label: "结束任务", action: () => act(t, "end") },
              ...(t.state === "Disabled"
                ? [{ label: "启用任务", action: () => act(t, "enable") }]
                : [{ label: "禁用任务", action: () => act(t, "disable") }]),
              { label: "删除任务", action: () => act(t, "delete"), danger: true },
              { label: "复制任务名", action: () => copyText(t.name) },
              { label: "复制命令", action: () => copyText((t.exe || "") + (t.args ? " " + t.args : "")) }
            ]}
            <tr on:contextmenu={(e) => rowCtx(e, items)} on:dblclick={() => (detail = t)}>
              <td title={t.path + t.name}><b>{t.name}</b><span class="dim mono" style="margin-left:6px">{t.path}</span></td>
              <td>
                {#if t.state === "Ready"}<span class="chip ok">就绪</span>
                {:else if t.state === "Running"}<span class="chip running">运行中</span>
                {:else if t.state === "Disabled"}<span class="chip disabled">已禁用</span>
                {:else}<span class="chip">{t.state || "-"}</span>{/if}
              </td>
              <td class="mono dim">{t.trigger_count}</td>
              <td class="dim" title={t.author || ""}>{t.author || "-"}</td>
              <td class="mono" title={t.exe || ""}>{t.exe || "-"}{t.args ? " " + t.args : ""}</td>
              <td><button class="btn mini" title="操作菜单（或右键行）" on:click={(e) => rowCtx(e, items)}>⋮</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if loading && rows.length === 0}
        <div class="empty">正在加载计划任务（首次枚举可能需要数十秒）…</div>
      {:else if filtered.length === 0}
        <div class="empty">没有匹配的计划任务</div>
      {/if}
    </div>
    <div class="footer">
      <span>仅列出非 \Microsoft 路径的任务（由服务端过滤）</span>
      <span class="dim" style="margin-left:auto">可疑项：隐藏路径、以系统权限自启、命令指向非常规位置</span>
    </div>
  </div>
</div>
<DetailModal
  open={!!detail}
  title={`计划任务详情${detail ? " · " + detail.name : ""}`}
  sections={detail ? taskSections(detail) : []}
  on:close={() => (detail = null)}
/>
