<script lang="ts">
  import { onMount } from "svelte";
  import { settingsGet, settingsSetAutoStart, settingsSetFlag, floatDockSet, alertSetRetention } from "./api";
  import type { AppSettings } from "./api";
  import { toast, pauseOnBlurEnabled } from "./stores";

  let st: AppSettings | null = null;
  let busy = "";

  async function toggleAutoStart(on: boolean) {
    busy = "auto";
    try {
      await settingsSetAutoStart(on);
      if (st) st.auto_start = on;
      toast(on ? "已加入开机启动（当前用户，无需管理员）" : "已取消开机启动", "ok");
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
      toast("已保存", "ok");
    } catch (e) { toast(String(e), "err"); }
    finally { busy = ""; }
  }

  async function toggleDiag(on: boolean) {
    busy = "diag";
    try {
      await settingsSetFlag("DiagLog", on);
      if (st) st.diag_log = on;
      toast(on ? "诊断日志已开启（data\\logs\\diag.log）" : "诊断日志已关闭，日志文件已删除", "ok");
    } catch (e) { toast(String(e), "err"); }
    finally { busy = ""; }
  }

  async function toggleAlertMonitor(on: boolean) {
    busy = "amon";
    try {
      await settingsSetFlag("AlertMonitor", on);
      if (st) st.alert_monitor = on;
      toast(on ? "威胁预警监控已开启（需本机安装 Sysmon，事件驱动零轮询）" : "威胁预警监控已关闭，立即生效", "ok");
    } catch (e) { toast(String(e), "err"); }
    finally { busy = ""; }
  }

  async function changeRetention(days: number) {
    try {
      await alertSetRetention(days);
      if (st) st.alert_retention_days = days;
      toast(`告警保留期已设为 ${days === 7 ? "一周" : days === 30 ? "一个月" : "三个月"}，过期数据已即时清除`, "ok");
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
    <div class="opt-group-title">启动与托盘</div>
    {#if st}
      <div class="opt-row">
        <div class="info">
          <div class="t">开机自动启动</div>
          <div class="d">写入当前用户注册表 Run 键（不建服务/计划任务，零常驻开销）；取消勾选即还原</div>
        </div>
        <label class="switch">
          <input type="checkbox" checked={st.auto_start} disabled={busy === "auto"}
            on:change={(e) => toggleAutoStart(e.currentTarget.checked)} />
          <span class="track"></span>
        </label>
      </div>
      <div class="opt-row">
        <div class="info">
          <div class="t">关闭按钮隐藏到托盘</div>
          <div class="d">点击 ✕ 时隐藏窗口并保留托盘图标，右键托盘图标可彻底退出；配合开机启动可后台待命</div>
        </div>
        <label class="switch">
          <input type="checkbox" checked={st.close_to_tray} disabled={busy === "CloseToTray"}
            on:change={(e) => toggleFlag("CloseToTray", e.currentTarget.checked)} />
          <span class="track"></span>
        </label>
      </div>
      <div class="opt-row">
        <div class="info">
          <div class="t">贴边浮漂（迷你 UFO）</div>
          <div class="d">屏幕右缘趴着一艘迷你三角 UFO：关闭主窗自动停靠贴边，点浮标一键呼出主窗，不遮挡屏幕</div>
        </div>
        <label class="switch">
          <input type="checkbox" checked={st.float_dock} disabled={busy === "float"}
            on:change={(e) => toggleFloat(e.currentTarget.checked)} />
          <span class="track"></span>
        </label>
      </div>
      <div class="opt-row">
        <div class="info">
          <div class="t">窗口失焦时暂停自动刷新</div>
          <div class="d">资源占用核心策略：窗口不在前台/被最小化时，进程、网络等页面的轮询自动空转，切回前台立即恢复，CPU 占用趋近于 0</div>
        </div>
        <label class="switch">
          <input type="checkbox" checked={st.pause_on_blur} disabled={busy === "PauseOnBlur"}
            on:change={(e) => toggleFlag("PauseOnBlur", e.currentTarget.checked)} />
          <span class="track"></span>
        </label>
      </div>
      <div class="opt-row">
        <div class="info">
          <div class="t">诊断日志</div>
          <div class="d">记录预警链路 / 端到端自检 / 健康巡检等 debug 明细到 data\logs\diag.log，默认关闭——不开启则完全不产生日志；关闭时会自动删除已有日志文件</div>
        </div>
        <label class="switch">
          <input type="checkbox" checked={st.diag_log} disabled={busy === "diag"}
            on:change={(e) => toggleDiag(e.currentTarget.checked)} />
          <span class="track"></span>
        </label>
      </div>
    {:else}
      <div class="opt-row"><div class="info"><div class="d">加载中…</div></div></div>
    {/if}
  </div>

  <div class="card" style="flex:none">
    <div class="opt-group-title">威胁预警</div>
    {#if st}
      <div class="opt-row">
        <div class="info">
          <div class="t">Sysmon 威胁监控</div>
          <div class="d">基于 Windows 事件订阅的高风险行为告警（事件驱动、零轮询、无临时文件）；需本机安装 Sysmon，规则可在「威胁预警」页配置</div>
        </div>
        <label class="switch">
          <input type="checkbox" checked={st.alert_monitor} disabled={busy === "amon"}
            on:change={(e) => toggleAlertMonitor(e.currentTarget.checked)} />
          <span class="track"></span>
        </label>
      </div>
      <div class="opt-row">
        <div class="info">
          <div class="t">告警保留期</div>
          <div class="d">自动清除 alerts.json（与工具同目录）中多久之前的告警数据，变更即时生效并立即清理</div>
        </div>
        <select disabled={busy === "amon"} value={st.alert_retention_days}
          on:change={(e) => changeRetention(Number(e.currentTarget.value))}>
          <option value={7}>一周前</option>
          <option value={30}>一个月前</option>
          <option value={90}>三个月前</option>
        </select>
      </div>
    {:else}
      <div class="opt-row"><div class="info"><div class="d">加载中…</div></div></div>
    {/if}
  </div>

  <div class="card" style="flex:none">
    <div class="opt-group-title">资源占用说明</div>
    <div class="opt-row">
      <div class="info">
        <div class="d" style="line-height:1.8">
          · 无任何后台服务 / 驱动 / 常驻进程，关闭窗口（未开托盘/浮漂驻留）即完全退出<br />
          · 托盘仅为消息泵，无轮询线程；网络监控走 iphlpapi 内核 API（毫秒级、无 PowerShell 开销）<br />
          · 自动刷新定时器仅在其页面处于前台可见时运行，失焦自动暂停（可关闭此策略）<br />
          · IP 归属地为离线库查询，数据不出本机<br />
          · 启动时自动启用 SeDebug / SeTakeOwnership / SeBackup / SeRestore 等管理特权（用户态最高权限组合）
        </div>
      </div>
    </div>
  </div>
</div>
