<script lang="ts">
  import { toast } from "./stores";

  let mode: "base64" | "url" = "base64";
  let input = "";
  let output = "";

  function b64encode(s: string): string {
    const bytes = new TextEncoder().encode(s);
    let bin = "";
    const CH = 0x8000;
    for (let i = 0; i < bytes.length; i += CH)
      bin += String.fromCharCode(...bytes.subarray(i, i + CH));
    return btoa(bin);
  }
  function b64decode(s: string): string {
    const bin = atob(s.replace(/\s+/g, ""));
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    return new TextDecoder().decode(bytes);
  }
  function urlencode(s: string): string {
    return encodeURIComponent(s);
  }
  function urldecode(s: string): string {
    return decodeURIComponent(s.replace(/\+/g, "%20"));
  }

  function run(dir: "enc" | "dec") {
    if (!input) return toast("请先输入内容", "info");
    try {
      if (mode === "base64")
        output = dir === "enc" ? b64encode(input) : b64decode(input);
      else
        output = dir === "enc" ? urlencode(input) : urldecode(input);
      toast(dir === "enc" ? "编码完成" : "解码完成", "ok");
    } catch (e) {
      toast(`处理失败：${e instanceof Error ? e.message : String(e)}`, "err");
    }
  }

  function swap() {
    input = output;
    output = "";
  }
  function clearAll() {
    input = ""; output = "";
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
  const placeholder = () =>
    mode === "base64" ? "输入文本编码为 Base64，或粘贴 Base64 解码…" : "输入文本进行 URL 编码，或粘贴 %XX 序列解码…";
</script>

<div class="view">
  <div class="toolbar">
    <button class="btn {mode === 'base64' ? 'primary' : ''}" on:click={() => { mode = "base64"; }}>Base64</button>
    <button class="btn {mode === 'url' ? 'primary' : ''}" on:click={() => { mode = "url"; }}>URL</button>
    <span class="dim" style="margin-left:auto">{mode === "base64" ? "支持中文等多字节字符（UTF-8）" : "空格按 + 处理可先 Base64；解码时 + 视为空格"}</span>
  </div>

  <div class="card" style="flex:none">
    <div style="padding:10px; display:flex; flex-direction:column; gap:8px">
      <textarea class="ta" bind:value={input} placeholder={placeholder()} style="min-height:110px"></textarea>
      <div style="display:flex; gap:8px; justify-content:center; flex-wrap:wrap">
        <button class="btn primary" on:click={() => run("enc")}>↓ 编码 ↓</button>
        <button class="btn primary" on:click={() => run("dec")}>↑ 解码 ↑</button>
        <button class="btn" on:click={swap}>⇅ 交换</button>
        <button class="btn" on:click={copyOut}>📋 复制结果</button>
        <button class="btn danger" on:click={clearAll}>🗑 清空</button>
      </div>
      <textarea class="ta" bind:value={output} placeholder="结果输出区…" style="min-height:110px" readonly></textarea>
    </div>
    <div class="footer">
      <span>纯本地处理，数据不出本机</span>
    </div>
  </div>
</div>
