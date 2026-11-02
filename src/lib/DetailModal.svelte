<script context="module" lang="ts">
  export interface DetailItem { k: string; v: string | number; wide?: boolean; }
  export interface DetailSection { title: string; items: DetailItem[]; }
  export function sigText(status: string): string {
    if (status === "Valid") return "✔ 已签名";
    if (status === "NotSigned") return "⚠ 未签名";
    if (status === "Invalid") return "✗ 签名无效";
    return status;
  }
</script>

<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { copyText, toast } from "./stores";

  export let open = false;
  export let title = "";
  export let sections: DetailSection[] = [];

  const dispatch = createEventDispatcher<{ close: void }>();
  function close() {
    open = false;
    dispatch("close");
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape" && open) close();
  }
  // 双击任意行 → 复制该行完整文本（"标签：值"），无需选中
  async function onDblCopy(it: DetailItem) {
    const text = `${it.k}：${it.v}`;
    try {
      await copyText(text);
      toast("复制成功", "ok");
    } catch (e) {
      toast(String(e), "err");
    }
  }
</script>

<svelte:window on:keydown={onKey} />
{#if open}
  <!-- svelte-ignore a11y-no-noninteractive-element-interactions a11y-mouse-events-have-key-events -->
  <div class="dmask" role="dialog" aria-modal="true" on:mousedown|self={close}>
    <div class="dbox">
      <div class="dhead"><b>{title}</b><button class="dx" title="关闭 (Esc)" on:click={close}>×</button></div>
      <div class="dbody">
        {#each sections as sec}
          {#if sec.items.some((i) => i.v !== "" && i.v != null)}
            <div class="dsec"><b>{sec.title}</b></div>
            <div class="dgrid">
              {#each sec.items as it}
                {#if it.v !== "" && it.v != null}
                  <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-noninteractive-element-interactions a11y-no-static-element-interactions -->
              <!-- svelte-ignore a11y-mouse-events-have-key-events -->
              <!-- svelte-ignore a11y-no-noninteractive-tabindex -->
                  <div class="dcell" class:wide={it.wide} title="双击复制该行" on:dblclick={() => onDblCopy(it)}>
                    <span class="dk"><b>{it.k}</b></span>
                    <span class="dv">{it.v}</span>
                  </div>
                {/if}
              {/each}
            </div>
          {/if}
        {/each}
      </div>
    </div>
  </div>
{/if}
