<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { view } from "./stores";
  import type { View } from "./stores";

  // 拟真悬停：静置期保持正立 + 缓慢沉浮（标志始终可见），随机间隔优雅转一整圈后归位——离开页面即沉默
  let markEl: SVGSVGElement;
  let rafId = 0;
  let phase: "rest" | "spin" = "rest";
  let phaseUntil = 0;
  let spinStart = 0;
  const SPIN_MS = 2800; // 一整圈 2.8 秒，缓入缓出
  function tick(t: number) {
    if (markEl) {
      const bob = Math.sin(t / 900) * 2.2; // 拟真缓慢沉浮
      let rot = 0;
      if (phase === "rest") {
        if (t > phaseUntil) {
          phase = "spin";
          spinStart = t;
        }
      } else {
        const p = Math.min(1, (t - spinStart) / SPIN_MS);
        const e = p < 0.5 ? 2 * p * p : 1 - Math.pow(-2 * p + 2, 2) / 2; // easeInOutQuad
        rot = 360 * e;
        if (p >= 1) {
          phase = "rest";
          phaseUntil = t + 3800 + Math.random() * 5200; // 下一次自转时刻随机
        }
      }
      markEl.style.transform = `translateY(${bob.toFixed(2)}px) rotate(${rot.toFixed(2)}deg)`;
    }
    rafId = requestAnimationFrame(tick);
  }
  onMount(() => {
    phaseUntil = performance.now() + 2200 + Math.random() * 3000;
    rafId = requestAnimationFrame(tick);
  });
  onDestroy(() => cancelAnimationFrame(rafId));

  type Feat = { id: View; icon: string; label: string; desc: string; g: string };
  // 渐变分栏：三大功能分区 —— 分区标题渐变线条 + 每项独立主题渐变（左缘线条 + 图标底），
  // 一眼分区、一键直达（与侧边导航一致）
  const zones: { label: string; g: string; feats: Feat[] }[] = [
    {
      label: "安全检测与预警", g: "linear-gradient(90deg,#e11d48,#f97316)",
      feats: [
        { id: "alerts", icon: "⚠", label: "威胁预警", desc: "Sysmon 规则 · 实时气泡告警", g: "linear-gradient(135deg,#e11d48,#fb7185)" },
        { id: "network", icon: "🌐", label: "网络监控", desc: "外联 IP · 情报查询", g: "linear-gradient(135deg,#2563eb,#60a5fa)" },
        { id: "events", icon: "📜", label: "事件日志", desc: "登录 · 账户 · 反取证", g: "linear-gradient(135deg,#7c3aed,#a78bfa)" },
      ],
    },
    {
      label: "文件与数据", g: "linear-gradient(90deg,#0ea5e9,#06b6d4)",
      feats: [
        { id: "unlock", icon: "🔓", label: "文件解锁", desc: "占用检测 · 强制删除", g: "linear-gradient(135deg,#d97706,#fbbf24)" },
        { id: "extract", icon: "⌗", label: "数据提取", desc: "IP · URL · 域名", g: "linear-gradient(135deg,#0891b2,#22d3ee)" },
        { id: "codec", icon: "⇆", label: "编解码", desc: "Base64 / URL", g: "linear-gradient(135deg,#4f46e5,#818cf8)" },
      ],
    },
    {
      label: "系统管理", g: "linear-gradient(90deg,#059669,#14b8a6)",
      feats: [
        { id: "processes", icon: "◈", label: "进程", desc: "签名甄别 · 进程树", g: "linear-gradient(135deg,#0d9488,#2dd4bf)" },
        { id: "services", icon: "⚙", label: "服务", desc: "启停禁用 · 非微软过滤", g: "linear-gradient(135deg,#475569,#94a3b8)" },
        { id: "startup", icon: "⏻", label: "启动项", desc: "Run 键 · 启动文件夹", g: "linear-gradient(135deg,#db2777,#f472b6)" },
        { id: "tasks", icon: "⏱", label: "计划任务", desc: "非系统任务管理", g: "linear-gradient(135deg,#ca8a04,#eab308)" },
        { id: "optimize", icon: "⚡", label: "系统优化", desc: "桌面 · 快捷键 · 右键", g: "linear-gradient(135deg,#059669,#34d399)" },
      ],
    },
  ];
</script>

