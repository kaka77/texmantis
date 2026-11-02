<script lang="ts">
  import { toast } from "./stores";
  import { extractData, EXTRACT_LABELS } from "./extract";
  import type { ExtractKind } from "./extract";

  const ALL: ExtractKind[] = ["ipv4", "ipv6", "url", "domain", "hostname"];
  let selected: Record<ExtractKind, boolean> = { ipv4: true, ipv6: true, url: true, domain: true, hostname: true };
  let dedupe = true;
  let sort = false;
  let input = "";
  let output = "";
  let counts: Partial<Record<ExtractKind, number>> = {};
  let ran = false;

  $: selectedKinds = ALL.filter((k) => selected[k]);

  function run() {
    if (!input.trim()) return toast("请先粘贴待分析的数据", "info");
    if (!selectedKinds.length) return toast("请至少选择一种提取类型", "info");
    const r = extractData(input, selectedKinds, { dedupe, sort });
    output = r.items.join("\n");
    counts = r.counts;
    ran = true;
    toast(`提取完成：共 ${r.items.length} 条`, "ok");
  }

  async function copyOut() {
    if (!output) return toast("没有可复制的结果", "info");
    try {
      await navigator.clipboard.writeText(output);
      toast("已复制到剪贴板", "ok");
    } catch {
      toast("复制失败，请手动选择文本复制", "err");
    }
  }
  function clearAll() {
    input = ""; output = ""; counts = {}; ran = false;
  }
  function swap() {
    input = output;
    output = "";
  }
</script>

<div class="view">
  <div class="toolbar">
    {#each ALL as k (k)}
      <label class="chk"><input type="checkbox" bind:checked={selected[k]} /> {EXTRACT_LABELS[k]}</label>
    {/each}
    <span style="width:1px;height:18px;background:var(--stroke);display:inline-block"></span>
    <label class="chk"><input type="checkbox" bind:checked={dedupe} /> 去重</label>
    <label class="chk"><input type="checkbox" bind:checked={sort} /> 排序</label>
    <button class="btn primary" on:click={run}>⚡ 提取</button>
    <button class="btn" on:click={copyOut} disabled={!output}>📋 复制结果</button>
    <button class="btn" on:click={swap} disabled={!output}>⇅ 交换</button>
    <button class="btn danger" on:click={clearAll}>🗑 清空</button>
  </div>

  <div class="card" style="flex:none">
    <div style="padding:10px; display:flex; flex-direction:column; gap:8px">
      <textarea class="ta" bind:value={input}
        placeholder="粘贴日志 / 流量记录 / 配置文件等复杂数据，提取 IPv4 · IPv6 · URL · 域名 · 主机名（Ctrl+Enter 运行）"
        style="min-height:140px"
        on:keydown={(e) => e.ctrlKey && e.key === "Enter" && run()}></textarea>
      <textarea class="ta" bind:value={output} readonly
        placeholder="提取结果（一行一个）…" style="min-height:170px"></textarea>
    </div>
    <div class="footer">
      <span>{ran ? `共提取 ${output ? output.split("\n").length : 0} 条` : "等待提取"}</span>
      <span style="margin-left:auto; display:flex; gap:6px">
        {#each selectedKinds as k (k)}
          <span class="chip">{EXTRACT_LABELS[k]} {counts[k] ?? 0}</span>
        {/each}
      </span>
    </div>
  </div>

  <div class="card" style="flex:none">
    <div class="footer" style="border-top:none">
      <span>说明：结果一行一个，已按所选类型与出现顺序输出；「去重」跨类型合并相同项。</span>
      <span class="dim" style="margin-left:auto">主机名按上下文识别（UNC 路径 / Computer= / Hostname= / 常见命名形态）；域名按 FQDN 形态提取，文件名可能产生少量噪声</span>
    </div>
  </div>
</div>
