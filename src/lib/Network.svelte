<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { netConnections, geoGetStatus, geoSetDbPath, geoLookupIps, killProcess, openUrl } from "./api";
  import type { NetConn, GeoStatus } from "./api";
  import { toast, askConfirm, rowCtx, copyText, appPaused } from "./stores";

  let rows: NetConn[] = [];
  let geo = new Map<string, string>();
  let geoSt: GeoStatus | null = null;
  let externalOnly = true;
  let establishedOnly = true;
  let q = "";
  let auto = true;
  let timer: ReturnType<typeof setInterval>;
  let loading = false;
  let err = "";
  let paused = false;
  const unsubPaused = appPaused.subscribe((p) => (paused = p));

  $: isExternal = (ip: string): boolean => {
    const m = /^(\d+)\.(\d+)\.(\d+)\.(\d+)/.exec(ip);
    if (!m) return /^\[\w:/.test(ip) && !/^\[(::1|f[cd]|fe80)/i.test(ip);
    const [a, b] = [+m[1], +m[2]];
    if (a === 10 || a === 127 || a === 0) return false;
    if (a === 172 && b >= 16 && b <= 31) return false;
    if (a === 192 && b === 168) return false;
    if (a === 169 && b === 254) return false;
    if (a === 100 && b >= 64 && b <= 127) return false;
    return true;
  };

  $: filtered = rows.filter(
    (r) =>
      (!establishedOnly || r.state === "ESTABLISHED") &&
      (!externalOnly || isExternal(r.remote.split(":").slice(0, -1).join(":"))) &&
      (!q ||
        r.remote.toLowerCase().includes(q.toLowerCase()) ||
        r.proc_name.toLowerCase().includes(q.toLowerCase()) ||
        String(r.pid).includes(q))
  );

  async function load() {
    if (paused) return;
    loading = true;
    try {
      rows = await netConnections();
      err = "";
      // 为可见外联 IP 批量查询归属地（带本地缓存）
      const ips = [...new Set(filtered.map((r) => r.remote.split(":").slice(0, -1).join(":")))]
        .filter((ip) => ip && ip !== "0.0.0.0" && !geo.has(ip))
        .slice(0, 200);
      if (ips.length) {
        const r = await geoLookupIps(ips);
        for (const [k, v] of Object.entries(r)) geo.set(k, v);
        geo = geo; // 触发响应
      }
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
  }

  async function reloadGeo() {
    try {
      geoSt = await geoSetDbPath("");
      geo = new Map();
      toast(geoSt.found ? "IP 库已加载" : geoSt.msg, geoSt.found ? "ok" : "info");
    } catch (e) { toast(String(e), "err"); }
  }

  function toggleAuto() {
    auto = !auto;
    clearInterval(timer);
    if (auto) timer = setInterval(load, 2000);
  }

  function connMenu(e: MouseEvent, r: NetConn) {
    const ip = r.remote.split(":").slice(0, -1).join(":");
    rowCtx(e, [
      { label: "复制远程地址", action: () => copyText(r.remote) },
      { label: "复制远程 IP", action: () => copyText(ip) },
      { label: "复制归属地", action: () => copyText(geo.get(ip) || "") },
      { label: "🔴 微步查询 IP", action: async () => {
          if (!isExternal(ip)) return toast("内网地址无需威胁情报查询", "info");
          try { await openUrl(`https://x.threatbook.com/v5/ip/${ip}`); toast("已在浏览器打开微步查询", "ok"); }
          catch (e) { toast(String(e), "err"); }
        } },
      { label: "🟢 VirusTotal 查询 IP", action: async () => {
          if (!isExternal(ip)) return toast("内网地址无需威胁情报查询", "info");
          try { await openUrl(`https://www.virustotal.com/gui/ip-address/${ip}`); toast("已在浏览器打开 VirusTotal", "ok"); }
          catch (e) { toast(String(e), "err"); }
        } },
      { label: `结束进程 ${r.proc_name} (PID ${r.pid})`, action: async () => {
          if (r.pid < 10) return toast("拒绝操作关键系统进程", "err");
          if (await askConfirm(`确定结束「${r.proc_name}」(PID ${r.pid})？`, true)) {
            try { await killProcess(r.pid); toast("已结束", "ok"); load(); } catch (e) { toast(String(e), "err"); }
          }
        }, danger: true }
    ]);
  }

  onMount(async () => {
    load();
    timer = setInterval(load, 2000);
    try { geoSt = await geoGetStatus(); } catch {}
  });
  onDestroy(() => { clearInterval(timer); unsubPaused(); });
</script>

<div class="view">
  <div class="toolbar">
    <input class="search" placeholder="搜索 IP / 进程 / PID…" bind:value={q} />
    <label class="chk"><input type="checkbox" bind:checked={externalOnly} /> 仅外联</label>
    <label class="chk"><input type="checkbox" bind:checked={establishedOnly} /> 仅已连接</label>
    <button type="button" class="btn" class:primary={auto} on:click={toggleAuto}>
      {auto ? "⏸ 停止刷新" : "▶ 自动刷新(2s)"}
    </button>
    <button class="btn" on:click={load} disabled={loading}>{loading ? "刷新中…" : "刷新"}</button>
    <span class="dim">外联 {filtered.length} / 全部 {rows.length}</span>
  </div>

  {#if err}<div class="banner">{err}</div>{/if}
  {#if geoSt && !geoSt.found}
    <div class="banner banner-warn">📍 {geoSt.msg}　获取 ip2region.xdb：github.com/lionsoul2014/ip2region（v2.0+ xdb 格式）</div>
  {:else if geoSt}
    <div class="banner banner-ok" style="max-height:40px">📍 {geoSt.msg}</div>
  {/if}  <div class="card">
    <div class="tbl-wrap">
      <table class="tbl fix">
        <thead>
          <tr>
            <th style="width:110px">状态</th>
            <th style="width:24%">本地地址</th>
            <th style="width:26%">远程地址</th>
            <th>归属地（IP 库）</th>
            <th style="width:16%">进程</th>
            <th style="width:34px"></th>
          </tr>
        </thead>
        <tbody>
          {#each filtered as r (r.local + r.remote + r.pid)}
            <tr on:contextmenu={(e) => connMenu(e, r)}>
              <td><span class="chip {r.state === 'ESTABLISHED' ? 'ok' : r.state === 'LISTEN' ? '' : 'ms'}">{r.state}</span></td>
              <td class="mono" title={r.local}>{r.local}</td>
              <td class="mono" title={r.remote}>{r.remote}</td>
              <td class="dim" title={geo.get(r.remote.split(':').slice(0, -1).join(':')) || ''}>
                {#if externalOnly || isExternal(r.remote.split(':').slice(0, -1).join(':'))}
                  {geo.get(r.remote.split(':').slice(0, -1).join(':')) || "—"}
                {:else}内网{/if}
              </td>
              <td title={r.proc_name + ' (PID ' + r.pid + ')'}>{r.proc_name}</td>
              <td><button class="btn mini" title="操作菜单" on:click={(e) => connMenu(e, r)}>⋮</button></td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if filtered.length === 0 && !loading}
        <div class="empty">{q ? "没有匹配的连接" : "没有符合筛选条件的连接"}</div>
      {/if}
    </div>
    <div class="footer">
      <span>TCP 连接（含 IPv4 / IPv6）· 右键行可复制/结束进程</span>
      <span class="dim" style="margin-left:auto">归属地数据：ip2region 离线库，纯本地查询</span>
    </div>
  </div>
</div>