<div class="view home">
  <div class="card hero">
    <svg class="mark" bind:this={markEl} viewBox="0 0 96 96" width="52" height="52" aria-hidden="true">
      <defs>
        <linearGradient id="homemark" x1="0" y1="0" x2="1" y2="0">
          <stop offset="0" stop-color="#17c98d"/>
          <stop offset="1" stop-color="#0a7a5c"/>
        </linearGradient>
      </defs>
      <path d="M8 44 Q5 48 8 52 L66 86 Q76 92 76 81 L76 15 Q76 4 66 10 Z"
        fill="url(#homemark)" stroke="#0f7a5b" stroke-width="2.5" stroke-linejoin="round"/>
      <path d="M12 45 L62 13" stroke="#34d399" stroke-width="2" stroke-linecap="round" opacity="0.9"/>
      <circle cx="34" cy="48" r="7" fill="#d9fff1"/>
      <circle cx="68" cy="26" r="2.6" fill="#0a4a37"/>
      <circle cx="71" cy="48" r="2.6" fill="#f5c518"/>
      <circle cx="68" cy="70" r="2.6" fill="#0a4a37"/>
    </svg>
    <h1>青刃 <span class="latin">TexMantis</span></h1>
    <div class="sub">系统精密工具箱 · Windows x64 绿色免安装</div>

    <p class="intro">
      青刃是一把藏在系统深处的精密小刀：进程、服务、启动项、计划任务一览无余，数字签名甄别可疑程序；
      文件解锁配合备份语义强删，专治最顽固的占用；网络监控直连内核 API，外联 IP 离线归属地、
      右键一键微步在线 / VirusTotal 威胁情报查询；数据提取从日志与流量中批量捞取 IP、URL 与域名；
      系统优化让桌面与资源管理器更顺手。它无服务、无驱动、无常驻进程，关闭即退、绝不驻留；
      启动即自动启用管理特权组，直取用户态最高权限——青竹之雅，利刃之锋。
    </p>

    <div class="zones">
      {#each zones as z (z.label)}
        <section class="zone" style="--zg:{z.g}">
          <div class="zhead"><span class="zlabel">{z.label}</span></div>
          <div class="zgrid">
            {#each z.feats as e (e.id)}
              <button type="button" class="tile" style="--g:{e.g}" on:click={() => view.set(e.id)}>
                <span class="ticon">{e.icon}</span>
                <span class="ttext">
                  <span class="tlabel">{e.label}</span>
                  <span class="tdesc">{e.desc}</span>
                </span>
              </button>
            {/each}
          </div>
        </section>
      {/each}
    </div>

    <div class="tip">提示：标题栏 ⚙ 可开启开机启动、托盘驻留与贴边浮漂（右缘磁吸拉片）。</div>
    <div class="sig"></div>
  </div>
</div>

<style>
  /* 居中用 margin:auto 而非 justify-content:center：
     后者在内容高于视口时会把顶部（UFO 图标）裁出滚动区且不可达——非最大化窗口"飞碟消失"的根因；
     auto 边距在溢出时自动归零，内容从顶部开始、正常可滚 */
  .home { display: flex; }
  .hero {
    margin: auto;
    max-width: 820px; padding: 30px 38px 24px; text-align: center;
    display: flex; flex-direction: column; align-items: center; gap: 9px;
  }
  .mark {
    filter: drop-shadow(0 2px 6px rgba(16, 185, 129, 0.25));
    will-change: transform;
  }
  h1 { font-size: 25px; font-weight: 700; letter-spacing: 1px; color: var(--text); }
  h1 .latin { font-size: 16px; font-weight: 600; color: var(--text-dim); letter-spacing: 0.5px; }
  .sub { font-size: 12px; color: var(--text-dim); letter-spacing: 2px; }
  .intro {
    font-size: 13px; line-height: 1.95; color: var(--text);
    text-align: justify; margin: 6px 2px 2px;
  }
  .zones { width: 100%; display: flex; flex-direction: column; gap: 16px; margin-top: 12px; }
  .zone { display: flex; flex-direction: column; gap: 8px; }
  .zhead { display: flex; }
  .zlabel { font-size: 11.5px; font-weight: 700; letter-spacing: 3px; color: var(--text-dim); }
  .zgrid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 9px; }
  .tile {
    position: relative; overflow: hidden; display: flex; align-items: center; gap: 10px;
    text-align: left; padding: 11px 12px 11px 15px; border-radius: 12px; cursor: pointer;
    border: 1px solid rgba(15, 23, 42, 0.08); background: rgba(255, 255, 255, 0.55);
    transition: background 0.15s, border-color 0.15s, transform 0.12s, box-shadow 0.15s;
  }
  .tile::before {
    content: ""; position: absolute; left: 0; top: 0; bottom: 0; width: 3px;
    background: var(--g); opacity: 0.85;
  }
  .tile:hover {
    background: rgba(255, 255, 255, 0.88); border-color: rgba(15, 23, 42, 0.16);
    transform: translateY(-2px); box-shadow: 0 8px 20px rgba(15, 23, 42, 0.09);
  }
  .tile:hover::before { opacity: 1; }
  .ticon {
    width: 30px; height: 30px; border-radius: 9px; flex: none;
    display: flex; align-items: center; justify-content: center;
    font-size: 15px; color: #fff; background: var(--g);
    box-shadow: 0 2px 8px rgba(15, 23, 42, 0.14);
  }
  .ttext { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .tlabel { font-size: 12.5px; font-weight: 650; color: var(--text); white-space: nowrap; }
  .tdesc { font-size: 10.5px; color: var(--text-dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .tip { font-size: 11.5px; color: var(--text-dim); margin-top: 8px; }
  .sig {
    font-size: 11px; color: var(--text-dim); letter-spacing: 1px; margin-top: 5px;
    border-top: 1px dashed rgba(148, 163, 184, 0.4); padding-top: 8px; width: 42%;
  }
</style>
