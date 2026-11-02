<script lang="ts">
  import { onMount } from "svelte";
  import { settingsGet, settingsSetAutoStart, settingsSetFlag, floatDockSet, alertSetRetention } from "./api";
  import type { AppSettings } from "./api";
  import { toast, pauseOnBlurEnabled } from "./stores";
  import { T, lang, setLang, type Lang } from "./i18n";

  let st: AppSettings | null = null;
  let busy = "";

  async function toggleAutoStart(on: boolean) {
    busy = "auto";
    try {
      await settingsSetAutoStart(on);
      if (st) st.auto_start = on;
      toast(on ? $T("set.toast.auto.on") : $T("set.toast.auto.off"), "ok");
    } catch (e) { toast(String(e), "err"); }
    finally { busy = ""; }
  }
  async function toggleFlag(name: "CloseToTray" | "PauseOnBlur", on: boolean) {
    busy = name;
    try {
      await settingsSetFlag(name, on);
      if (name === "PauseOnBlur") pauseOnBlurEnabled.set(on);
      if (st) {
        if (name === "CloseToTray") st.close_to_tray = on;
        else st.pause_on_blur = on;
      }
      toast($T("set.toast.saved"), "ok");
    } catch (e) { toast(String(e), "err"); }
    finally { busy = ""; }
  }

  async function toggleDiag(on: boolean) {
    busy = "diag";
    try {
      await settingsSetFlag("DiagLog", on);
      if (st) st.diag_log = on;
      toast(on ? $T("set.toast.diag.on") : $T("set.toast.diag.off"), "ok");
    } catch (e) { toast(String(e), "err"); }
    finally { busy = ""; }
  }

  async function toggleAlertMonitor(on: boolean) {
    busy = "amon";
    try {
      await settingsSetFlag("AlertMonitor", on);
      if (st) st.alert_monitor = on;
      toast(on ? $T("set.toast.amon.on") : $T("set.toast.amon.off"), "ok");
    } catch (e) { toast(String(e), "err"); }
    finally { busy = ""; }
  }

  async function changeRetention(days: number) {
    try {
      await alertSetRetention(days);
      if (st) st.alert_retention_days = days;
      toast($T("set.toast.ret", { n: $T(days === 7 ? "ret.week" : days === 30 ? "ret.month" : "ret.quarter") }), "ok");
    } catch (e) { toast(String(e), "err"); }
  }

  async function toggleFloat(on: boolean) {
    busy = "float";
    try {
      const msg: string = await floatDockSet(on);
      if (st) st.float_dock = on;
      toast(msg || (on ? "已开启" : "已关闭"), "ok");
    } catch (e) { toast(String(e), "err"); }
    finally { busy = ""; }
  }

  onMount(async () => {
    try { st = await settingsGet(); } catch (e) { toast(String(e), "err"); }
  });
</script>

<div class="view">
  <div class="card" style="flex:none">
    <div class="opt-group-title">{$T("set.group.lang")}</div>
    <div class="opt-row">
      <div class="info">
        <div class="t">{$lang === "zh" ? "界面语言" : "Interface language"}</div>
        <div class="d">{$lang === "zh" ? "切换后立即生效，仅影响本机界面文字" : "Applies immediately; affects UI text only"}</div>
      </div>
      <select value={$lang} on:change={(e) => setLang(e.currentTarget.value === "en" ? "en" : "zh")}>
        <option value="zh">中文</option>
        <option value="en">English</option>
      </select>
    </div>
  </div>

  <div class="card" style="flex:none">
    <div class="opt-group-title">{$T("set.group.startup")}</div>
    {#if st}
      <div class="opt-row">
        <div class="info">
          <div class="t">{$T("set.autostart")}</div>
          <div class="d">{$T("set.autostart.d")}</div>
        </div>
        <label class="switch">
          <input type="checkbox" checked={st.auto_start} disabled={busy === "auto"}
            on:change={(e) => toggleAutoStart(e.currentTarget.checked)} />
          <span class="track"></span>
        </label>
      </div>
      <div class="opt-row">
        <div class="info">
          <div class="t">{$T("set.closeToTray")}</div>
          <div class="d">{$T("set.closeToTray.d")}</div>
        </div>
        <label class="switch">
          <input type="checkbox" checked={st.close_to_tray} disabled={busy === "CloseToTray"}
            on:change={(e) => toggleFlag("CloseToTray", e.currentTarget.checked)} />
          <span class="track"></span>
        </label>
      </div>
      <div class="opt-row">
        <div class="info">
          <div class="t">{$T("set.float")}</div>
          <div class="d">{$T("set.float.d")}</div>
        </div>
        <label class="switch">
          <input type="checkbox" checked={st.float_dock} disabled={busy === "float"}
            on:change={(e) => toggleFloat(e.currentTarget.checked)} />
          <span class="track"></span>
        </label>
      </div>
      <div class="opt-row">
        <div class="info">
          <div class="t">{$T("set.pauseOnBlur")}</div>
          <div class="d">{$T("set.pauseOnBlur.d")}</div>
        </div>
        <label class="switch">
          <input type="checkbox" checked={st.pause_on_blur} disabled={busy === "PauseOnBlur"}
            on:change={(e) => toggleFlag("PauseOnBlur", e.currentTarget.checked)} />
          <span class="track"></span>
        </label>
      </div>
      <div class="opt-row">
        <div class="info">
          <div class="t">{$T("set.diag")}</div>
          <div class="d">{$T("set.diag.d")}</div>
        </div>
        <label class="switch">
          <input type="checkbox" checked={st.diag_log} disabled={busy === "diag"}
            on:change={(e) => toggleDiag(e.currentTarget.checked)} />
          <span class="track"></span>
        </label>
      </div>
    {:else}
      <div class="opt-row"><div class="info"><div class="d">{$T("set.loading")}</div></div></div>
    {/if}
  </div>

  <div class="card" style="flex:none">
    <div class="opt-group-title">{$T("set.group.alert")}</div>
    {#if st}
      <div class="opt-row">
        <div class="info">
          <div class="t">{$T("set.sysmon")}</div>
          <div class="d">{$T("set.sysmon.d")}</div>
        </div>
        <label class="switch">
          <input type="checkbox" checked={st.alert_monitor} disabled={busy === "amon"}
            on:change={(e) => toggleAlertMonitor(e.currentTarget.checked)} />
          <span class="track"></span>
        </label>
      </div>
      <div class="opt-row">
        <div class="info">
          <div class="t">{$T("set.retention")}</div>
          <div class="d">{$T("set.retention.d")}</div>
        </div>
        <select disabled={busy === "amon"} value={st.alert_retention_days}
          on:change={(e) => changeRetention(Number(e.currentTarget.value))}>
          <option value={7}>{$T("set.ret.7")}</option>
          <option value={30}>{$T("set.ret.30")}</option>
          <option value={90}>{$T("set.ret.90")}</option>
        </select>
      </div>
    {:else}
      <div class="opt-row"><div class="info"><div class="d">加载中…</div></div></div>
    {/if}
  </div>

  <div class="card" style="flex:none">
    <div class="opt-group-title">{$T("set.group.res")}</div>
    <div class="opt-row">
      <div class="info">
        <div class="d" style="line-height:1.8; white-space:pre-line">{$T("set.res.body")}</div>
      </div>
    </div>
  </div>
</div>
