<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import {
    socialStatus, socialScan, socialSetProfile, socialSetFavs, socialOpenSession, socialSendText,
    socialHistory, socialGenName, socialSetLayout, socialFixFirewall,
    socialSendFile, socialFileAccept, socialFileReject, socialFilePause,
    socialFileResume, socialFileCancel, socialFileRetry,
    socialBroadcast, socialSetRemark, socialFileReceiveAgain,
    socialDirs, socialSetRecvDir, socialPickDir, socialOpenPath, socialRevealPath, socialRemovePeer, socialSetZombie, socialSetFontSize, socialClearHistory,
    socialDataUsage, socialClearAllHistory, socialClearRecvFiles, socialClearChatFiles, socialShake, socialMarkRead, socialReadAll,
    type DataUsage,
    socialCreateGroup, socialGroupSend, socialGroupInvite, socialGroupKick, socialGroupSetAdmin,
    socialGroupRename, socialGroupLeave, socialGroupDissolve, socialGroupNudge, socialGroupFiles,
  } from "./api";
  import type {
    SocialStatus, SocialPeer, SocialMsgEvt, SocialHistItem, GroupMsgEvt, FileOfferEvt,
    GroupInfo, GroupFile, XferOut, SocialDirs,
  } from "./api";
  import { fade } from "svelte/transition";
  import FilePicker from "./FilePicker.svelte";
  import { toast, askConfirm, dropQueue, pendingSocialOpen, focusMode, chatFontSize} from "./stores";

  let st: SocialStatus | null = null;
  let scanning = false;
  let chatLeft = true; // 两栏布局偏好（配置记忆，⇄ 一键调换）

  // v0.9.8 联系人列表宽度：左缘（调换后为右缘）拖拽调整，240-460px，持久化
  let contactsW = 300;
  {
    const v = Number(localStorage.getItem("tz-contacts-w"));
    if (v >= 240 && v <= 460) contactsW = v;
  }
  let contactsEl: HTMLElement | null = null;
  function plistGripDown(e: MouseEvent) {
    if (e.button !== 0 || !contactsEl) return;
    e.preventDefault();
    const wrap = contactsEl.parentElement as HTMLElement | null;
    if (!wrap) return;
    const wr = wrap.getBoundingClientRect();
    const flipped = !chatLeft;
    const outerEdge = flipped ? wr.left : wr.right; // 联系人栏外侧（贴窗）边
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    const move = (ev: MouseEvent) => {
      contactsW = Math.max(240, Math.min(460, Math.round(flipped ? ev.clientX - outerEdge : outerEdge - ev.clientX)));
    };
    const up = () => {
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      try { localStorage.setItem("tz-contacts-w", String(contactsW)); } catch {}
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }

  // v0.9.8 字号预览浮窗：拖动/按键调节时浮现，松开 1 秒后淡出（不遮挡其他功能）
  let fontPv = false;
  let fontPvTimer: ReturnType<typeof setTimeout> | null = null;
  function fontPvShow() {
    if (fontPvTimer) { clearTimeout(fontPvTimer); fontPvTimer = null; }
    fontPv = true;
  }
  function fontPvFade() {
    if (fontPvTimer) clearTimeout(fontPvTimer);
    fontPvTimer = setTimeout(() => (fontPv = false), 1000);
  }

  // 我的档案（右下角收纳 + 自动保存）
  let showMe = false;
  let nickDraft = "";
  let genderDraft = "x";
  let avatarDraft = 0;
  let saveTimer: ReturnType<typeof setTimeout> | null = null;

  // 两栏 IM：左聊天（选中会话）右联系人
  type ChatTarget = { kind: "peer"; peer: SocialPeer } | { kind: "group"; group: GroupInfo } | null;
  let target: ChatTarget = null;
  let chatKey = ""; // peer.uid 或 "group_"+gid（历史记录键）
  let chatTitle = "";
  let chatFp = "";
  let sessState: "connecting" | "ok" | "fail" = "connecting";
  let sessErr = "";
  let msgs: SocialHistItem[] = [];
  let draft = "";
  let showEmoji = false;
  let emojiTab = "最近";
  let emojiQ = "";
  let msgsEl: HTMLElement | null = null;
  let previews: Record<string, { text: string; ts: number }> = {};
  let unread: Record<string, number> = {};
  let groupUnread: Record<string, number> = {};
  let unlistenMsg: (() => void) | null = null;
  let unlistenGroupMsg: (() => void) | null = null;
  let unlistenOffer: (() => void) | null = null;
  let unlistenXfer: (() => void) | null = null;
  let unlistenRec: (() => void) | null = null;
  let unlistenShake: (() => void) | null = null;
  let unlistenOfferGone: (() => void) | null = null;
  let unlistenNote: (() => void) | null = null;
  let unlistenRead: (() => void) | null = null;
  let unlistenSync: (() => void) | null = null;
  let unlistenGone: (() => void) | null = null;
  let unlistenNudge: (() => void) | null = null;

  // 文件传输实时状态（事件驱动，轮询兜底）
  let xfers: Record<string, XferOut> = {};
  // 待处理入站文件邀请
  type Offer = FileOfferEvt & { ts: number };
  let offers: Offer[] = [];

  // ============ Telegram 级表情（Unicode 全量精选，分类 + 最近使用 + 搜索） ============
  const EMOJI_CATS: Record<string, string[]> = {
    "微笑": ["😀","😃","😄","😁","😆","😅","🤣","😂","🙂","🙃","😉","😊","😇","🥰","😍","🤩","😘","😗","😚","😙","🥲","😋","😛","😜","🤪","😝","🤑","🤗","🤭","🫢","🫣","🤫","🤔","🫡","🤐","🤨","😐","😑","😶","🫥","😏","😒","🙄","😬","🤥","😌","😔","😪","🤤","😴","😷","🤒","🤕","🤢","🤮","🤧","🥵","🥶","🥴","😵","🤯","🤠","🥳","🥸","😎","🤓","🧐","😕","🫤","😟","🙁","😮","😯","😲","😳","🥺","🥹","😦","😧","😨","😰","😥","😢","😭","😱","😖","😣","😞","😓","😩","😫","🥱","😤","😡","😠","🤬","😈","👿","💀","☠️","💩","🤡","👹","👺","👻","👽","🤖"],
    "手势": ["👋","🤚","🖐️","✋","🖖","🫱","🫲","🫳","🫴","👌","🤌","🤏","✌️","🤞","🫰","🤟","🤘","🤙","👈","👉","👆","🖕","👇","☝️","🫵","👍","👎","✊","👊","🤛","🤜","👏","🙌","🫶","👐","🤲","🤝","🙏","✍️","💅","🤳","💪","🦾","🦿","🦵","🦶","👂","🦻","👃","🧠","🫀","🫁","🦷","🦴","👀","👁️","👄","🫦","👅"],
    "动物": ["🐶","🐱","🐭","🐹","🐰","🦊","🐻","🐼","🐻‍❄️","🐨","🐯","🦁","🐮","🐷","🐸","🐵","🙈","🙉","🙊","🐒","🐔","🐧","🐦","🐤","🐣","🐥","🦆","🦅","🦉","🦇","🐺","🐗","🐴","🦄","🐝","🪱","🐛","🦋","🐌","🐞","🐜","🪰","Spider","🕷️","🦂","🐢","🐍","🦎","🦖","🦕","🐙","🦑","🦐","🦞","🦀","🐡","🐠","🐟","🐬","🐳","🐋","🦈","🐊","🐅","🐆","🦓","🦍","🦧","🐘","🦛","🦏","🐪","🐫","🦒","🦘","🐃","🐂","🐄","🐎","🐖","🐏","🐑","🦙","🐐","🦌","🐕","🐩","🦮","🐈","🐓","🦃","🦚","🦜","🦢","🦩","🕊️","🐇","🦝","🦨","🦡","🦦","🦥","🐁","🐀","🐿️","🦔"],
    "食物": ["🍏","🍎","🍐","🍊","🍋","🍌","🍉","🍇","🍓","🫐","🍈","🍒","🍑","🥭","🍍","🥥","🥝","🍅","🍆","🥑","🥦","🥬","🥒","🌶️","🫑","🌽","🥕","🫒","🧄","🧅","🥔","🍠","🥐","🥯","🍞","🥖","🥨","🧀","🥚","🍳","🧈","🥞","🧇","🥓","🥩","🍗","🍖","🦴","🌭","🍔","🍟","🍕","🫓","🥪","🥙","🧆","🌮","🌯","🫔","🥗","🥘","🫕","🥫","🍝","🍜","🍲","🍛","🍣","🍱","🥟","🦪","🍤","🍙","🍚","🍘","🍥","🥠","🥮","🍢","🍡","🍧","🍨","🍦","🥧","🧁","🍰","🎂","🍮","🍭","🍬","🍫","🍿","🍩","🍪","🌰","🥜","🍯","🥛","🍼","☕","🫖","🍵","🧃","🥤","🧋","🍶","🍺","🥂","🍷","🥃","🍸","🍹","🧉","🍾"],
    "活动": ["⚽️","🏀","🏈","⚾️","🥎","🎾","🏐","🏉","🥏","🎱","🪀","🏓","🏸","🏒","🥍","🏏","🪃","🥅","⛳️","🪁","🏹","🎣","🤿","🥊","🥋","🎽","🛹","🛼","🛷","⛸️","🥌","🎿","⛷️","🏂","🪂","🏋️","🤼","🤸","⛹️","🤺","🤾","🏌️","🏇","🧘","🏄","🏊","🤽","🚣","🧗","🚵","🚴","🏆","🥇","🥈","🥉","🏅","🎖️","🏵️","🎗️","🎫","🎟️","🎪","🤹","🎭","🩰","🎨","🎬","🎤","🎧","🎼","🎹","🥁","🎷","🎺","🎸","🪕","🎻","🎲","♟️","🎯","🎳","🎮","🎰","🧩"],
    "旅行": ["🚗","🚕","🚙","🚌","🚎","🏎️","🚓","🚑","🚒","🚐","🛻","🚚","🚛","🚜","🦯","🦽","🦼","🛴","🚲","🛵","🏍️","🛺","🚨","🚔","🚍","🚘","🚖","🚡","🚠","🚟","🚃","🚋","🚞","🚝","🚄","🚅","🚈","🚂","🚆","🚇","🚊","🚉","✈️","🛫","🛬","🛩️","💺","🛰️","🚀","🛸","🚁","🛶","⛵️","🚤","🛥️","🛳️","⛴️","🚢","⚓️","🪝","⛽️","🚧","🚦","🚥","🚏","🗺️","🗿","🗽","🗼","🏰","🏯","🏟️","🎡","🎢","🎠","⛲️","⛱️","🏖️","🏝️","🏜️","🌋","⛰️","🏔️","🗻","🏕️","⛺️","🏠","🏡","🏘️","🏚️","🏗️","🏭","🏢","🏬","🏣","🏤","🏥","🏦","🏨","🏪","🏫","💒","🏛️","⛪️","🕌","🛕","🕍","🕋","⛩️"],
    "物品": ["⌚️","📱","📲","💻","⌨️","🖥️","🖨️","🖱️","🖲️","🕹️","🗜️","💽","💾","💿","📀","📼","📷","📸","📹","🎥","📽️","🎞️","📞","☎️","📟","📠","📺","📻","🎙️","🎚️","🎛️","🧭","⏱️","⏲️","⏰","🕰️","⌛️","⏳","📡","🔋","🪫","🔌","💡","🔦","🕯️","🪔","🧯","🛢️","💸","💵","💴","💶","💷","🪙","💰","💳","💎","⚖️","🪜","🧰","🪛","🔧","🔨","⚒️","🛠️","⛏️","🪚","🔩","⚙️","🪤","🧱","⛓️","🧲","🔫","💣","🧨","🪓","🔪","🗡️","⚔️","🛡️","🚬","⚰️","🪦","⚱️","🏺","🔮","📿","🧿","💈","⚗️","🔭","🔬","🕳️","🩹","🩺","💊","💉","🩸","🧬","🦠","🧫","🧪","🌡️","🧹","🪠","🧺","🧻","🚽","🚿","🛁"],
    "符号": ["❤️","🧡","💛","💚","💙","💜","🖤","🤍","🤎","💔","❣️","💕","💞","💓","💗","💖","💘","💝","💟","♥️","💯","💢","💥","💫","💦","💨","🕳️","💬","👁️‍🗨️","🗨️","🗯️","💭","💤","✅","☑️","✔️","❌","❎","➕","➖","➗","✖️","♾️","❓","❔","❕","❗️","‼️","⁉️","🔴","🟠","🟡","🟢","🔵","🟣","⚫️","⚪️","🟤","🔺","🔻","🔶","🔷","🔸","🔹","⭕️","✳️","✴️","❇️","©️","®️","™️","#⃣","*⃣","0️⃣","1️⃣","2️⃣","3️⃣","4️⃣","5️⃣","6️⃣","7️⃣","8️⃣","9️⃣","🔟","🎉","🎊","🎈","🎁","🎀","🪄","🪅","🎊"],
  };
  const EMOJI_TABS = ["最近", ...Object.keys(EMOJI_CATS)];
  const KEYWORD_HINT: Record<string, string[]> = {
    "笑": ["😀","😄","😁","🤣","😂","😊"], "哭": ["😢","😭","🥺","😿"],
    "爱": ["❤️","💕","😘","🥰","😍","💞"], "赞": ["👍","👏","🫶","💯","🙏"],
    "花": ["🌸","🌺","🌻","🌷","💐","🌹"], "生日": ["🎂","🎉","🎁","🎈","🕯️"],
    "好": ["👌","✅","👍","🙂"], "餐": ["🍚","🍜","🍲","🥢"],
  };
  const RECENT_KEY = "tz-social-emoji-recent";
  let recentEmoji: string[] = [];
  function loadRecent() {
    try { recentEmoji = JSON.parse(localStorage.getItem(RECENT_KEY) || "[]"); } catch { recentEmoji = []; }
  }
  function pushRecent(e: string) {
    recentEmoji = [e, ...recentEmoji.filter((x) => x !== e)].slice(0, 24);
    try { localStorage.setItem(RECENT_KEY, JSON.stringify(recentEmoji)); } catch {}
  }
  // 显式传参：Svelte 4 只跟踪响应式语句表达式里出现的标识符，
  // 直接在模板调 emojiList() 会导致内部分类/搜索/最近使用状态变化不重渲染
  function emojiList(tab: string, q: string, recent: string[]): string[] {
    const dedupe = (arr: string[]) => {
      const seen = new Set<string>();
      return arr.filter((e) => !seen.has(e) && seen.add(e));
    };
    if (q.trim()) {
      const needle = q.trim().toLowerCase();
      const all = Object.values(EMOJI_CATS).flat();
      const hit = dedupe(all.filter((e) => e.includes(needle)));
      // 搜索无结果时给出关键词联想（中文关键词映射，覆盖常用场景）
      if (!hit.length) return dedupe(KEYWORD_HINT[needle] ?? []);
      return hit;
    }
    if (tab === "最近") return dedupe(recent.length ? recent : EMOJI_CATS["微笑"].slice(0, 24));
    return dedupe(EMOJI_CATS[tab] ?? []);
  }
  $: emojiShown = emojiList(emojiTab, emojiQ, recentEmoji);

  const AV_EMOJI = [
    "🐯", "🦁", "🐺", "🦊", "🐵", "🤴", "🕶️", "🤖",
    "🐰", "🦌", "🐱", "🦋", "👸", "🌸", "🍓", "🎀",
    "🐧", "🐸", "🌈", "🛸"
  ];
  const AV_BG = [
    "#f59e0b", "#ef4444", "#6366f1", "#d97706", "#16a34a", "#0ea5e9", "#475569", "#8b5cf6",
    "#ec4899", "#a16207", "#f97316", "#db2777", "#7c3aed", "#e11d48", "#059669", "#be185d",
    "#0284c7", "#65a30d", "#9333ea", "#0891b2"
  ];

  // 已结束（完成/取消/失败）且提示过卡片再移除的传输：轮询快照不再复活它们
  // v0.9.11：失败(st=4)纳入终态（此前失败卡永久残留）；失败给 6 秒可读期，完成/取消 2.6 秒
  const isTerm = (st: number) => st === 2 || st === 3 || st === 4;
  const hiddenXfers = new Set<string>();
  const xferTimers = new Map<string, ReturnType<typeof setTimeout>>();
  function scheduleXferRemoval(id: string, ms = 2600) {
    if (xferTimers.has(id)) return;
    xferTimers.set(id, setTimeout(() => {
      xferTimers.delete(id);
      hiddenXfers.add(id);
      const { [id]: _drop, ...rest } = xfers;
      xfers = rest;
    }, ms));
  }
  let booted = false; // v0.9.16：首次同步时快照里的历史终态不渲染（切换界面返回不再闪现）
  function syncFrom(s: SocialStatus) {
    st = s;
    chatLeft = s.chat_left;
    // v0.9.9 会话状态自愈：对方来信或任一次成功发送都会建立加密会话，
    // 快照里的活跃会话表就是真相 —— 顶部状态随之纠正（连接尝试进行中除外）
    if (target?.kind === "peer" && sessState !== "connecting") {
      sessState = (s.sessions ?? []).includes(target.peer.uid) ? "ok" : "fail";
    }
    nickDraft = s.nickname;
    genderDraft = s.gender;
    avatarDraft = s.avatar;
    // 传输状态合流：轮询快照为底、事件为增量；终态短暂展示后移除（v0.9.2 修复"完成不消失"）
    // 首次同步：历史终态（完成/取消/失败）直接进隐藏集，不闪卡
    if (!booted) {
      for (const x of s.xfers ?? []) {
        if (isTerm(x.st)) hiddenXfers.add(x.id);
      }
      booted = true;
    }
    for (const x of s.xfers ?? []) {
      if (isTerm(x.st) && hiddenXfers.has(x.id)) continue;
      const cur = xfers[x.id];
      if (!cur || cur.done !== x.done || cur.st !== x.st) {
        if (cur && cur.st !== x.st) {
          if (x.st === 3) toast(x.dir === "out" ? `「${x.name}」已发送` : `已接收「${x.name}」`, "ok");
          else if (x.st === 4) toast(`「${x.name}」传输失败`, "err");
        }
        xfers = { ...xfers, [x.id]: x };
        if (isTerm(x.st)) scheduleXferRemoval(x.id, x.st === 4 ? 6000 : 2600);
      }
    }
    // v0.9.11：快照里已消失的入站邀请（后端已回收）→ 本地同步移除，卡片不再残留
    {
      const snap = new Set((s.xfers ?? []).map((x) => x.id));
      const stale = Object.values(xfers)
        .filter((x) => x.st === 5 && x.dir === "in" && !snap.has(x.id) && !offers.some((o) => o.id === x.id))
        .map((x) => x.id);
      if (stale.length) {
        const next = { ...xfers };
        for (const id of stale) delete next[id];
        xfers = next;
      }
    }
    // 会话仍打开时保持标题/成员最新（局部捕获避免回调内收窄失效）
    const t = target;
    if (t?.kind === "group") {
      const g = (s.groups ?? []).find((x) => x.gid === t.group.gid);
      if (g) { target = { kind: "group", group: g }; chatTitle = g.name; }
    } else if (t?.kind === "peer") {
      const p = (s.peers ?? []).find((x) => x.uid === t.peer.uid);
      if (p) { target = { kind: "peer", peer: p }; chatTitle = dispName(p); }
    }
  }

  function dispName(p: SocialPeer): string {
    return p.remark ? p.remark : p.nickname;
  }

  // 网络权限一键修复
  async function fixFw() {
    try {
      await socialFixFirewall();
      toast("已请求管理员权限：请在 UAC 弹窗点「是」；若安装了安全软件，请在其弹窗允许联网", "info");
    } catch (e) {
      toast(String(e), "err");
    }
    setTimeout(() => { void refresh(); }, 6000);
  }

  async function flipLayout() {
    chatLeft = !chatLeft;
    try { await socialSetLayout(chatLeft); } catch {}
  }

  // v0.9.2 起发现由后端保活线程接管（两级心跳：历史联系人单播探测 → 名单空才局域网探测）
  let lastAutoScan = 0;
  async function refresh() {
    try { syncFrom(await socialStatus()); } catch {}
  }

  // v0.9.4 自适应雷达：空名单 3 秒快捕获（前 10 轮）→ 10 秒（再 10 轮）→ 30 秒兜底；
  // 有联系人 30 秒轻刷新（新同伴的工具启动即宣告，2 秒内就会出现在列表里，
  // 扫描只是保险）；页面在后台/最小化时降频到 60 秒。风暴预算：
  // 每轮 2 个 ~90B 广播包 + 每位同伴 10 秒至多 1 个去重应答（单播、抖动），
  // 稳态 < 100B/s 出向流量，且只在本页打开期间发生 —— 无风暴特征
  let scanTimer: ReturnType<typeof setTimeout> | null = null;
  let scanRound = 0;
  function scheduleScan() {
    if (scanTimer) clearTimeout(scanTimer);
    const peers = (st?.peers ?? []).length;
    let delay: number;
    if (!peers) {
      scanRound++;
      delay = scanRound <= 10 ? 3000 : scanRound <= 20 ? 10000 : 30000;
    } else {
      scanRound = 0;
      delay = 30000;
    }
    if (typeof document !== "undefined" && document.hidden) delay = Math.max(delay, 60000);
    scanTimer = setTimeout(async () => {
      if (st?.running && !scanning) {
        lastAutoScan = Date.now();
        void radar();
      }
      scheduleScan();
    }, delay);
  }

  async function radar() {
    if (scanning) return;
    scanning = true;
    try {
      syncFrom(await socialScan());
    } catch (e) {
      toast(String(e), "err");
    } finally {
      scanning = false;
    }
  }

  function autoSave() {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(async () => {
      try { syncFrom(await socialSetProfile(nickDraft, genderDraft, avatarDraft)); } catch {}
    }, 500);
  }

  async function rollName() {
    try {
      nickDraft = await socialGenName(genderDraft);
      autoSave();
    } catch (e) {
      toast(String(e), "err");
    }
  }

  async function toggleFav(p: SocialPeer) {
    const favs = p.fav ? (st?.favs ?? []).filter((x) => x !== p.uid) : [...(st?.favs ?? []), p.uid];
    try {
      syncFrom(await socialSetFavs(favs));
    } catch (e) {
      toast(String(e), "err");
    }
  }

  // ============ 备注好友 ============
  let remarkFor: SocialPeer | null = null;
  let remarkDraft = "";
  function openRemark(p: SocialPeer) {
    remarkFor = p;
    remarkDraft = p.remark || "";
  }
  async function saveRemark() {
    if (!remarkFor) return;
    const uid = remarkFor.uid;
    remarkFor = null;
    try {
      await socialSetRemark(uid, remarkDraft);
      await refresh();
      if (target?.kind === "peer" && target.peer.uid === uid) chatTitle = dispName(target.peer);
      toast("备注已保存", "ok");
    } catch (e) {
      toast(String(e), "err");
    }
  }

  // ============ 会话打开（好友 / 群统一入口） ============
  // 任何后端返回都强制收敛为数组：绝不让 undefined 进入渲染层（v0.8.1 教训）
  const asMsgs = (x: unknown): SocialHistItem[] => (Array.isArray(x) ? (x as SocialHistItem[]) : []);

  async function openChat(p: SocialPeer) {
    sideTab = "peer";
    if (target?.kind === "peer" && target.peer.uid === p.uid) return;
    target = { kind: "peer", peer: p };
    chatKey = p.uid;
    chatTitle = dispName(p);
    msgs = [];
    sessState = "connecting";
    sessErr = "";
    draft = "";
    showEmoji = false;
    unread = { ...unread, [p.uid]: 0 };
    try {
      const h = asMsgs(await socialHistory(p.uid));
      if (target?.kind === "peer" && target.peer.uid === p.uid) {
        msgs = h;
        queueMarkRead(); // v0.9.21 打开聊天即已读
        const last = h[h.length - 1];
        if (last) previews = { ...previews, [p.uid]: { text: last.file ? `📄 ${last.file}` : last.text, ts: last.ts } };
      }
    } catch {}
    setTimeout(scrollBottom, 60);
    try {
      const r = await socialOpenSession(p.uid);
      if (target?.kind === "peer" && target.peer.uid === p.uid) {
        chatFp = r.fp;
        sessState = "ok";
      }
    } catch (e) {
      if (target?.kind === "peer" && target.peer.uid === p.uid) {
        sessState = "fail";
        sessErr = String(e);
        toast(String(e), "err");
      }
    }
  }

  async function openGroup(g: GroupInfo) {
    sideTab = "group";
    if (target?.kind === "group" && target.group.gid === g.gid) return;
    target = { kind: "group", group: g };
    chatKey = `group_${g.gid}`;
    chatTitle = g.name;
    msgs = [];
    sessState = "ok";
    draft = "";
    showEmoji = false;
    groupUnread = { ...groupUnread, [g.gid]: 0 };
    try {
      const h = asMsgs(await socialHistory(chatKey));
      if (target?.kind === "group" && target.group.gid === g.gid) msgs = h;
    } catch {}
    setTimeout(scrollBottom, 60);
  }

  // v0.9.11 清除本会话接收的文件（记录条目 + 托管目录内磁盘文件）
  async function clearChatFiles() {
    if (!target) return;
    const ok = await askConfirm(`清除与「${chatTitle}」会话中接收的全部文件？\n\n文件记录将从聊天记录中移除，托管目录内的文件同步删除，不可恢复。`, true);
    if (!ok) return;
    try {
      const n = await socialClearChatFiles(chatKey);
      histItems = asMsgs(await socialHistory(chatKey).catch(() => []));
      toast(n > 0 ? `已清除 ${n} 个接收文件` : "没有可清除的接收文件", "ok");
    } catch (e) {
      toast(String(e), "err");
    }
  }

  // v0.9.10 一键清空当前会话的本地聊天记录（红色确认，不可恢复）
  async function clearCurrentHistory() {
    if (!target) return;
    const name = chatTitle || "对方";
    const ok = await askConfirm(`清空与「${name}」的全部聊天记录？\n\n此操作不可恢复，且只影响本机（对方记录不受影响）。`, true);
    if (!ok) return;
    try {
      await socialClearHistory(chatKey);
      msgs = [];
      histItems = [];
      const p = { ...previews };
      delete p[chatKey];
      previews = p;
      toast("聊天记录已清空", "ok");
    } catch (e) {
      toast(String(e), "err");
    }
  }

  // v0.9.9 联系人资料卡（右键）
  let peerInfo: SocialPeer | null = null;
  function infoChat() {
    if (peerInfo) openChat(peerInfo);
    peerInfo = null;
  }
  function infoRemark() {
    if (peerInfo) openRemark(peerInfo);
    peerInfo = null;
  }

  function closeChat() {
    target = null;
    chatKey = "";
    msgs = [];
    sessState = "connecting";
    sessErr = "";
  }

  function scrollBottom() {
    if (msgsEl) msgsEl.scrollTop = msgsEl.scrollHeight;
  }

  // ============ 发送（好友/群统一） ============
  let sending = false;
  async function send() {
    const text = draft.trim();
    if (!text || !target || sending) return;
    sending = true;
    try {
      if (target.kind === "peer") {
        const r = await socialSendText(target.peer.uid, text);
        sessState = "ok"; // v0.9.9 发送成功 = 会话已建立，顶部状态立即纠正
        msgs = [...asMsgs(msgs), { d: "out", ts: r.ts, text, file: "", size: 0, path: "", from: "", from_name: "" }];
        previews = { ...previews, [target.peer.uid]: { text, ts: r.ts } };
      } else {
        const r = await socialGroupSend(target.group.gid, text);
        msgs = [...asMsgs(msgs), { d: "out", ts: r.ts, text, file: "", size: 0, path: "", from: st?.uid ?? "", from_name: st?.nickname ?? "" }];
        previews = { ...previews, [`group_${target.group.gid}`]: { text, ts: r.ts } };
      }
      draft = "";
      showEmoji = false;
      requestAnimationFrame(autoGrow);
      setTimeout(scrollBottom, 30);
    } catch (e) {
      toast(String(e), "err");
    } finally {
      sending = false;
    }
  }

  // ============ 文件发送 ============
  // v0.9.12 附件 = 应用内轻量资源管理器（即时响应，免原生对话框）
  let showPicker = false;
  function pickAndSend() {
    if (!target) return;
    showPicker = true;
  }
  async function doSendFile(path: string) {
    if (!target) return;
    try {
      if (target.kind === "peer") {
        const x = await socialSendFile(target.peer.uid, path);
        ingestXfer(x);
        toast(`已向 ${xferPeerName(x)} 发出文件邀请`, "ok");
      } else {
        // 群文件 = 对每位其他成员逐人加密发送（文件落在群目录）
        const g = target.group;
        const others = g.members.filter((m) => m.uid !== st?.uid);
        if (!others.length) { toast("群里暂无其他成员", "err"); return; }
        toast(`向 ${others.length} 位成员发送「${path.split(/[\\/]/).pop()}」…`, "info");
        for (const m of others) {
          try {
            const x = await socialSendFile(m.uid, path, g.gid);
            ingestXfer(x);
          } catch (e) {
            toast(`${m.nickname}: ${e}`, "err");
          }
        }
      }
    } catch (e) {
      toast(String(e), "err");
    }
  }

  // ============ 社交功能设置（右上角 ⚙；右下角头像仍是个人资料） ============
  let showSettings = false;
  let dirs: SocialDirs | null = null;
  let recvDraft = "";
  async function openSettings() {
    showSettings = true;
    showMe = false;
    showEmoji = false;

    try {
      dirs = await socialDirs();
      recvDraft = dirs.custom;
      syncFont(dirs);
      loadUsage(); // v0.9.11 数据占用
    } catch {}
  }
  async function pickRecv() {
    try {
      const d = await socialPickDir();
      if (d) recvDraft = d;
    } catch (e) {
      toast(String(e), "err");
    }
  }
  async function saveRecv() {
    try {
      dirs = await socialSetRecvDir(recvDraft.trim());
      recvDraft = dirs.custom;
      toast(recvDraft ? "接收目录已更新（对新传输生效）" : "已恢复默认：下载文件夹", "ok");
    } catch (e) {
      toast(String(e), "err");
    }
  }
  async function resetRecv() {
    recvDraft = "";
    await saveRecv();
  }
  function openChatDir() {
    if (dirs?.chat) socialOpenPath(dirs.chat).catch((e) => toast(String(e), "err"));
  }
  // v0.9.3 手动移除联系人（删除不拉黑：对方来信自动恢复）
  async function removePeer(p: SocialPeer) {
    const ok = await askConfirm(`将「${dispName(p)}」从联系人列表移除？\n\n对方发来消息时会自动恢复，不会拉黑。`, true);
    if (!ok) return;
    try {
      await socialRemovePeer(p.uid);
      if (target?.kind === "peer" && target.peer.uid === p.uid) closeChat();
      await refresh();
      toast("已移除", "ok");
    } catch (e) {
      toast(String(e), "err");
    }
  }
  // v0.9.5 聊天字号：拖动即时全聊天生效（--chat-fs 变量），松手落盘；预览小浮窗跟随滑块
  let fontDraft = 13;
  $: fontPct = Math.max(15, Math.min(85, ((fontDraft - 12) / 5) * 100));
  function fontLive() {
    chatFontSize.set(fontDraft);
  }
  async function saveFont() {
    try { await socialSetFontSize(fontDraft); } catch (e) { toast(String(e), "err"); }
  }
  // v0.9.11 数据管理
  let usage: DataUsage | null = null;
  async function loadUsage() {
    try { usage = await socialDataUsage(); } catch { usage = null; }
  }
  async function clearAllHistory() {
    const ok = await askConfirm("清除所有聊天记录？\n\n将删除本机全部会话的消息与文件记录（含群聊），不可恢复。对方不受影响。", true);
    if (!ok) return;
    try {
      await socialClearAllHistory();
      if (target) { msgs = []; const p = { ...previews }; delete p[chatKey]; previews = p; }
      await loadUsage();
      toast("已清除所有聊天记录", "ok");
    } catch (e) {
      toast(String(e), "err");
    }
  }
  async function clearRecvFiles() {
    const ok = await askConfirm(`清除全部接收的文件？\n\n将删除托管目录内 ${usage?.files_count ?? 0} 个文件（约 ${fmtSize(usage?.files_bytes ?? 0)}），不可恢复。`, true);
    if (!ok) return;
    try {
      await socialClearRecvFiles();
      await loadUsage();
      toast("已清除所有接收的文件", "ok");
    } catch (e) {
      toast(String(e), "err");
    }
  }
  function syncFont(d: SocialDirs | null) {
    if (d?.font) {
      fontDraft = d.font;
      chatFontSize.set(fontDraft);
    }
  }
  // v0.9.3 僵尸号清理开关
  async function toggleZombie() {
    if (!dirs) return;
    try {
      dirs = await socialSetZombie(!dirs.zombie);
      toast(dirs.zombie ? "已开启僵尸号自动清理：同机多身份将自动合并" : "已关闭僵尸号自动清理", "ok");
      await refresh();
    } catch (e) {
      toast(String(e), "err");
    }
  }

  // ============ Telegram 式拖拽发送（聊天卡片命中测试 + 悬浮高亮） ============
  let chatCardEl: HTMLElement | null = null;
  let dragOver = false;
  let unlistenDrop: (() => void) | null = null;
  // Tauri 给的是物理像素坐标，CSS 命中测试要用逻辑像素
  function inChatCard(px: number, py: number): boolean {
    if (!chatCardEl || !target) return false;
    const scale = window.devicePixelRatio || 1;
    const x = px / scale;
    const y = py / scale;
    const r = chatCardEl.getBoundingClientRect();
    return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom;
  }
  async function dropSend(paths: string[]) {
    dragOver = false;
    const tgt = target; // 快照：长循环期间用户收起聊天也不抛空引用
    if (!tgt || !paths.length) return;
    dropQueue.set([]); // 已由社交页接管，避免稍后被文件解锁页误接收
    let okCnt = 0;
    const first = paths[0].split(/[\\/]/).pop() ?? paths[0];
    for (const path of paths) {
      try {
        if (tgt.kind === "peer") {
          const x = await socialSendFile(tgt.peer.uid, path);
          ingestXfer(x);
          okCnt++;
        } else {
          const g = tgt.group;
          const others = g.members.filter((m) => m.uid !== st?.uid);
          for (const m of others) {
            try {
              const x = await socialSendFile(m.uid, path, g.gid);
              ingestXfer(x);
            } catch (e) {
              toast(`${m.nickname}: ${e}`, "err");
            }
          }
          okCnt++;
        }
      } catch (e) {
        toast(`${path.split(/[\\/]/).pop()}: ${e}`, "err");
      }
    }
    if (okCnt) toast(`已投送 ${okCnt} 个文件（${first}${paths.length > 1 ? ` 等 ${paths.length} 个` : ""}），等待对方接收`, "ok");
  }
  async function dropHandler(ev: any) {
    const pl = ev?.payload;
    if (!pl) return;
    if (pl.type === "enter" || pl.type === "over") {
      // 仅当聊天会话打开且落点在聊天卡片内时提示（否则沿用全局拖放行为）
      dragOver = !!target && inChatCard(pl.position?.x ?? 0, pl.position?.y ?? 0);
    } else if (pl.type === "drop") {
      const ps: string[] = pl.paths ?? [];
      if (target && ps.length && inChatCard(pl.position?.x ?? 0, pl.position?.y ?? 0)) {
        await dropSend(ps);
      }
      dragOver = false;
    } else {
      dragOver = false;
    }
  }

  // ============ 传输控制 ============
  async function xAct(id: string, act: "pause" | "resume" | "cancel" | "retry" | "receive") {
    try {
      if (act === "pause") await socialFilePause(id);
      else if (act === "resume") await socialFileResume(id);
      else if (act === "cancel") { await socialFileCancel(id); setTimeout(() => { const { [id]: _drop, ...rest } = xfers; xfers = rest; }, 400); }
      else if (act === "retry") { const x = await socialFileRetry(id); ingestXfer(x); }
      else await socialFileReceiveAgain(id);
      const cur = xfers[id];
      if (cur) xfers = { ...xfers, [id]: { ...cur, st: act === "pause" ? 1 : act === "resume" ? 0 : cur.st } };
    } catch (e) {
      toast(String(e), "err");
    }
  }

  const fmtSize = (n: number) =>
    n >= 1 << 30 ? `${(n / (1 << 30)).toFixed(2)} GB` : n >= 1 << 20 ? `${(n / (1 << 20)).toFixed(1)} MB` : n >= 1024 ? `${(n / 1024).toFixed(0)} KB` : `${n} B`;

  const xferStateText = (st: number, dir = "") =>
    st === 0 ? "传输中" : st === 1 ? "已暂停" : st === 2 ? "已取消" : st === 3 ? "已完成" : st === 4 ? "失败" : dir === "out" ? "等待对方接收" : "待回应";

  // ============ 广播 ============
  let showBcast = false;
  let bcastText = "";
  let bcastSel: string[] = [];
  let bcastBusy = false;
  function openBcast() {
    bcastText = "";
    bcastSel = (st?.peers ?? []).filter((p) => p.online).map((p) => p.uid);
    showBcast = true;
  }
  function toggleBcastSel(uid: string) {
    bcastSel = bcastSel.includes(uid) ? bcastSel.filter((x) => x !== uid) : [...bcastSel, uid];
  }
  async function doBcast() {
    if (bcastBusy) return;
    const text = bcastText.trim();
    if (!text) { toast("广播内容不能为空", "err"); return; }
    if (!bcastSel.length) { toast("请至少选择一位联系人", "err"); return; }
    bcastBusy = true;
    try {
      const r = await socialBroadcast(bcastSel, text);
      const ok = r.filter((x) => x.ok).length;
      const fail = r.filter((x) => !x.ok);
      toast(`广播完成：成功 ${ok}/${r.length}${fail.length ? `（失败：${fail.map((f) => st?.peers.find((p) => p.uid === f.uid)?.remark || st?.peers.find((p) => p.uid === f.uid)?.nickname || f.uid.slice(0, 8)).join("、")}）` : ""}`, fail.length ? "err" : "ok");
      showBcast = false;
    } catch (e) {
      toast(String(e), "err");
    } finally {
      bcastBusy = false;
    }
  }

  // ============ 群聊 ============
  let showCreateGroup = false;
  let gNameDraft = "";
  let gSel: string[] = [];
  function openCreateGroup() {
    gNameDraft = "";
    gSel = [];
    showCreateGroup = true;
  }
  function toggleGSel(uid: string) {
    gSel = gSel.includes(uid) ? gSel.filter((x) => x !== uid) : [...gSel, uid];
  }
  async function doCreateGroup() {
    try {
      const g = await socialCreateGroup(gNameDraft, gSel);
      showCreateGroup = false;
      await refresh();
      await openGroup(g);
      toast(`群「${g.name}」已创建，邀请已发送`, "ok");
    } catch (e) {
      toast(String(e), "err");
    }
  }

  let showGroupMgr = false;
  let gMgrRename = "";
  let gMgrInviteSel: string[] = [];
  function openGroupMgr() {
    if (target?.kind !== "group") return;
    gMgrRename = target.group.name;
    gMgrInviteSel = [];
    showGroupMgr = true;
  }
  function toggleInviteSel(uid: string) {
    gMgrInviteSel = gMgrInviteSel.includes(uid) ? gMgrInviteSel.filter((x) => x !== uid) : [...gMgrInviteSel, uid];
  }
  const amOwner = (g: GroupInfo) => st && g.owner === st.uid;
  const amAdmin = (g: GroupInfo) => st && (g.owner === st.uid || (g.admins ?? []).includes(st.uid));

  async function gAction(act: "rename" | "invite" | "kick" | "admin" | "leave" | "dissolve" | "nudge", arg?: string | boolean) {
    if (target?.kind !== "group") return;
    const gid = target.group.gid;
    try {
      if (act === "rename") {
        const g = await socialGroupRename(gid, gMgrRename);
        syncGroup(g);
        toast("群名已更新", "ok");
      } else if (act === "invite") {
        const g = await socialGroupInvite(gid, gMgrInviteSel);
        syncGroup(g);
        toast("邀请已发出", "ok");
      } else if (act === "kick" && typeof arg === "string") {
        if (!(await confirmSec(`确定将「${memberName(target.group, arg)}」移出群聊？`))) return;
        const g = await socialGroupKick(gid, arg);
        syncGroup(g);
        toast("已移出", "ok");
      } else if (act === "leave") {
        if (!(await confirmSec("确定退出该群聊？"))) return;
        await socialGroupLeave(gid);
        showGroupMgr = false;
        closeChat();
        await refresh();
        toast("已退出群聊", "ok");
      } else if (act === "dissolve") {
        if (!(await confirmSec("确定解散该群？所有成员的群将被移除。", true))) return;
        await socialGroupDissolve(gid);
        showGroupMgr = false;
        closeChat();
        await refresh();
        toast("群已解散", "ok");
      } else if (act === "nudge") {
        await socialGroupNudge(gid);
        toast("已向全体成员发送提醒", "ok");
      }
    } catch (e) {
      toast(String(e), "err");
    }
  }
  async function gSetAdmin(g: GroupInfo, uid: string, admin: boolean) {
    try {
      const ng = await socialGroupSetAdmin(g.gid, uid, admin);
      syncGroup(ng);
      toast(admin ? "已任命管理员" : "已撤销管理员", "ok");
    } catch (e) {
      toast(String(e), "err");
    }
  }
  function syncGroup(g: GroupInfo) {
    if (target?.kind === "group" && target.group.gid === g.gid) {
      target = { kind: "group", group: g };
      chatTitle = g.name;
    }
    refresh();
  }
  function memberName(g: GroupInfo, uid: string): string {
    return g.members.find((m) => m.uid === uid)?.nickname ?? uid.slice(0, 8);
  }

  // 简易确认框（复用全局 confirmBox）
  async function confirmSec(text: string, danger = false): Promise<boolean> {
    return askConfirm(text, danger);
  }

  // ============ 聊天记录查看器（消息/文件 + 搜索） ============
  let showHist = false;
  let histTab: "msg" | "file" = "msg";
  let histQ = "";
  let histItems: SocialHistItem[] = [];
  let groupFiles: GroupFile[] = [];
  async function openHist() {
    if (!target) return;
    showHist = true;
    histTab = "msg";
    histQ = "";
    histItems = asMsgs(await socialHistory(chatKey).catch(() => []));
    if (target.kind === "group") {
      groupFiles = await socialGroupFiles(target.group.gid).catch(() => []);
    }
    setTimeout(() => histMsgsEl?.scrollTo(0, histMsgsEl.scrollHeight), 50);
  }
  let histMsgsEl: HTMLElement | null = null;
  $: histFiltered = histItems.filter((m) => {
    if (histTab === "file") return !!m.file;
    return !m.file || m.file === "";
  });
  $: histSearched = histQ.trim()
    ? histFiltered.filter((m) =>
        (m.text || "").includes(histQ.trim()) || (m.file || "").includes(histQ.trim()) || (m.from_name || "").includes(histQ.trim()))
    : histFiltered;
  $: fileRows = target?.kind === "group"
    ? [
        ...groupFiles.map((f) => ({ name: f.name, size: f.size, ts: f.ts, path: "", d: "in" })),
      ].filter((f) => !histQ.trim() || f.name.includes(histQ.trim()))
    : histItems.filter((m) => m.file).map((m) => ({ name: m.file, size: m.size, ts: m.ts, path: m.path, d: m.d }))
        .filter((f) => !histQ.trim() || f.name.includes(histQ.trim()));

  // ============ 联系人排序：最近有消息的在前 ============
  $: sortedPeers = st ? [...st.peers].sort((a, b) => (previews[b.uid]?.ts ?? 0) - (previews[a.uid]?.ts ?? 0)) : [];
  $: favCount = st?.peers.filter((p) => p.fav).length ?? 0;
  $: groupsList = st?.groups ?? [];
  // 侧栏分页签：好友 / 群聊
  let sideTab: "peer" | "group" = "peer";

  function previewText(p: SocialPeer) {
    return previews[p.uid]?.text ?? "";
  }
  function groupPreview(g: GroupInfo) {
    return previews[`group_${g.gid}`]?.text ?? "";
  }
  function previewTime(p: SocialPeer) {
    const pv = previews[p.uid];
    if (!pv) return "";
    return fmtTime(pv.ts);
  }
  function fmtTime(ms: number) {
    const d = new Date(ms);
    const now = new Date();
    const sameDay = d.toDateString() === now.toDateString();
    return sameDay
      ? d.toLocaleTimeString("zh-CN", { hour12: false, hour: "2-digit", minute: "2-digit" })
      : `${d.getMonth() + 1}/${d.getDate()}`;
  }
  const fmtT = (ms: number) => new Date(ms).toLocaleTimeString("zh-CN", { hour12: false });
  const genderText = (g: string) => (g === "m" ? "男" : g === "f" ? "女" : "中性");
  const fwText = (f: string) =>
    f === "ok" ? "防火墙已放行" : f === "guest" ? "等待防火墙允许（弹窗点「允许」）" : f === "fail" ? "防火墙规则添加失败" : "";
  function msgSender(m: SocialHistItem): string {
    if (target?.kind !== "group") return "";
    if (m.d === "out") return "";
    return m.from_name || "成员";
  }
  function memberAvatar(g: GroupInfo, uid: string): number {
    return g.members.find((m) => m.uid === uid)?.avatar ?? 0;
  }

  let timer: ReturnType<typeof setInterval> | null = null;
  let unsubOpen: (() => void) | null = null;
  onMount(async () => {
    loadRecent();
    await refresh();
    if (st?.running) { lastAutoScan = Date.now(); void radar(); }
    scheduleScan(); // v0.9.4 自适应雷达循环
    socialDirs().then((d) => { dirs = d; syncFont(d); }).catch(() => {}); // v0.9.5 字号预载
    // v0.9.2 气泡点击直达：社交板块打开对应会话（等状态就绪，最多重试 4 秒）
    unsubOpen = pendingSocialOpen.subscribe((v) => {
      if (!v) return;
      pendingSocialOpen.set(null);
      let tries = 0;
      const tryOpen = () => {
        tries++;
        const [kind, id] = v.split(":");
        if (kind === "group") {
          const g = (st?.groups ?? []).find((x) => x.gid === id);
          if (g) { void openGroup(g); return; }
        } else {
          const p = (st?.peers ?? []).find((x) => x.uid === id);
          if (p) { void openChat(p); return; }
        }
        if (tries < 8) setTimeout(tryOpen, 500);
      };
      tryOpen();
    });
    if (st && !st.named) {
      try {
        const n = await socialGenName(st.gender);
        syncFrom(await socialSetProfile(n, st.gender, st.avatar));
        toast(`已为你取了社交昵称「${n}」，右下角头像可随时更换`, "ok");
      } catch {}
    }
    timer = setInterval(refresh, 20000);
    try {
      unlistenDrop = await getCurrentWebview().onDragDropEvent(dropHandler);
    } catch {}
    try {
      unlistenMsg = await listen<SocialMsgEvt>("social-msg", (ev) => {
        const m = ev.payload;
        previews = { ...previews, [m.uid]: { text: m.text, ts: m.ts } };
        if (target?.kind === "peer" && target.peer.uid === m.uid) {
          msgs = [...asMsgs(msgs), { d: "in", ts: m.ts, text: m.text, file: "", size: 0, path: "", from: "", from_name: "" }];
          setTimeout(scrollBottom, 30);
          queueMarkRead(); // v0.9.21 聊天开着即已读
        } else {
          unread = { ...unread, [m.uid]: (unread[m.uid] ?? 0) + 1 };
          toast(`💬 ${m.nickname}：${m.text.slice(0, 24)}`, "info");
        }
      });
      unlistenGroupMsg = await listen<GroupMsgEvt>("social-group-msg", (ev) => {
        const m = ev.payload;
        previews = { ...previews, [`group_${m.gid}`]: { text: m.text, ts: m.ts } };
        if (target?.kind === "group" && target.group.gid === m.gid) {
          msgs = [...asMsgs(msgs), { d: m.from === st?.uid ? "out" : "in", ts: m.ts, text: m.text, file: "", size: 0, path: "", from: m.from, from_name: m.name }];
          setTimeout(scrollBottom, 30);
        } else {
          groupUnread = { ...groupUnread, [m.gid]: (groupUnread[m.gid] ?? 0) + 1 };
          const gname = (st?.groups ?? []).find((g) => g.gid === m.gid)?.name ?? "群聊";
          if (m.from !== st?.uid) toast(`💬 ${gname} · ${m.name}：${m.text.slice(0, 24)}`, "info");
        }
      });
      unlistenOffer = await listen<FileOfferEvt>("social-file-offer", (ev) => {
        offers = [...offers, { ...ev.payload, ts: Date.now() }];
        toast(`📄 ${ev.payload.from_name} 想发送「${ev.payload.name}」（${fmtSize(ev.payload.size)}）`, "info");
      });
      unlistenXfer = await listen<XferOut>("social-xfer", (ev) => {
        ingestXfer(ev.payload);
      });
      // v0.9.13 文件完成 → 聊天流实时出现文件卡（发送/接收都是）
      unlistenRec = await listen<{ key: string; d: string; ts: number; file: string; size: number; path: string }>("social-file-record", (ev) => {
        const r = ev.payload;
        previews = { ...previews, [r.key]: { text: `📄 ${r.file}`, ts: r.ts } };
        if (target && chatKey === r.key) {
          msgs = [...asMsgs(msgs), { d: r.d === "out" ? "out" : "in", ts: r.ts, text: "", file: r.file, size: r.size, path: r.path, from: "", from_name: "" }];
          setTimeout(scrollBottom, 30);
        } else if (r.d === "in") {
          toast(`📄 已接收「${r.file}」`, "ok");
        }
      });
      // v0.9.21 已读回执
      readMap = await socialReadAll().catch(() => ({}));
      unlistenRead = await listen<{ uid: string; up_to: number }>("social-read", (ev) => {
        readMap = { ...readMap, [ev.payload.uid]: Math.max(readMap[ev.payload.uid] ?? 0, ev.payload.up_to) };
      });
      // v0.9.17 对方撤回文件邀请：邀请卡整体退场 + 明确提示
      unlistenOfferGone = await listen<{ id: string; text?: string }>("social-offer-gone", (ev) => {
        offers = offers.filter((x) => x.id !== ev.payload.id);
        const { [ev.payload.id]: _drop, ...rest } = xfers;
        xfers = rest;
        hiddenXfers.add(ev.payload.id);
        toast(ev.payload.text || "对方取消了文件发送", "info");
      });
      // v0.9.17 传输中被对方取消：终态卡由 emit_final 退场，这里只补提示
      unlistenNote = await listen<{ id: string; text?: string }>("social-xfer-note", (ev) => {
        toast(ev.payload.text || "对方取消了文件传输", "info");
      });
      // v0.9.16 收到抖一抖：面板轻抖 + 提示（窗口原生抖动在后端）
      unlistenShake = await listen<{ name?: string }>("social-shake", (ev) => {
        toast(`💢 ${ev.payload?.name || "对方"} 给你发了一个抖一抖`, "info");
        panelShake(1500);
      });
      unlistenSync = await listen<{ gid: string }>("social-group-sync", () => { refresh(); });
      unlistenGone = await listen<{ gid: string }>("social-group-gone", (ev) => {
        if (target?.kind === "group" && target.group.gid === ev.payload.gid) closeChat();
        refresh();
        toast("群聊已被群主解散", "info");
      });
      unlistenNudge = await listen<{ name?: string; from_name?: string }>("social-nudge", (ev) => {
        const who = ev.payload?.from_name ? `来自 ${ev.payload.from_name}` : "";
        toast(`📣 收到一键提醒 ${who}`, "info");
      });
    } catch {}
  });
  onDestroy(() => {
    if (scanTimer) clearTimeout(scanTimer);
    unsubOpen?.();
    xferTimers.forEach((t) => clearTimeout(t));
    if (timer) clearInterval(timer);
    if (saveTimer) clearTimeout(saveTimer);
    unlistenDrop?.();
    unlistenMsg?.(); unlistenGroupMsg?.(); unlistenOffer?.(); unlistenXfer?.(); unlistenRec?.(); unlistenShake?.(); unlistenOfferGone?.(); unlistenNote?.(); unlistenRead?.(); unlistenSync?.(); unlistenGone?.(); unlistenNudge?.();
  });

  // 当前会话相关传输条目（进度卡片；st5=待确认邀请，单独渲染邀请卡，v0.9.2 修复"出现两个状态"）
  $: activeXfers = Object.values(xfers).filter((x) => {
    if (x.st === 5 && x.dir === "in") return false; // 入站待回应 → 邀请卡（带接收/拒绝）
    // 出站 st5 = 等待对方接收，显示为等待卡（带取消）
    if (!target) return false;
    if (target.kind === "group") return x.gid === target.group.gid;
    return !x.gid && x.peer === target.peer.uid;
  });
  // 邀请卡：事件流 + 轮询快照（st5）合并去重，页面重开也不丢
  type OfferView = { id: string; name: string; size: number; gid: string; from_name: string; offset: number };
  $: pendingOffers = (() => {
    const out = new Map<string, OfferView>();
    for (const x of Object.values(xfers)) {
      if (x.st !== 5 || x.dir !== "in") continue; // 只允许入站条目进邀请卡（出站绝不出现接收/拒绝按钮）
      if (!target) continue;
      if (target.kind === "group" ? x.gid !== target.group.gid : !(!x.gid && x.peer === target.peer.uid)) continue;
      out.set(x.id, { id: x.id, name: x.name, size: x.size, gid: x.gid, from_name: xferPeerName(x), offset: 0 });
    }
    for (const o of offers) {
      if (!target) continue;
      if (target.kind === "group" ? o.gid !== target.group.gid : !(!o.gid && o.from === target.peer.uid)) continue;
      if (!out.has(o.id)) out.set(o.id, { id: o.id, name: o.name, size: o.size, gid: o.gid, from_name: o.from_name, offset: Number(o.offset) || 0 });
    }
    return [...out.values()];
  })();
  // 传输卡上的对端名（群发时一眼分清是发给谁的；接收时显示来源）
  function xferPeerName(x: XferOut): string {
    if (x.gid) return (st?.groups ?? []).find((g) => g.gid === x.gid)?.name ?? "群聊";
    const p = (st?.peers ?? []).find((q) => q.uid === x.peer);
    return p ? dispName(p) : x.peer.slice(0, 8);
  }
  // 事件到达时同样纳入终态自动移除
  function ingestXfer(x: XferOut) {
    if (isTerm(x.st) && hiddenXfers.has(x.id)) return;
    const cur = xfers[x.id];
    // v0.9.13 状态跃迁沿提示一次（事件与快照双路合流，不会重复）
    if (cur && cur.st !== x.st) {
      if (x.st === 3) toast(x.dir === "out" ? `「${x.name}」已发送` : `已接收「${x.name}」`, "ok");
      else if (x.st === 4) toast(`「${x.name}」传输失败`, "err");
    }
    xfers = { ...xfers, [x.id]: x };
    if (isTerm(x.st)) scheduleXferRemoval(x.id, x.st === 4 ? 6000 : 2600);
    // v0.9.17 完成自愈：无论事件链路是否丢达，700ms 后对账聊天流文件卡（收/发一致）
    if (x.st === 3) {
      const key = x.gid ? `group_${x.gid}` : x.peer;
      setTimeout(() => reconcileFileBubble(key, x.name, x.size), 700);
    }
  }

  // v0.9.17 文件卡对账：聊天流缺失同名同大小记录 → 从历史尾段补齐（幂等，绝不重复追加）
  function reconcileFileBubble(key: string, file: string, size: number) {
    if (!target || chatKey !== key) return;
    if (msgs.some((m) => m.file === file && m.size === size)) return;
    socialHistory(key).then((h) => {
      const rec = [...h].reverse().find((r) => r.file === file && r.size === size);
      if (!rec) return;
      if (msgs.some((m) => m.file === rec.file && m.size === rec.size && Math.abs(m.ts - rec.ts) < 60_000)) return;
      msgs = [...asMsgs(msgs), { d: rec.d === "out" ? "out" : "in", ts: rec.ts, text: "", file: rec.file, size: rec.size, path: rec.path, from: "", from_name: "" }];
      setTimeout(scrollBottom, 30);
    }).catch(() => {});
  }

  let offerBusy = ""; // 正在建立连接的邀请 id（按钮即时反馈，杜绝"点了没反应"）
  async function acceptOffer(o: { id: string; name: string }) {
    if (offerBusy) return;
    offerBusy = o.id;
    try {
      await socialFileAccept(o.id);
      offers = offers.filter((x) => x.id !== o.id);
      toast(`开始接收「${o.name}」`, "ok");
    } catch (e) {
      // 邀请保留在卡片上，网络恢复后可直接再点
      toast(String(e), "err");
    } finally {
      offerBusy = "";
    }
  }
  async function rejectOffer(o: { id: string }) {
    if (offerBusy === o.id) return;
    offers = offers.filter((x) => x.id !== o.id);
    try { await socialFileReject(o.id); } catch {}
  }

  function openXferFile(path: string) {
    socialOpenPath(path).catch((e) => toast(String(e), "err"));
  }

  // v0.9.16 聊天面板轻抖（本地视觉反馈）
  function panelShake(ms = 500) {
    const el = msgsEl;
    if (!el) return;
    el.classList.remove("shaking");
    void el.offsetWidth;
    el.classList.add("shaking");
    setTimeout(() => el.classList.remove("shaking"), ms);
  }
  let shakeCd = false;
  async function shakePeer() {
    if (!target || target.kind !== "peer" || shakeCd) return;
    shakeCd = true;
    setTimeout(() => (shakeCd = false), 2500);
    try {
      await socialShake(target.peer.uid);
      toast("已向对方发起抖一抖", "ok");
      panelShake(500);
    } catch (e) {
      toast(String(e), "err");
    }
  }

  // v0.9.14 文件卡右键菜单（iOS 式浮动菜单：打开 / 打开所在文件夹 / 复制路径）
  let fileCtx: { x: number; y: number; path: string; name: string; size: number } | null = null;
  function openFileCtx(e: MouseEvent, m: { path: string; file: string; size?: number }) {
    if (!m.path) return;
    e.preventDefault();
    const w = 196, h = 172;
    fileCtx = {
      path: m.path, name: m.file, size: m.size ?? 0,
      x: Math.max(8, Math.min(e.clientX, window.innerWidth - w - 8)),
      y: Math.max(8, Math.min(e.clientY, window.innerHeight - h - 8)),
    };
  }
  function ctxOpen() {
    if (fileCtx) socialOpenPath(fileCtx.path).catch((e) => toast(String(e), "err"));
    fileCtx = null;
  }
  function ctxReveal() {
    if (fileCtx) socialRevealPath(fileCtx.path).catch((e) => toast(String(e), "err"));
    fileCtx = null;
  }
  async function ctxCopy() {
    if (fileCtx) {
      try { await navigator.clipboard.writeText(fileCtx.path); toast("已复制文件路径", "ok"); }
      catch { toast("复制失败", "err"); }
    }
    fileCtx = null;
  }

  // ===== v0.9.18 右键转发（消息 / 文件，聊天流与记录查看器通用）=====
  type Fwd =
    | { kind: "text"; text: string }
    | { kind: "file"; path: string; name: string; size: number };
  let fwd: Fwd | null = null;
  let fwdQ = "";
  let fwdBusy = false;

  function openMsgCtx(e: MouseEvent, m: { text: string }) {
    if (!m.text) return;
    e.preventDefault();
    msgCtx = { x: Math.max(8, Math.min(e.clientX, window.innerWidth - 196 - 8)), y: Math.max(8, Math.min(e.clientY, window.innerHeight - 110 - 8)), text: m.text };
  }
  let msgCtx: { x: number; y: number; text: string } | null = null;
  function msgCtxCopy() {
    if (msgCtx) navigator.clipboard.writeText(msgCtx.text).then(() => toast("已复制", "ok")).catch(() => toast("复制失败", "err"));
    msgCtx = null;
  }
  function msgCtxFwd() {
    if (msgCtx) fwd = { kind: "text", text: msgCtx.text };
    msgCtx = null;
  }
  function ctxForwardFile() {
    if (fileCtx) fwd = { kind: "file", path: fileCtx.path, name: fileCtx.name, size: fileCtx.size };
    fileCtx = null;
  }
  function fwdFile(m: { path: string; file?: string; name?: string; size?: number }) {
    if (!m.path) return;
    fwd = { kind: "file", path: m.path, name: (m.file || m.name || ""), size: m.size ?? 0 };
  }
  async function doForward(t: { kind: "peer" | "group"; uid?: string; gid?: string; name: string }) {
    if (!fwd || fwdBusy) return;
    fwdBusy = true;
    try {
      if (fwd.kind === "text") {
        if (t.kind === "peer" && t.uid) await socialSendText(t.uid, fwd.text);
        else if (t.gid) await socialGroupSend(t.gid, fwd.text);
      } else {
        if (t.kind === "peer" && t.uid) {
          await socialSendFile(t.uid, fwd.path);
        } else if (t.gid) {
          const g = (st?.groups ?? []).find((x) => x.gid === t.gid);
          const others = (g?.members ?? []).filter((m) => m.uid !== st?.uid);
          for (const m of others) {
            try { await socialSendFile(m.uid, fwd.path, t.gid); } catch { /* 单个失败不中断 */ }
          }
        }
      }
      toast(`已转发给 ${t.name}`, "ok");
      fwd = null;
      fwdQ = "";
    } catch (e) {
      toast(String(e), "err");
    }
    fwdBusy = false;
  }

  // ===== v0.9.21 已读回执（好友聊天）=====
  let readMap: Record<string, number> = {}; // uid → 对端读到我方消息的最大 ts
  let markTimer: ReturnType<typeof setTimeout> | null = null;
  function queueMarkRead() {
    if (markTimer) clearTimeout(markTimer);
    markTimer = setTimeout(markChatRead, 80); // 批到突发消息
  }
  async function markChatRead() {
    if (!target || target.kind !== "peer") return;
    const uid = target.peer.uid;
    let up = 0;
    for (const m of msgs) if (m.d === "in" && m.ts > up) up = m.ts;
    if (up <= (readMap[uid] ?? 0)) return; // 无新进展不发
    readMap = { ...readMap, [uid]: up }; // 乐观更新（气泡立即亮"已读"逻辑对称）
    socialMarkRead(uid, up).catch(() => {});
  }
  $: lastOutTs = msgs.reduce((a, m) => (m.d === "out" && m.ts > a ? m.ts : a), 0);

  $: fwdQs = fwdQ.trim().toLowerCase();
  $: peersFwd = {
    label: "好友",
    items: (st?.peers ?? [])
      .filter((p) => !fwdQs || dispName(p).toLowerCase().includes(fwdQs))
      .sort((a, b) => Number(b.online) - Number(a.online))
      .map((p) => ({ kind: "peer" as const, uid: p.uid, name: dispName(p), av: p.avatar, online: p.online, count: 0 })),
  };
  $: groupsFwd = {
    label: "群聊",
    items: (st?.groups ?? [])
      .filter((g) => !fwdQs || g.name.toLowerCase().includes(fwdQs))
      .map((g) => ({ kind: "group" as const, gid: g.gid, name: g.name, av: 0, online: true, count: g.members.length })),
  };

  // v0.9.4 输入框自适应增高：1 行起步，随内容长高，上限 150px（≈6 行），发送后回落
  // v0.9.9 手动高度：拖拽顶部把手可固定高度（44-300px，双击把手恢复自适应），持久化
  let draftEl: HTMLTextAreaElement | null = null;
  let inputManual = false;
  let inputH = 44;
  {
    const v = Number(localStorage.getItem("tz-chat-input-h"));
    if (v >= 44 && v <= 300) { inputH = v; inputManual = true; }
  }
  function autoGrow() {
    if (inputManual || !draftEl) return;
    draftEl.style.height = "auto";
    // v0.9.16：上限 = min(300px, 聊天区高度 45%)，下限 150px —— 长内容平滑长高，短内容不占空间
    const cap = Math.max(150, Math.min(300, Math.round((msgsEl?.clientHeight || 500) * 0.45)));
    draftEl.style.height = Math.min(cap, draftEl.scrollHeight) + "px";
  }
  function footGripDown(e: MouseEvent) {
    if (e.button !== 0 || !draftEl) return;
    e.preventDefault();
    const startY = e.clientY;
    const startH = inputManual ? inputH : draftEl.offsetHeight;
    document.body.style.cursor = "row-resize";
    document.body.style.userSelect = "none";
    const move = (ev: MouseEvent) => {
      inputH = Math.max(44, Math.min(300, Math.round(startH - (ev.clientY - startY))));
      inputManual = true;
    };
    const up = () => {
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
      try {
        if (inputManual) localStorage.setItem("tz-chat-input-h", String(inputH));
        else localStorage.removeItem("tz-chat-input-h");
      } catch {}
      requestAnimationFrame(autoGrow);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  }
  function footGripReset() {
    inputManual = false;
    try { localStorage.removeItem("tz-chat-input-h"); } catch {}
    requestAnimationFrame(autoGrow);
  }

  function insertEmoji(e: string) {
    draft += e;
    pushRecent(e);
  }
  // v0.9.4 点输入框 = 要打字了，表情浮窗自动让位
  function closeEmojiOnFocus() {
    if (showEmoji) showEmoji = false;
  }

</script>

<div class="view" style="--chat-fs: {$chatFontSize}px">
  <!-- 状态条（纤薄） -->
  <div class="card svc" style="flex:none">
    <div class="svc-head">
      <span class="dot" class:on={st?.running} class:wait={!st}></span>
      {#if st}
        <b>{st.running ? "社交服务在线" : "社交服务未运行"}</b>
        {#if st.running}
          <span class="dim">端口 {st.port} · 无服务无驱动无常驻</span>
        {:else}
          <span class="dim">{st.reason || "可在设置中开启「随工具启动社交服务」"}</span>
        {/if}
      {:else}
        <b>正在连接社交服务…</b>
      {/if}
      <span style="flex:1"></span>
      {#if st?.running}
        {#if st.firewall === "ok"}
          <span class="chip fw-ok">防火墙已放行</span>
        {:else}
          <button type="button" class="chip fw-fix" title="点击后在 UAC 弹窗点「是」；若安装安全软件，请在其弹窗允许 TexMantis 联网"
            on:click={fixFw}>⚠ 网络权限待修复 · 点击允许</button>
        {/if}
        <span class="chip mono" title="本机加密指纹，可与他人核对">🔒 {st.fp}</span>
      {/if}
      <button type="button" class="mini gear-btn" title="社交功能设置（接收目录 · 数据目录）" on:click={openSettings}>⚙</button>
      <button type="button" class="mini swap-btn" title="调换左右两栏位置（聊天 ↔ 联系人）" on:click={flipLayout}>⇄</button>
    </div>
  </div>

  <div class="im-wrap" class:flipped={!chatLeft}>
    <!-- v0.9.8 联系人栏：宽度可拖拽（把手在栏内侧边缘，调换后自动跟随到另一侧） -->
    <!-- 左栏：聊天 -->
    <div class="card chat-card" bind:this={chatCardEl}>
      {#if dragOver}
        <div class="drop-veil">📄 松开发送到「{chatTitle}」</div>
      {/if}
      {#if target?.kind === "peer"}
        <div class="chat-head">
          <span class="pav" style="background:{AV_BG[target.peer.avatar] ?? '#64748b'}">{AV_EMOJI[target.peer.avatar] ?? "🙂"}</span>
          <div class="ch-info">
            <b>{chatTitle}{target.peer.remark ? `（${target.peer.nickname}）` : ""}</b>
            <div class="dim">{target.peer.host} · {target.peer.ip}</div>
          </div>
          <span style="flex:1"></span>
          {#if sessState === "connecting"}
            <span class="chip sess-wait" title="正在建立端到端加密会话（X25519 协商）">◌ 连接中…</span>
          {:else if sessState === "fail"}
            <span class="chip sess-bad" title={sessErr}>⚠ 未连接 · 可查看记录</span>
          {:else}
            <span class="fp" title="端到端加密指纹：与对方核对一致，即确认无中间人">🔒 {chatFp}</span>
          {/if}
          <button type="button" class="mini" title="清空本地聊天记录（不可恢复，仅影响本机）" on:click={clearCurrentHistory}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2m2 0v14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V6"/></svg> 清空
          </button>
          <button type="button" class="mini" title="查看聊天记录（消息/文件），支持搜索" on:click={openHist}>📜 记录</button>
          <button type="button" class="mini" on:click={closeChat}><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg> 收起</button>
        </div>
        <div class="msgs" bind:this={msgsEl}>
          {#each msgs as m, i (i)}
            <div class="mrow" class:out={m.d === "out"}>
              {#if m.file}
                <div class="bub fcard clickable" role="button" tabindex="0" title={m.path ? `${m.path}（双击打开 · 右键更多）` : m.file} on:dblclick={() => m.path && openXferFile(m.path)} on:keydown={(e) => e.key === "Enter" && m.path && openXferFile(m.path)} on:contextmenu|preventDefault|stopPropagation={(e) => openFileCtx(e, m)}>
                  <span class="fico doc"><svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><path d="M13 2v7h7"/></svg></span>
                  <span class="fmeta"><b>{m.file}</b><span class="dim">{fmtSize(m.size)} · {m.d === "out" ? "已发送" : "已接收"}{m.path ? " · 双击打开" : ""}</span></span>
                </div>
              {:else}
                <!-- svelte-ignore a11y-no-static-element-interactions -->
                <div class="bub" title={fmtT(m.ts)} on:contextmenu={(e) => openMsgCtx(e, m)}>{m.text}</div>
              {/if}
              {#if m.d === "out" && !m.file && m.ts === lastOutTs && target?.kind === "peer" && (readMap[target.peer.uid] ?? 0) >= m.ts}
                <span class="read-tag">已读</span>
              {/if}
            </div>
          {:else}
            <div class="empty-msg">聊点什么吧 —— 消息经端到端加密传输（X25519 + ChaCha20-Poly1305）</div>
          {/each}
        </div>
      {:else if target?.kind === "group"}
        <div class="chat-head">
          <span class="pav gav">👥</span>
          <div class="ch-info">
            <b>{chatTitle}</b>
            <div class="dim">{target.group.members.length} 人 · 群主 {memberName(target.group, target.group.owner)}</div>
          </div>
          <span style="flex:1"></span>
          <span class="chip gchip">端到端加密 · 逐人单播</span>
          {#if amAdmin(target.group)}
            <button type="button" class="mini" title="群管理（成员/管理员/群名/邀请）" on:click={openGroupMgr}>👥 管理</button>
          {/if}
          <button type="button" class="mini" title="清空本地聊天记录（不可恢复，仅影响本机）" on:click={clearCurrentHistory}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2m2 0v14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V6"/></svg> 清空
          </button>
          <button type="button" class="mini" title="查看聊天记录（消息/文件），支持搜索" on:click={openHist}>📜 记录</button>
          <button type="button" class="mini" on:click={closeChat}><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="m6 9 6 6 6-6"/></svg> 收起</button>
        </div>
        <div class="msgs" bind:this={msgsEl}>
          {#each msgs as m, i (i)}
            <div class="mrow" class:out={m.d === "out"}>
              {#if m.file}
                <div class="bub fcard clickable" role="button" tabindex="0" title={m.path ? `${m.path}（双击打开 · 右键更多）` : m.file} on:dblclick={() => m.path && openXferFile(m.path)} on:keydown={(e) => e.key === "Enter" && m.path && openXferFile(m.path)} on:contextmenu|preventDefault|stopPropagation={(e) => openFileCtx(e, m)}>
                  <span class="fico doc"><svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><path d="M13 2v7h7"/></svg></span>
                  <span class="fmeta"><b>{m.file}</b><span class="dim">{fmtSize(m.size)} · {m.d === "out" ? "已发送" : "已接收"}{m.path ? " · 双击打开" : ""}</span></span>
                </div>
              {:else}
                <div class="gwrap">
                  {#if msgSender(m)}<div class="gsender">{msgSender(m)} · {fmtT(m.ts)}</div>{/if}
                  <!-- svelte-ignore a11y-no-static-element-interactions -->
                  <div class="bub" title={fmtT(m.ts)} on:contextmenu={(e) => openMsgCtx(e, m)}>{m.text}</div>
                </div>
              {/if}
            </div>
          {:else}
            <div class="empty-msg">群聊已就绪 —— 消息对每位成员端到端加密单播</div>
          {/each}
        </div>
      {:else}
        <div class="chat-empty">
          <div class="ce-icon">💬</div>
          <b>选择右侧联系人或群聊，开始加密沟通</b>
          <span class="dim">文字/文件经 X25519 协商 + ChaCha20-Poly1305 端到端加密，<br />指纹核对一致即可确认无中间人</span>
        </div>
      {/if}

      {#if target}
        <!-- 文件传输条：待确认邀请 + 进度卡片 -->
        {#if pendingOffers.length || activeXfers.length}
          <div class="xfer-strip">
            {#each pendingOffers as o (o.id)}
              <div class="xcard">
                <span class="fico">📥</span>
                <span class="fmeta"><b>{o.name}</b><span class="dim">{fmtSize(o.size)} · {o.from_name} 发来{o.offset > 0 ? ` · 已有 ${fmtSize(o.offset)} 可续传` : ""}</span></span>
                <button type="button" class="mini ok" disabled={!!offerBusy}
                  on:click={() => acceptOffer(o)}>{offerBusy === o.id ? "连接中…" : "接收"}</button>
                <button type="button" class="mini" disabled={!!offerBusy} on:click={() => rejectOffer(o)}>拒绝</button>
              </div>
            {/each}
            {#each activeXfers as x (x.id)}
              {#if x.st !== 2}
                <div class="xcard" class:xdone={x.st === 3}>
                  <span class="fico">{x.dir === "out" ? "📤" : "📥"}</span>
                  <span class="fmeta">
                    <b>{x.name} <span class="dim">{x.dir === "out" ? "→ 发送给 " : "← 来自 "}{xferPeerName(x)}</span></b>
                    <span class="xbar"><span class="xbar-in" class:paused={x.st === 1} style="width:{Math.min(100, (x.done / Math.max(1, x.size)) * 100)}%"></span></span>
                    <span class="dim">{xferStateText(x.st, x.dir)} · {fmtSize(x.done)} / {fmtSize(x.size)}</span>
                  </span>
                  {#if x.st === 0}
                    <button type="button" class="mini" on:click={() => xAct(x.id, "pause")}>暂停</button>
                  {:else if x.st === 1 || x.st === 4}
                    <button type="button" class="mini ok" on:click={() => xAct(x.id, x.st === 4 && x.dir === "out" ? "retry" : x.st === 4 ? "receive" : "resume")}>续传</button>
                  {:else if x.st === 3}
                    <span class="xdone-tick">✓</span>
                  {/if}
                  {#if x.st === 0 || x.st === 1 || (x.st === 5 && x.dir === "out")}
                    <button type="button" class="mini bad" on:click={() => xAct(x.id, "cancel")}>取消</button>
                  {/if}
                </div>
              {/if}
            {/each}
          </div>
        {/if}

        <!-- 群管理员：内容编辑上方小工具栏（一键提醒） -->
        {#if target.kind === "group" && amAdmin(target.group)}
          <div class="adm-bar">
            <span class="dim">群工具：</span>
            <button type="button" class="mini" title="向全体成员发送提醒：自动弹出对方工具面板到社交界面，并伴随提示音" on:click={() => gAction("nudge")}>📣 一键提醒</button>
            <button type="button" class="mini" on:click={openGroupMgr}>群管理</button>
          </div>
        {/if}

        <div class="emoji-bar" class:open={showEmoji}>
          <div class="etabs">
            {#each EMOJI_TABS as t (t)}
              <button type="button" class:on={emojiTab === t} on:click={() => { emojiTab = t; emojiQ = ""; }}>{t}</button>
            {/each}
            <input type="text" class="esearch" placeholder="搜索表情…" bind:value={emojiQ} />
          </div>
          <div class="egrid">
            {#each emojiShown as e (e)}
              <button type="button" on:click={() => insertEmoji(e)}>{e}</button>
            {/each}
            {#if !emojiShown.length}
              <div class="dim es-empty">没有匹配的表情</div>
            {/if}
          </div>
        </div>
        <div class="chat-foot">
          <div class="foot-grip" role="presentation" title="拖动调整输入框高度（双击恢复自适应）"
            on:mousedown={footGripDown} on:dblclick={footGripReset}></div>
          <button type="button" class="mini emoji-toggle" title="表情" on:click={() => (showEmoji = !showEmoji)}>😀</button>
          <button type="button" class="mini file-btn" title="发送文件（支持断点续传 / 拖拽到聊天窗）" on:click={pickAndSend}>
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><path d="M13 2v7h7"/></svg>

          </button>
          <textarea rows="1" bind:this={draftEl} bind:value={draft} maxlength="4000"
            class:manual={inputManual} style:height={inputManual ? inputH + "px" : null}
            placeholder="输入消息，Enter 发送，Shift + Enter 换行"
            on:input={autoGrow} on:focus={closeEmojiOnFocus}
            on:keydown={(e) => { if (e.key === "Enter" && !e.shiftKey && !e.isComposing) { e.preventDefault(); send(); } }}></textarea>
          <button type="button" class="mini shake-btn" title="抖一抖：让对方窗口抖动 2 秒"
            on:click={shakePeer}>
            <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M22 12h-4l-3 9L9 3l-3 9H2"/></svg>
          </button>
          <button type="button" class="send" disabled={sending} on:click={send}>{sending ? "发送中…" : "发送"}</button>
        </div>
      {/if}
    </div>

    <!-- 右栏：联系人 / 群聊 -->
    <div class="card contacts" bind:this={contactsEl} style="width:{contactsW}px">
      <div class="plist-grip" role="presentation" title="拖动调整联系人列表宽度" on:mousedown={plistGripDown}></div>
      <div class="sec-head" title="联系人（列表随时可拖缘调宽）">
        <span style="flex:1"></span>
        <button type="button" class="pill pill-bcast" title="广播：一键群发（逐人端到端加密单播）" on:click={openBcast}><span class="pi">📣</span>广播</button>
        <button type="button" class="pill pill-group" title="创建群聊" on:click={openCreateGroup}><span class="pi">➕</span>建群</button>
        <button type="button" class="radar-btn" class:spin={scanning} disabled={scanning} on:click={radar}>
          <span class="sweep"></span>{scanning ? "扫描中" : "雷达"}
        </button>
      </div>
      <div class="side-tabs">
        <button type="button" class:on={sideTab === "peer"} on:click={() => (sideTab = "peer")}>好友 {sortedPeers.length ? `(${sortedPeers.length})` : ""}</button>
        <button type="button" class:on={sideTab === "group"} on:click={() => (sideTab = "group")}>群聊 {groupsList.length ? `(${groupsList.length})` : ""}{favCount ? "" : ""}</button>
      </div>
      <div class="plist">
        {#if sideTab === "peer"}
          {#if sortedPeers.length}
            {#each sortedPeers as p (p.uid)}
              <div class="prow" class:active={target?.kind === "peer" && target.peer.uid === p.uid} class:offline={!p.online}
                role="button" tabindex="0"
                on:click={() => openChat(p)} on:keydown={(e) => e.key === "Enter" && openChat(p)}
                on:contextmenu|preventDefault|stopPropagation={() => (peerInfo = p)}>
                <span class="pav-wrap">
                  <span class="pav" class:online={p.online} style="background:{AV_BG[p.avatar] ?? '#64748b'}">{AV_EMOJI[p.avatar] ?? "🙂"}</span>
                  {#if p.gender === "m" || p.gender === "f"}
                    <span class="gbadge {p.gender === "f" ? "gf" : "gm"}">{p.gender === "f" ? "♀" : "♂"}</span>
                  {/if}
                </span>
                <div class="pmid">
                  <div class="pline1">
                    <b>{dispName(p)}</b>
                    {#if p.remark}<span class="dim small">{p.nickname}</span>{/if}
                    {#if p.fav}<span class="star">★</span>{/if}
                    {#if previewText(p)}<span class="ptime">{previewTime(p)}</span>{/if}
                  </div>
                  <div class="pline2">{previewText(p) || `${p.host} · ${p.ip}`}</div>
                </div>
                {#if unread[p.uid]}
                  <span class="badge-u">{unread[p.uid] > 99 ? "99+" : unread[p.uid]}</span>
                {/if}
                <button type="button" class="fav-btn" title={p.remark ? "修改备注" : "设置备注"}
                  on:click|stopPropagation={() => openRemark(p)}>✎</button>
                <button type="button" class="fav-btn" title={p.fav ? "移除收藏" : "收藏"}
                  on:click|stopPropagation={() => toggleFav(p)}>{p.fav ? "★" : "☆"}</button>
                <button type="button" class="fav-btn rm" title="移除联系人"
                  on:click|stopPropagation={() => removePeer(p)}>✕</button>
              </div>
            {/each}
          {:else}
            <div class="dim pempty">
              {#if st && !st.running}
                社交服务未运行，无法发现同伴
              {:else}
                还没有发现同伴 —— 对方也在运行本工具并打开社交页后，点「雷达」互相发现
              {/if}
            </div>
          {/if}
        {:else}
          {#if groupsList.length}
            {#each groupsList as g (g.gid)}
              <div class="prow" class:active={target?.kind === "group" && target.group.gid === g.gid}
                role="button" tabindex="0"
                on:click={() => openGroup(g)} on:keydown={(e) => e.key === "Enter" && openGroup(g)}>
                <span class="pav gav">👥</span>
                <div class="pmid">
                  <div class="pline1">
                    <b>{g.name}</b>
                    <span class="dim small">{g.members.length}人</span>
                    {#if groupPreview(g)}<span class="ptime">{fmtTime(previews[`group_${g.gid}`]?.ts ?? 0)}</span>{/if}
                  </div>
                  <div class="pline2">{groupPreview(g) || `群主 ${memberName(g, g.owner)}`}</div>
                </div>
                {#if groupUnread[g.gid]}
                  <span class="badge-u">{groupUnread[g.gid] > 99 ? "99+" : groupUnread[g.gid]}</span>
                {/if}
              </div>
            {/each}
          {:else}
            <div class="dim pempty">还没有群聊 —— 点上方「➕ 建群」创建</div>
          {/if}
        {/if}
      </div>
      <div class="dim pnote">文件断点续传 · 群聊 · 广播 · 一键提醒（v0.9.0）</div>
    </div>
  </div>

  <!-- 我的档案：右下角收纳 -->
  <!-- v0.9.4 面板 / 聊天 模式切换（iOS 拟物滑块，两种模式都常驻底部） -->
  <!-- v0.9.5 底部行：左右等宽弹性衬垫让切换器始终居中于社交区（面板模式含导航时亦然），
       导航收拢的 0.28s 过渡中 flex 逐帧回流 → 切换器平滑滑向全宽中点；
       资料徽章与切换器同一水平线、贴右侧栏位 -->
  <div class="bottom-row">
    <div class="bspacer"></div>
    <div class="mode-sw" role="tablist" title="切换社交界面模式">
      <div class="thumb" class:right={$focusMode}></div>
      <button type="button" role="tab" class:on={!$focusMode} aria-selected={!$focusMode}
        on:click={() => focusMode.set(false)}>▦ 面板</button>
      <button type="button" role="tab" class:on={$focusMode} aria-selected={$focusMode}
        on:click={() => focusMode.set(true)}>💬 聊天</button>
    </div>
    <div class="bspacer bsp-r">
      {#if st}
        <button type="button" class="me-chip" title="个人资料与设置" on:click={() => (showMe = !showMe)}>
          <span class="pav big" style="background:{AV_BG[st.avatar]}">{AV_EMOJI[st.avatar]}</span>
          <span class="me-name">{st.nickname}</span>
          <span class="chip-gear">⚙</span>
        </button>
      {/if}
    </div>
  </div>
</div>

<!-- 我的档案弹窗 -->
{#if showMe}
  <div class="me-mask" role="presentation" on:click|self={() => (showMe = false)}>
    <div class="me-pop">
      <div class="me-head"><b>社交设置</b><span class="dim">我的档案 · 修改即自动保存</span><span style="flex:1"></span><button type="button" class="mini me-close" title="关闭（Esc）" on:click={() => (showMe = false)}>✕</button></div>
      <div class="av-grid">
        {#each AV_EMOJI as e, i (i)}
          <button type="button" class="av" class:sel={avatarDraft === i}
            style="background:{AV_BG[i]}" title="头像 {i + 1}"
            on:click={() => { avatarDraft = i; autoSave(); }}>{e}</button>
        {/each}
      </div>
      <label class="fld">
        <span>昵称</span>
        <input type="text" bind:value={nickDraft} maxlength="16" placeholder={(st?.host ?? "") + " / 系统用户名"}
          on:input={autoSave} />
        <button type="button" class="mini" title="随机一个有趣的名字" on:click={rollName}>🎲</button>
      </label>
      <div class="fld">
        <span>性别</span>
        <div class="seg">
          {#each [["m", "男"], ["f", "女"], ["x", "中性"]] as [v, t] (v)}
            <button type="button" class:sel={genderDraft === v}
              on:click={() => { genderDraft = v; autoSave(); }}>{t}</button>
          {/each}
        </div>
      </div>
      <div class="fld">
        <span>布局</span>
        <div class="lay-row">
          <span class="lay-lab">{chatLeft ? "聊天在左 · 联系人在右" : "联系人在左 · 聊天在右"}</span>
          <input type="checkbox" class="ios-sw" checked={chatLeft} title="调换左右两栏"
            on:change={(e) => { if (e.currentTarget.checked !== chatLeft) flipLayout(); }} />
        </div>
      </div>
      {#if st?.running && st.firewall !== "ok"}
        <div class="dim me-warn">⚠ 网络权限待修复：点状态栏「⚠ 网络权限待修复」按钮，在 UAC 点「是」（Windows 防火墙弹窗若曾点过「取消」将不再弹出，用该按钮可修复）</div>
      {/if}
      <div class="dim me-fp">我的加密指纹 <span class="mono">{st?.fp ?? "…"}</span>（可与他人核对）</div>
    </div>
  </div>
{/if}

<!-- 社交功能设置弹窗（右上角 ⚙） -->
<!-- v0.9.9 联系人资料卡（右键联系人行呼出） -->
{#if peerInfo}
  <div class="me-mask" role="presentation" on:click|self={() => (peerInfo = null)}>
    <div class="me-pop">
      <div class="me-head"><b>联系人资料</b><span class="dim">右键任意联系人随时查看</span><span style="flex:1"></span>
        <button type="button" class="mini me-close" title="关闭（Esc）" on:click={() => (peerInfo = null)}>✕</button></div>
      <div class="info-top">
        <span class="pav big" style="background:{AV_BG[peerInfo.avatar] ?? '#64748b'}">{AV_EMOJI[peerInfo.avatar] ?? "🙂"}</span>
        <div class="info-id">
          <b>{dispName(peerInfo)}</b>
          <span class="dim">{peerInfo.nickname}</span>
          <span class="chip {peerInfo.online ? "sess-ok" : ""}" style="align-self: flex-start">{peerInfo.online ? "● 在线" : "○ 离线"}</span>
        </div>
      </div>
      <div class="info-rows">
        <div class="irow"><span class="dim">性别</span><b>{genderText(peerInfo.gender)}{#if peerInfo.gender === "m"}&nbsp;♂{:else if peerInfo.gender === "f"}&nbsp;♀{/if}</b></div>
        <div class="irow"><span class="dim">设备</span><b>{peerInfo.host}</b></div>
        <div class="irow"><span class="dim">地址</span><b>{peerInfo.ip}:{peerInfo.port}</b></div>
        <div class="irow"><span class="dim">用户 ID</span><b class="mono">{peerInfo.uid.slice(0, 16)}</b></div>
      </div>
      <div class="set-actions">
        <button type="button" class="mini" on:click={infoRemark}>设备注</button>
        <button type="button" class="mini ok" on:click={infoChat}>发消息</button>
      </div>
    </div>
  </div>
{/if}

{#if showSettings}
  <div class="me-mask" role="presentation" on:click|self={() => (showSettings = false)}>
    <div class="me-pop mid">
      <div class="me-head"><b>社交设置</b><span style="flex:1"></span><button type="button" class="mini me-close" title="关闭（Esc）" on:click={() => (showSettings = false)}>✕</button></div>

      <!-- v0.9.25 iOS 分组列表：白色分组卡 + 发丝分隔线，标签卡内左对齐 -->
      <div class="sg">
        <div class="sg-row sg-col">
          <div class="sg-line">
            <span class="sg-lab">接收目录</span>
            <input type="text" bind:value={recvDraft} placeholder="默认：下载 \TexMantis" />
          </div>
          <div class="sg-line">
            <span class="sg-lab"></span>
            <div class="sg-btns">
              <button type="button" class="mini" on:click={pickRecv}>浏览…</button>
              <button type="button" class="mini" on:click={resetRecv}>恢复默认</button>
              <button type="button" class="mini ok" on:click={saveRecv}>保存</button>
            </div>
          </div>
          <div class="dim sg-note">对之后的新传输生效。</div>
        </div>
        <div class="sg-row">
          <span class="sg-lab">数据目录</span>
          <div class="chat-dir" title={dirs?.chat ?? ""}>{dirs?.chat ?? "…"}</div>
          <button type="button" class="mini" on:click={openChatDir}>打开</button>
        </div>
        <div class="sg-row">
          <span class="sg-lab">字号</span>
          <div class="font-row">
            {#if fontPv}
            <div class="font-pv" transition:fade={{ duration: 160 }} style="left: {fontPct}%">
              <div class="pv-card">
                <div class="pv-in" style="font-size: {fontDraft}px">你好，青刃知己！</div>
                <div class="pv-out" style="font-size: {fontDraft}px">字号 {fontDraft}px · Aa 123</div>
              </div>
              <div class="pv-arrow"></div>
            </div>
            {/if}
            <input type="range" min="12" max="17" step="1" bind:value={fontDraft}
              title="聊天文字大小" on:mousedown={fontPvShow} on:input={() => { fontPvShow(); fontLive(); }}
              on:change={() => { saveFont(); fontPvFade(); }} on:blur={fontPvFade} />
          </div>
        </div>
      </div>

      <div class="sg">
        <div class="sg-row">
          <span class="sg-lab">联系人</span>
          <div class="sg-text">
            <b>自动清理疑似僵尸号</b>
            <span class="dim">重装后重复的联系人自动合并</span>
          </div>
          <input type="checkbox" class="ios-sw" checked={dirs?.zombie ?? true} title="同机多身份自动清理"
            on:change={() => toggleZombie()} />
        </div>
        <div class="sg-row sg-col">
          <div class="sg-line">
            <span class="sg-lab">数据管理</span>
            <div class="dim sg-data">
              聊天记录 <b>{fmtSize(usage?.msgs_bytes ?? 0)}</b>
              <span class="sep">·</span> 接收文件 <b>{usage?.files_count ?? 0}</b> 个 / <b>{fmtSize(usage?.files_bytes ?? 0)}</b>
            </div>
          </div>
          <div class="sg-line">
            <span class="sg-lab"></span>
            <div class="sg-btns">
              <button type="button" class="mini" on:click={clearAllHistory}>清除所有聊天记录</button>
              <button type="button" class="mini" on:click={clearRecvFiles}>清除所有接收的文件</button>
            </div>
          </div>
          <div class="dim sg-note">仅影响本机，对方不受影响。</div>
        </div>
      </div>
    </div>
  </div>
{/if}

<!-- v0.9.12 应用内文件选择器（轻量资源管理器） -->
<FilePicker open={showPicker} on:pick={(e) => { showPicker = false; void doSendFile(e.detail); }} on:close={() => (showPicker = false)} />

<!-- v0.9.14 文件卡右键菜单 -->
{#if fileCtx}
  <div class="ctx-veil" role="presentation"
    on:click|self={() => (fileCtx = null)}
    on:contextmenu|preventDefault={() => (fileCtx = null)}>
    <div class="ctx-menu" role="menu" style="left:{fileCtx.x}px; top:{fileCtx.y}px">
      <button type="button" role="menuitem" on:click={ctxOpen}>
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/><path d="M15 3h6v6"/></svg>
        打开
      </button>
      <button type="button" role="menuitem" on:click={ctxReveal}>
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
        打开所在文件夹
      </button>
      <button type="button" role="menuitem" on:click={ctxForwardFile}>
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M22 2 11 13"/><path d="M22 2 15 22l-4-9-9-4z"/></svg>
        转发…
      </button>
      <button type="button" role="menuitem" on:click={ctxCopy}>
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
        复制文件路径
      </button>
    </div>
  </div>
{/if}

<!-- v0.9.18 消息右键菜单（复制 / 转发） -->
{#if msgCtx}
  <div class="ctx-veil" role="presentation"
    on:click|self={() => (msgCtx = null)}
    on:contextmenu|preventDefault={() => (msgCtx = null)}>
    <div class="ctx-menu" role="menu" style="left:{msgCtx.x}px; top:{msgCtx.y}px">
      <button type="button" role="menuitem" on:click={msgCtxCopy}>
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="13" height="13" rx="2"/><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"/></svg>
        复制
      </button>
      <button type="button" role="menuitem" on:click={msgCtxFwd}>
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M22 2 11 13"/><path d="M22 2 15 22l-4-9-9-4z"/></svg>
        转发…
      </button>
    </div>
  </div>
{/if}

<!-- v0.9.18 转发目标选择（iOS 浅色小弹层：搜索 + 好友/群聊分节，点按即转发） -->
{#if fwd}
  <div class="me-mask" role="presentation" on:click|self={() => (fwd = null)}>
    <div class="me-pop small fwd-pop">
      <div class="me-head"><b>转发</b><span class="dim">{fwd.kind === "file" ? fwd.name : (fwd.text.length > 18 ? fwd.text.slice(0, 18) + "…" : fwd.text)}</span><span style="flex:1"></span><button type="button" class="mini me-close" on:click={() => (fwd = null)}>✕</button></div>
      <div class="fwd-search">
        <span>🔍</span>
        <input type="text" placeholder="搜索联系人或群聊" bind:value={fwdQ} />
      </div>
      <div class="fwd-list">
        {#each [peersFwd, groupsFwd] as sect, si (si)}
          {#if sect.items.length}
            <div class="fwd-sec">{sect.label}</div>
            {#each sect.items as t (t.kind === "peer" ? `p:${t.uid}` : `g:${t.gid}`)}
              <button type="button" class="frow" disabled={fwdBusy} on:click={() => doForward(t)}>
                <span class="pav {t.kind === 'group' ? 'online' : ''}" style="width:26px;height:26px;font-size:13px;background:{t.kind === 'group' ? '#34c759' : (AV_BG[t.av] ?? '#64748b')}">{t.kind === "group" ? "👥" : (AV_EMOJI[t.av] ?? "🙂")}</span>
                <b>{t.name}</b>
                <span class="dim">{t.kind === "group" ? `${t.count} 人` : (t.online ? "在线" : "离线")}</span>
                <svg class="farr" width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M22 2 11 13"/><path d="M22 2 15 22l-4-9-9-4z"/></svg>
              </button>
            {/each}
          {/if}
        {/each}
        {#if !peersFwd.items.length && !groupsFwd.items.length}
          <div class="dim pempty">无匹配联系人</div>
        {/if}
      </div>
    </div>
  </div>
{/if}


<!-- 备注好友弹窗 -->
{#if remarkFor}
  <div class="me-mask" role="presentation" on:click|self={() => (remarkFor = null)}>
    <div class="me-pop small">
      <div class="me-head"><b>备注好友</b><span style="flex:1"></span><button type="button" class="mini me-close" on:click={() => (remarkFor = null)}>✕</button></div>
      <div class="fld fld-top">
        <span>备注</span>
        <input type="text" bind:value={remarkDraft} maxlength="16" placeholder={remarkFor.nickname}
          on:keydown={(e) => e.key === "Enter" && saveRemark()} />
      </div>
      <div class="dim" style="padding: 4px 14px 10px; font-size: 11px">备注仅保存在本机，对方看不到；留空即清除备注。</div>
      <div class="pop-actions">
        <button type="button" class="mini" on:click={() => (remarkFor = null)}>取消</button>
        <button type="button" class="mini ok" on:click={saveRemark}>保存</button>
      </div>
    </div>
  </div>
{/if}

<!-- 广播弹窗 -->
{#if showBcast}
  <div class="me-mask" role="presentation" on:click|self={() => (showBcast = false)}>
    <div class="me-pop mid">
      <div class="me-head"><b>📣 群发广播</b><span class="dim">逐人端到端加密单播 · 不产生广播风暴</span><span style="flex:1"></span><button type="button" class="mini me-close" on:click={() => (showBcast = false)}>✕</button></div>
      <div class="fld fld-top">
        <span>内容</span>
        <input type="text" bind:value={bcastText} maxlength="2000" placeholder="例如：12:00 会议室吃饭，来的举手"
          on:keydown={(e) => e.key === "Enter" && doBcast()} />
      </div>
      <div class="bcast-quick">
        <button type="button" class="mini" on:click={() => (bcastSel = (st?.peers ?? []).filter((p) => p.online).map((p) => p.uid))}>全选在线（{(st?.peers ?? []).filter((p) => p.online).length}）</button>
        <button type="button" class="mini" on:click={() => (bcastSel = (st?.peers ?? []).map((p) => p.uid))}>全选全部（{(st?.peers ?? []).length}）</button>
        <button type="button" class="mini" on:click={() => (bcastSel = (st?.peers ?? []).filter((p) => p.fav).map((p) => p.uid))}>仅收藏</button>
        <button type="button" class="mini" on:click={() => (bcastSel = [])}>清空</button>
      </div>
      <div class="bcast-list">
        {#each sortedPeers as p (p.uid)}
          <label class="brow" class:off={!p.online}>
            <input type="checkbox" checked={bcastSel.includes(p.uid)} on:change={() => toggleBcastSel(p.uid)} />
            <span class="pav" class:online={p.online} style="width:22px;height:22px;font-size:12px;background:{AV_BG[p.avatar] ?? '#64748b'}">{AV_EMOJI[p.avatar] ?? "🙂"}</span>
            <b>{dispName(p)}</b>
            <span class="dim">{p.online ? "在线" : "离线（发后自动补投需对方上线，建议选在线）"}</span>
          </label>
        {/each}
      </div>
      <div class="pop-actions">
        <span class="dim">已选 {bcastSel.length} 人</span>
        <span style="flex:1"></span>
        <button type="button" class="mini" on:click={() => (showBcast = false)}>取消</button>
        <button type="button" class="mini ok" disabled={bcastBusy} on:click={doBcast}>{bcastBusy ? "发送中…" : "发送广播"}</button>
      </div>
    </div>
  </div>
{/if}

<!-- 建群弹窗 -->
{#if showCreateGroup}
  <div class="me-mask" role="presentation" on:click|self={() => (showCreateGroup = false)}>
    <div class="me-pop mid">
      <div class="me-head"><b>➕ 创建群聊</b><span class="dim">创建者为群主兼管理员 · 名单自动同步全员</span><span style="flex:1"></span><button type="button" class="mini me-close" on:click={() => (showCreateGroup = false)}>✕</button></div>
      <div class="fld fld-top">
        <span>群名</span>
        <input type="text" bind:value={gNameDraft} maxlength="24" placeholder="例如：周末爬山小队" />
      </div>
      <div class="bcast-list">
        {#each sortedPeers.filter((p) => p.online) as p (p.uid)}
          <label class="brow">
            <input type="checkbox" checked={gSel.includes(p.uid)} on:change={() => toggleGSel(p.uid)} />
            <span class="pav online" style="width:22px;height:22px;font-size:12px;background:{AV_BG[p.avatar] ?? '#64748b'}">{AV_EMOJI[p.avatar] ?? "🙂"}</span>
            <b>{dispName(p)}</b>
            <span class="dim">{p.host}</span>
          </label>
        {/each}
        {#if !(st?.peers ?? []).some((p) => p.online)}
          <div class="dim pempty">没有在线联系人 —— 先「雷达」发现同伴</div>
        {/if}
      </div>
      <div class="pop-actions">
        <span class="dim">已选 {gSel.length} 人</span>
        <span style="flex:1"></span>
        <button type="button" class="mini" on:click={() => (showCreateGroup = false)}>取消</button>
        <button type="button" class="mini ok" on:click={doCreateGroup}>创建</button>
      </div>
    </div>
  </div>
{/if}

<!-- 群管理弹窗 -->
{#if showGroupMgr && target?.kind === "group"}
  {@const g = target.group}
  <div class="me-mask" role="presentation" on:click|self={() => (showGroupMgr = false)}>
    <div class="me-pop mid">
      <div class="me-head"><b>👥 群管理 · {g.name}</b><span class="dim">{g.members.length} 人</span><span style="flex:1"></span><button type="button" class="mini me-close" on:click={() => (showGroupMgr = false)}>✕</button></div>
      <div class="fld fld-top">
        <span>群名</span>
        <input type="text" bind:value={gMgrRename} maxlength="24" disabled={!amAdmin(g)} />
        {#if amAdmin(g)}<button type="button" class="mini ok" on:click={() => gAction("rename")}>改名</button>{/if}
      </div>
      {#if amAdmin(g)}
        <div class="fld fld-top">
          <span>邀请</span>
          <div class="inv-row">
            {#each sortedPeers.filter((p) => p.online && !g.members.some((m) => m.uid === p.uid)) as p (p.uid)}
              <button type="button" class="mini" class:sel-inv={gMgrInviteSel.includes(p.uid)}
                on:click={() => toggleInviteSel(p.uid)}>{dispName(p)}</button>
            {/each}
          </div>
          {#if gMgrInviteSel.length}
            <button type="button" class="mini ok" on:click={() => gAction("invite")}>邀请（{gMgrInviteSel.length}）</button>
          {/if}
        </div>
      {/if}
      <div class="bcast-list member-list">
        {#each g.members as m (m.uid)}
          <div class="brow">
            <span class="pav" style="width:24px;height:24px;font-size:13px;background:{AV_BG[m.avatar] ?? '#64748b'}">{AV_EMOJI[m.avatar] ?? "🙂"}</span>
            <b>{m.nickname}</b>
            {#if m.uid === g.owner}<span class="chip role">👑 群主</span>
            {:else if (g.admins ?? []).includes(m.uid)}<span class="chip role adm">🛡 管理员</span>{/if}
            {#if m.uid === st?.uid}<span class="dim">（我）</span>{/if}
            <span style="flex:1"></span>
            {#if amOwner(g) && m.uid !== g.owner}
              <button type="button" class="mini" on:click={() => gSetAdmin(g, m.uid, !(g.admins ?? []).includes(m.uid))}>
                {(g.admins ?? []).includes(m.uid) ? "撤销管理" : "任命管理"}
              </button>
              <button type="button" class="mini bad" on:click={() => gAction("kick", m.uid)}>移出</button>
            {/if}
          </div>
        {/each}
      </div>
      <div class="pop-actions">
        {#if amAdmin(g)}
          <button type="button" class="mini" on:click={() => gAction("nudge")}>📣 一键提醒</button>
        {/if}
        <span style="flex:1"></span>
        {#if amOwner(g)}
          <button type="button" class="mini bad" on:click={() => gAction("dissolve")}>解散群聊</button>
        {:else}
          <button type="button" class="mini bad" on:click={() => gAction("leave")}>退出群聊</button>
        {/if}
      </div>
    </div>
  </div>
{/if}

<!-- 聊天记录查看器（消息/文件 + 搜索） -->
{#if showHist}
  <div class="me-mask" role="presentation" on:click|self={() => (showHist = false)}>
    <div class="me-pop hist">
      <div class="me-head">
        <b>📜 聊天记录 · {chatTitle}</b>
        <span style="flex:1"></span>
        <button type="button" class="mini me-close" on:click={() => (showHist = false)}>✕</button>
      </div>
      <div class="ih-head">
        <div class="iseg">
          <button type="button" class:on={histTab === "msg"} on:click={() => (histTab = "msg")}>消息</button>
          <button type="button" class:on={histTab === "file"} on:click={() => (histTab = "file")}>文件</button>
        </div>
        <span style="flex:1"></span>
        <div class="ih-act">
          <button type="button" class="ih-btn" title="清除本会话接收的文件（记录与托管目录内的文件一并删除）" on:click={clearChatFiles}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2m2 0v14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V6"/></svg> 清除接收文件
          </button>
          <button type="button" class="ih-btn" title="清空本会话的全部聊天记录（仅本机）" on:click={clearCurrentHistory}>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M3 6h18M8 6V4a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v2m2 0v14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2V6M10 11v6M14 11v6"/></svg> 清空记录
          </button>
        </div>
        <div class="isearch">
          <span>🔍</span>
          <input type="text" placeholder="搜索消息 / 文件名 / 发送者…" bind:value={histQ} />
          {#if histQ}<button type="button" title="清空" on:click={() => (histQ = "")}>✕</button>{/if}
        </div>
      </div>
      {#if histTab === "msg"}
        <div class="plist hist-list" bind:this={histMsgsEl}>
          {#each histSearched as m, i (i)}
            <div class="hrow" class:out={m.d === "out"}>
              <span class="htime">{new Date(m.ts).toLocaleString("zh-CN", { hour12: false })}</span>
              {#if m.file}
                <div class="ifile" class:clickable={!!m.path} role="button" tabindex="0" title={m.path ? `${m.path}（双击打开 · 右键转发）` : m.file} on:dblclick={() => m.path && openXferFile(m.path)} on:keydown={(e) => e.key === "Enter" && m.path && openXferFile(m.path)} on:contextmenu={(e) => m.path && (e.preventDefault(), fwdFile({ path: m.path, file: m.file, size: m.size }))}>
                  <span class="iico doc"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><path d="M13 2v7h7"/></svg></span>
                  <span class="imeta"><b>{m.file}</b><span>{fmtSize(m.size)} · {m.d === "out" ? "发出" : "收到"}{m.path ? " · 双击打开" : ""}</span></span>
                </div>
              {:else}
                <!-- svelte-ignore a11y-no-static-element-interactions -->
                <div class="ibub" on:contextmenu={(e) => openMsgCtx(e, m)}>
                  {#if target?.kind === "group" && m.d !== "out"}<span class="iwho">{m.from_name || "成员"}</span>{/if}{m.text}
                </div>
              {/if}
            </div>
          {:else}
            <div class="iempty">{histQ ? "无匹配记录" : "暂无记录"}</div>
          {/each}
        </div>
      {:else}
        <div class="plist hist-list">
          {#each fileRows as f, i (i)}
            <div class="hrow" class:out={f.d === "out"}>
              <span class="htime">{new Date(f.ts).toLocaleString("zh-CN", { hour12: false })}</span>
              <!-- svelte-ignore a11y-no-static-element-interactions -->
              <div class="ifile" class:clickable={!!f.path} title={f.path ? "右键转发" : f.name} on:contextmenu={(e) => f.path && (e.preventDefault(), fwdFile({ path: f.path, file: f.name, size: f.size }))}>
                <span class="iico doc"><svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M13 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z"/><path d="M13 2v7h7"/></svg></span>
                <span class="imeta"><b>{f.name}</b><span>{fmtSize(f.size)} · {f.d === "out" ? "发出" : "收到"}</span></span>
              </div>
            </div>
          {:else}
            <div class="iempty">{histQ ? "无匹配文件" : "暂无文件记录"}</div>
          {/each}
        </div>
      {/if}
    </div>
  </div>
{/if}

<svelte:window on:focus={queueMarkRead} on:keydown={(e) => { if (e.key === "Escape") { showMe = false; showBcast = false; showCreateGroup = false; showGroupMgr = false; showHist = false; showSettings = false; remarkFor = null; peerInfo = null; fileCtx = null; msgCtx = null; fwd = null; } }} />

<style>
  .svc-head { display: flex; align-items: center; gap: 10px; padding: 10px 14px; }
  .dot { width: 10px; height: 10px; border-radius: 50%; background: #94a3b8; flex: none; }
  .dot.on { background: var(--ok); box-shadow: 0 0 8px rgba(16, 185, 129, 0.7); }
  .dot.wait { background: var(--accent); animation: pulse 1s ease-in-out infinite; }
  @keyframes pulse { 50% { opacity: 0.35; } }
  .chip { font-size: 11.5px; color: var(--text-dim); border: 1px solid var(--stroke-soft); border-radius: 999px; padding: 3px 10px; flex: none; }
  .chip.fw-ok { color: var(--ok); border-color: rgba(16, 185, 129, 0.4); }
  .chip.fw-fix {
    color: #b45309; border-color: rgba(217, 119, 6, 0.55); background: rgba(217, 119, 6, 0.08);
    cursor: pointer; font-weight: 600; font-size: 11.5px;
  }
  .chip.fw-fix:hover { background: rgba(217, 119, 6, 0.16); }
  .chip.gchip { color: var(--text-dim); }
  .swap-btn { flex: none; font-size: 13px; padding: 4px 9px; }
  .me-warn { font-size: 11px; line-height: 1.6; color: #b45309; background: rgba(217, 119, 6, 0.08); border-radius: 9px; padding: 8px 10px; }
  /* v0.9.9 设置按钮：与右下角资料徽章同一表面语言（圆形 + 同族描边阴影 + 悬停上浮） */
  .gear-btn {
    flex: none; width: 28px; height: 28px; padding: 0; font-size: 13.5px;
    border-radius: 50%; display: inline-flex; align-items: center; justify-content: center;
    background: var(--panel); border: 1px solid var(--stroke-soft);
    box-shadow: 0 2px 8px rgba(15, 23, 42, 0.18);
    transition: transform 0.15s, box-shadow 0.15s;
  }
  .gear-btn:hover { transform: translateY(-1px); box-shadow: 0 4px 14px rgba(15, 23, 42, 0.24); }
  .small { font-size: 10px; }
  /* 两栏 IM */
  .im-wrap { flex: 1; min-height: 0; display: flex; gap: 10px; align-items: stretch; }
  .im-wrap.flipped { flex-direction: row-reverse; }
  .chat-card { flex: 1; min-width: 0; display: flex; flex-direction: column; overflow: hidden; position: relative; }
  .drop-veil {
    position: absolute; inset: 0; z-index: 20; display: flex; align-items: center; justify-content: center;
    font-size: 15px; font-weight: 700; color: var(--accent);
    background: color-mix(in srgb, var(--accent-bg) 82%, transparent);
    border: 2px dashed var(--accent); border-radius: 12px; pointer-events: none;
  }
  .contacts { width: 300px; flex: none; display: flex; flex-direction: column; overflow: hidden; position: relative; }
  /* v0.9.8 联系人栏拖拽把手：与导航把手同语言（悬停浮现竖向握持点） */
  .plist-grip {
    position: absolute; top: 6px; bottom: 6px; left: -5px; width: 9px;
    cursor: col-resize; z-index: 6;
  }
  .plist-grip::after {
    content: ""; position: absolute; left: 2.5px; top: 50%; transform: translateY(-50%);
    width: 3px; height: 44px; border-radius: 2px; background: var(--stroke);
    opacity: 0; transition: opacity 0.15s;
  }
  .plist-grip:hover::after { opacity: 1; }
  .im-wrap.flipped .plist-grip { left: auto; right: -5px; }
  .im-wrap.flipped .plist-grip::after { left: auto; right: 2.5px; }
  .sec-head { display: flex; align-items: center; gap: 6px; padding: 11px 12px 9px; border-bottom: 1px solid var(--stroke-soft); flex: none; }
  /* v0.9.3 三枚功能 pill：同一形态语言（圆角胶囊 + 微底色 + 同构图标位），异色区分职能 */
  .radar-btn, .pill {
    position: relative; overflow: hidden; display: inline-flex; align-items: center; gap: 5px;
    padding: 5px 11px; border-radius: 999px; border: 1px solid var(--accent);
    background: var(--accent-bg); color: var(--accent); font-size: 12px; font-weight: 600; cursor: pointer; flex: none;
    transition: filter 0.15s, transform 0.1s;
  }
  .pill .pi { font-size: 11.5px; line-height: 1; }
  .pill:hover, .radar-btn:hover { filter: brightness(1.08); }
  .pill:active { transform: scale(0.96); }
  .pill-bcast { border-color: rgba(245, 158, 11, 0.65); background: rgba(245, 158, 11, 0.13); color: #d97706; }
  .pill-group { border-color: rgba(59, 130, 246, 0.6); background: rgba(59, 130, 246, 0.12); color: #2563eb; }
  /* v0.9.10 头部去标题后：广播/建群 收为图标 pill，与齿轮/调换同视觉密度 */
  .sec-head .pill { height: 28px; padding: 0 11px; gap: 5px; font-size: 12px; font-weight: 650; }
  .sec-head .pill .pi { font-size: 13px; }
  .radar-btn { min-width: 76px; justify-content: center; } /* v0.9.17 固定宽度：文案切换不推挤相邻按钮 */
  .radar-btn:disabled { opacity: 0.6; cursor: wait; }
  .sweep { width: 11px; height: 11px; border-radius: 50%; border: 2px solid currentColor; position: relative; flex: none; }
  .sweep::before {
    content: ""; position: absolute; inset: -2px; border-radius: 50%;
    background: conic-gradient(currentColor 0 40deg, transparent 40deg 360deg);
    animation: spin 1.6s linear infinite; opacity: 0;
  }
  .radar-btn.spin .sweep::before { opacity: 0.9; }
  @keyframes spin { to { transform: rotate(360deg); } }
  .side-tabs { display: flex; gap: 4px; padding: 8px 10px 6px; border-bottom: 1px solid var(--stroke-soft); flex: none; }
  .side-tabs button {
    flex: 1; padding: 6px 0; border-radius: 8px; cursor: pointer; font-size: 12px;
    border: 1px solid transparent; background: transparent; color: var(--text-dim);
  }
  .side-tabs button.on { border-color: var(--accent); color: var(--accent); font-weight: 600; background: var(--accent-bg); }
  .plist { flex: 1; overflow-y: auto; min-height: 0; }
  .prow {
    display: flex; align-items: center; gap: 9px; padding: 9px 10px; cursor: pointer;
    transition: background 0.12s;
  }
  .prow + .prow { border-top: 1px solid var(--stroke-soft); }
  .prow:hover { background: var(--accent-bg); }
  .prow.active { background: var(--accent-bg); box-shadow: inset 3px 0 0 var(--accent); }
  .prow.offline { opacity: 0.55; }
  .pav {
    width: 36px; height: 36px; border-radius: 50%; flex: none;
    display: flex; align-items: center; justify-content: center; font-size: 18px; position: relative;
  }
  .pav.gav { background: rgba(59, 130, 246, 0.15); }
  .pav.online::after {
    content: ""; position: absolute; right: -1px; bottom: -1px; width: 9px; height: 9px;
    border-radius: 50%; background: var(--ok); border: 2px solid var(--panel);
  }
  .pav.big { width: 30px; height: 30px; font-size: 15px; }
  .pmid { flex: 1; min-width: 0; }
  .pline1 { display: flex; align-items: baseline; gap: 6px; min-width: 0; }
  .pline1 b { font-size: 13px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .star { color: #d97706; font-size: 11px; flex: none; }
  .ptime { margin-left: auto; font-size: 10.5px; color: var(--text-dim); flex: none; }
  .pline2 {
    font-size: 11.5px; color: var(--text-dim); margin-top: 2px;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .badge-u {
    background: var(--accent); color: #fff; border-radius: 999px; font-size: 10.5px;
    min-width: 18px; height: 18px; display: flex; align-items: center; justify-content: center;
    padding: 0 5px; flex: none; font-weight: 700;
  }
  .fav-btn {
    border: none; background: transparent; cursor: pointer; font-size: 13px; flex: none;
    color: #94a3b8; padding: 4px; border-radius: 7px; line-height: 1;
  }
  .fav-btn:hover { background: rgba(217, 119, 6, 0.12); color: #d97706; }
  .fav-btn.rm { color: #64748b; opacity: 0; transition: opacity 0.15s; margin-left: -2px; }
  .prow:hover .fav-btn.rm { opacity: 0.85; }
  .fav-btn.rm:hover { background: rgba(239, 68, 68, 0.12); color: #ef4444; opacity: 1; }
  .pempty { padding: 36px 18px; text-align: center; font-size: 12px; }
  .pnote { padding: 9px 14px; border-top: 1px dashed var(--stroke-soft); font-size: 11px; flex: none; }
  /* 聊天面板 */
  .chat-head { display: flex; align-items: center; gap: 10px; padding: 10px 14px; border-bottom: 1px solid var(--stroke-soft); flex: none; flex-wrap: wrap; }
  .ch-info { min-width: 0; font-size: 13px; }
  .ch-info .dim { font-size: 11px; }
  .fp { font-size: 11px; color: var(--ok); border: 1px solid rgba(16, 185, 129, 0.4); border-radius: 999px; padding: 3px 9px; flex: none; }
  .sess-wait { font-size: 11.5px; color: #b45309; border: 1px solid rgba(217, 119, 6, 0.5); border-radius: 999px; padding: 3px 9px; flex: none; }
  .sess-bad { font-size: 11.5px; color: #b91c1c; border: 1px solid rgba(185, 28, 28, 0.45); border-radius: 999px; padding: 3px 9px; flex: none; cursor: help; }
  .me-close { flex: none; font-size: 12px; padding: 3px 8px; }
  .msgs { flex: 1; overflow-y: auto; padding: 14px; display: flex; flex-direction: column; gap: 8px; min-height: 0; }
  .mrow { display: flex; align-items: center; }
  .mrow.out { justify-content: flex-end; }
  /* v0.9.21 已读标记（iMessage 式小字，仅最后一条发出的消息） */
  .read-tag { flex: none; margin: 0 2px 0 6px; font-size: 10px; color: #8e8e93; user-select: none; }
  .gwrap { max-width: 100%; }
  .gsender { font-size: 10.5px; color: var(--text-dim); margin: 0 0 2px 4px; }
  .bub {
    max-width: 76%; padding: 8px 12px; border-radius: 14px; font-size: var(--chat-fs, 13px); line-height: 1.55;
    background: #fff; border: 1px solid var(--stroke-soft); color: var(--text);
    word-break: break-word; white-space: pre-wrap; box-shadow: 0 1px 2px rgba(15, 23, 42, 0.06);
  }
  .mrow.out .bub { background: var(--accent-bg); border-color: color-mix(in srgb, var(--accent) 35%, transparent); }
  .fcard { display: inline-flex; align-items: center; gap: 10px; padding: 10px 14px; }
  .fico { font-size: 22px; flex: none; }
  /* v0.9.14 文件卡文档徽标（替代回形针，Telegram 式文档图形） */
  .fcard .fico.doc {
    width: 34px; height: 34px; border-radius: 9px; flex: none;
    background: rgba(0, 122, 255, 0.1); color: #007aff;
    display: flex; align-items: center; justify-content: center;
  }
  .ifile .iico.doc { background: rgba(0, 122, 255, 0.1); color: #007aff; }
  .fmeta { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .fmeta b { font-size: 12.5px; max-width: 240px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .fmeta .dim { font-size: 10.5px; }
  .empty-msg { text-align: center; margin: auto; font-size: 12px; }
  .chat-empty {
    flex: 1; display: flex; flex-direction: column; align-items: center; justify-content: center;
    gap: 8px; text-align: center; padding: 20px;
  }
  .ce-icon { font-size: 40px; opacity: 0.5; }
  .chat-empty b { font-size: 14px; }
  .chat-empty .dim { font-size: 11.5px; line-height: 1.7; }
  /* 文件传输条 */
  .xfer-strip { border-top: 1px solid var(--stroke-soft); padding: 6px 10px; display: flex; flex-direction: column; gap: 6px; max-height: 180px; overflow-y: auto; flex: none; }
  .xcard { display: flex; align-items: center; gap: 10px; padding: 6px 10px; border: 1px solid var(--stroke-soft); border-radius: 10px; background: var(--panel); }
  .xcard .fico { font-size: 18px; }
  .xcard .fmeta { flex: 1; min-width: 0; }
  .xbar { display: block; height: 5px; border-radius: 999px; background: rgba(100, 116, 139, 0.2); overflow: hidden; margin: 3px 0; }
  .xbar-in { display: block; height: 100%; background: var(--accent); border-radius: 999px; transition: width 0.25s; }
  .xbar-in.paused { background: #94a3b8; }
  .mini.ok { color: var(--ok); border-color: rgba(16, 185, 129, 0.45); }
  .mini.bad { color: #b91c1c; border-color: rgba(185, 28, 28, 0.4); }
  /* v0.9.3 传输卡按钮 iOS 化：接收=蓝色实心胶囊 / 拒绝、暂停、取消=中性描边胶囊 */
  .xcard .mini { border-radius: 999px; background: #fff; border: 1px solid #d1d1d6; color: #3a3a3c; font-weight: 600; padding: 4px 12px; }
  .xcard .mini:hover { background: #f2f2f7; }
  .xcard .mini.ok { background: #007aff; border-color: #007aff; color: #fff; }
  .xcard .mini.ok:hover { background: #0a84ff; }
  .xcard .mini.bad { color: #ff3b30; border-color: rgba(255, 59, 48, 0.45); background: #fff; }
  .xcard .mini.bad:hover { background: rgba(255, 59, 48, 0.08); }
  .xcard.xdone { opacity: 0.75; }
  .xdone-tick { color: var(--ok); font-weight: 800; font-size: 14px; flex: none; }
  .fcard.clickable { cursor: pointer; }
  .fcard.clickable:hover { border-color: var(--accent); }
  .ifile.clickable { cursor: pointer; }
  .ifile.clickable:hover { box-shadow: 0 2px 8px rgba(0, 122, 255, 0.25); }
  /* 群工具栏 */
  .adm-bar { display: flex; align-items: center; gap: 8px; padding: 6px 14px; border-top: 1px dashed var(--stroke-soft); flex: none; font-size: 11.5px; }
  /* 表情选择器 */
  .emoji-bar {
    display: none; flex-direction: column; gap: 4px; padding: 6px 10px;
    border-top: 1px solid var(--stroke-soft); max-height: 240px; overflow-y: auto; flex: none;
  }
  .emoji-bar.open { display: flex; }
  .etabs { display: flex; gap: 4px; align-items: center; flex-wrap: wrap; }
  .etabs button {
    border: 1px solid transparent; background: transparent; color: var(--text-dim);
    font-size: 11.5px; cursor: pointer; border-radius: 7px; padding: 3px 8px;
  }
  .etabs button.on { border-color: var(--accent); color: var(--accent); background: var(--accent-bg); }
  .esearch {
    flex: 1; min-width: 90px; max-width: 150px; margin-left: auto; padding: 4px 8px; border-radius: 7px;
    border: 1px solid var(--stroke); background: var(--panel); color: var(--text); font-size: 11.5px;
  }
  .egrid { display: grid; grid-template-columns: repeat(auto-fill, minmax(34px, 1fr)); gap: 2px; }
  .egrid button { border: none; background: transparent; font-size: 21px; cursor: pointer; border-radius: 7px; padding: 4px 2px; line-height: 1.2; }
  .egrid button:hover { background: var(--accent-bg); }
  .es-empty { grid-column: 1 / -1; text-align: center; font-size: 11.5px; padding: 14px 0; }
  .chat-foot { display: flex; gap: 8px; padding: 10px 12px; border-top: 1px solid var(--stroke-soft); align-items: flex-end; flex: none; }
  .emoji-toggle { font-size: 16px; }
  /* v0.9.4 多行内容编辑框：桌面 IM 常规高度（40px 起步），随内容长高至 ≈6 行 */
  .chat-foot textarea {
    flex: 1; min-width: 0; min-height: 40px; max-height: 300px;
    padding: 9px 13px; border-radius: 12px; resize: none; outline: none;
    border: 1px solid var(--stroke); background: var(--panel); color: var(--text);
    font: inherit; font-size: 13px; line-height: 1.5;
    transition: border-color 0.15s, box-shadow 0.15s;
  }
  .chat-foot textarea:focus { border-color: var(--accent); box-shadow: 0 0 0 2px var(--accent-bg); }
  .chat-foot textarea::placeholder { color: var(--text-dim); }
  .send {
    padding: 9px 18px; border-radius: 10px; border: none; cursor: pointer;
    background: var(--accent); color: #fff; font-size: 13px; font-weight: 600;
  }
  .send:disabled { opacity: 0.6; cursor: wait; }
  /* 弹窗（档案/备注/广播/建群/群管理/记录） */
  .me-mask { position: fixed; inset: 0; z-index: 60; background: rgba(2, 6, 23, 0.52); animation: maskIn 0.2s ease; }
  @keyframes maskIn { from { opacity: 0; } }
  @keyframes popIn { from { opacity: 0; transform: scale(0.96) translateY(10px); } to { opacity: 1; transform: none; } }
  @keyframes popInMid { from { opacity: 0; transform: translateX(-50%) scale(0.96); } to { opacity: 1; transform: translateX(-50%) scale(1); } }
  /* iOS 统一浅色弹窗系统（v0.9.2）：#f2f2f7 面板 + #e3e3e8 填充 + #007aff 主色 */
  .me-pop {
    position: fixed; right: 18px; bottom: 64px; width: 340px; z-index: 61;
    background: #f2f2f7; color: #1c1c1e; border: none; border-radius: 18px;
    box-shadow: 0 24px 70px rgba(0, 0, 0, 0.35); padding: 14px 0 16px; max-height: 78vh; overflow-y: auto;
    animation: popIn 0.24s cubic-bezier(0.32, 0.72, 0, 1);
  }
  .me-pop.small { width: 320px; }
  .me-pop.mid { width: 430px; left: 50%; right: auto; transform: translateX(-50%); top: 8vh; bottom: auto; max-height: 80vh; animation: popInMid 0.24s cubic-bezier(0.32, 0.72, 0, 1); }
  .me-pop.hist { width: min(640px, 92vw); left: 50%; right: auto; transform: translateX(-50%); top: 7vh; bottom: auto; max-height: 78vh; animation: popInMid 0.24s cubic-bezier(0.32, 0.72, 0, 1); }
  .me-head { display: flex; align-items: center; gap: 8px; padding: 10px 16px 12px; border-bottom: 1px solid rgba(60, 60, 67, 0.1); }
  .me-head b { font-size: 16px; font-weight: 700; color: #1c1c1e; letter-spacing: 0.2px; }
  .me-head .dim { font-size: 11.5px; color: #8e8e93; }
  .me-close { color: #8e8e93; border-color: #d1d1d6; background: #ffffff; }
  .me-close:hover { color: #ff3b30; border-color: #ff3b30; }
  .fld-top { padding: 4px 16px 2px; }
  .set-actions { display: flex; gap: 8px; padding: 8px 16px 0; }
  .chat-dir {
    flex: 1; min-width: 0; font-size: 11.5px; color: #3a3a3c; background: #e3e3e8;
    border-radius: 9px; padding: 7px 10px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
    direction: rtl; text-align: left; /* 长路径显示末段 */
  }
  /* v0.9.5 字号设置：iOS 滑杆 + 跟随式实时预览浮窗 */
  .font-row { flex: 1; position: relative; display: flex; align-items: center; padding: 26px 10px 6px; background: #fff; border-radius: 12px; }
  .font-row input[type="range"] { flex: 1; accent-color: #007aff; height: 22px; cursor: pointer; }
  .font-pv {
    position: absolute; bottom: calc(100% - 2px); transform: translateX(-50%);
    transition: left 0.08s linear; pointer-events: none; z-index: 5;
  }
  .pv-card {
    background: #ffffff; border-radius: 13px; padding: 8px 12px;
    box-shadow: 0 6px 22px rgba(15, 23, 42, 0.22), 0 0 0 1px rgba(15, 23, 42, 0.06);
    display: flex; flex-direction: column; gap: 5px; min-width: 128px;
  }
  .pv-in { background: #e9e9eb; color: #000; border-radius: 12px 12px 12px 4px; padding: 4px 9px; line-height: 1.45; align-self: flex-start; }
  .pv-out { background: linear-gradient(180deg, #2f8bff, #007aff); color: #fff; border-radius: 12px 12px 4px 12px; padding: 4px 9px; line-height: 1.45; align-self: flex-end; }
  .pv-arrow {
    width: 10px; height: 10px; background: #fff; margin: -5px auto 0; transform: rotate(45deg);
    box-shadow: 3px 3px 6px rgba(15, 23, 42, 0.08); border-radius: 2px;
  }
  /* v0.9.9 输入框手动高度：拖拽期间放开 CSS 高度上限 */
  .chat-foot textarea.manual { max-height: none; }
  /* v0.9.16 抖一抖 */
  .chat-foot .mini.shake-btn { color: var(--text-dim); transition: color 0.15s, transform 0.15s; }
  .chat-foot .mini.shake-btn:hover { color: #ff9500; transform: translateY(-1px); }
  .chat-foot .mini.shake-btn:active { transform: scale(0.92); }
  @keyframes chatshake {
    0%, 100% { transform: translateX(0); }
    15% { transform: translateX(-8px); }
    30% { transform: translateX(7px); }
    45% { transform: translateX(-5px); }
    60% { transform: translateX(4px); }
    75% { transform: translateX(-2px); }
    90% { transform: translateX(1px); }
  }
  :global(.msgs.shaking) { animation: chatshake 0.6s cubic-bezier(0.36, 0.07, 0.19, 0.97); }
  /* v0.9.9 输入框高度把手：横向握持条（悬停浮现），位于操作条上缘 */
  /* v0.9.18 聊天头部按钮 iOS 胶囊统一：等高等宽节奏，收起补 chevron 图标 */
  .chat-head .mini {
    display: inline-flex; align-items: center; gap: 5px; height: 26px; padding: 0 11px;
    border-radius: 999px; border: 1px solid var(--stroke); background: var(--panel);
    color: var(--text-dim); font-size: 12px; font-weight: 600; cursor: pointer; flex: none;
    transition: color 0.15s, border-color 0.15s, background 0.15s, transform 0.1s;
  }
  .chat-head .mini:hover { color: var(--text); border-color: var(--accent); background: var(--accent-bg); }
  .chat-head .mini:active { transform: scale(0.95); }
  .chat-head .mini:has(svg) svg { flex: none; }
  /* v0.9.10 表情/附件按钮：统一 38px 圆形、同视觉重量，更易点击 */
  .chat-foot .mini {
    width: 38px; height: 38px; padding: 0; flex: none;
    border-radius: 50%; display: inline-flex; align-items: center; justify-content: center;
  }
  .chat-foot .mini.emoji-toggle { font-size: 19px; }
  .chat-foot .file-btn svg { width: 18px; height: 18px; }
  .chat-foot { position: relative; }
  .foot-grip {
    position: absolute; top: -5px; left: 12px; right: 12px; height: 10px;
    cursor: row-resize; z-index: 5;
  }
  .foot-grip::after {
    content: ""; position: absolute; left: 50%; top: 50%; transform: translate(-50%, -50%);
    width: 46px; height: 3.5px; border-radius: 999px; background: var(--stroke);
    opacity: 0; transition: opacity 0.15s, width 0.15s;
  }
  .foot-grip:hover::after { opacity: 1; width: 62px; }
  /* v0.9.9 附件按钮：SF 风格回形针 SVG，与 😀 按钮同视觉重量 */
  .file-btn { display: inline-flex; align-items: center; justify-content: center; color: var(--text-dim); }
  .file-btn:hover { color: var(--text); }
  /* v0.9.9 头像性别徽章（联系人列表）：iOS 圆形小徽标，右下角切边悬浮 */
  .pav-wrap { position: relative; display: inline-flex; flex: none; }
  .gbadge {
    position: absolute; right: -4px; bottom: -3px; width: 14px; height: 14px;
    border-radius: 50%; display: flex; align-items: center; justify-content: center;
    font-size: 9.5px; font-weight: 700; color: #fff; line-height: 1;
    border: 1.5px solid rgba(255, 255, 255, 0.92); box-shadow: 0 1px 3px rgba(15, 23, 42, 0.25);
    pointer-events: none;
  }
  .gbadge.gm { background: #007aff; }
  .gbadge.gf { background: #ff2d55; }
  /* v0.9.9 联系人资料卡 */
  .info-top { display: flex; align-items: center; gap: 12px; padding: 4px 16px 12px; }
  .info-id { display: flex; flex-direction: column; gap: 3px; min-width: 0; }
  .info-id b { font-size: 14.5px; }
  .info-id .dim { font-size: 12px; }
  .info-id .chip { font-size: 10.5px; padding: 2px 8px; }
  .info-rows {
    display: flex; flex-direction: column; margin: 0 12px 12px; background: #ffffff;
    border-radius: 12px; overflow: hidden;
  }
  .info-rows .irow {
    display: flex; align-items: center; gap: 10px; padding: 9px 12px; font-size: 12.5px;
  }
  .info-rows .irow + .irow { border-top: 1px solid #f2f2f7; }
  .info-rows .irow .dim { width: 52px; flex: none; font-size: 12px; }
  .info-rows .irow b { font-weight: 600; word-break: break-all; }
  .info-rows .mono { font-family: ui-monospace, "Cascadia Mono", Consolas, monospace; font-size: 11.5px; letter-spacing: 0.4px; }
  /* v0.9.11 数据管理卡 */
  /* ===== v0.9.25 设置面板：iOS 分组列表 =====
     灰底分组白卡 + 发丝分隔线 + 标签卡内 64px 左对齐 —— 标签、内容、按钮同一直线体系 */
  .sg { margin: 10px 14px 0; background: #ffffff; border-radius: 13px; padding: 1px 13px; }
  .sg-row { display: flex; align-items: center; gap: 10px; padding: 10px 0; border-bottom: 1px solid #f2f2f7; }
  .sg-row:last-child { border-bottom: none; }
  .sg-col { flex-direction: column; align-items: stretch; gap: 8px; }
  .sg-line { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .sg-lab { flex: none; width: 64px; font-size: 12px; color: #8e8e93; font-weight: 600; }
  .sg-line input[type="text"] { flex: 1; min-width: 0; padding: 8px 11px; border-radius: 10px; border: 1px solid #e3e3e8; background: #ffffff; color: #1c1c1e; font-size: 13px; transition: border-color 0.15s, box-shadow 0.15s; }
  .sg-line input[type="text"]:focus { outline: none; border-color: #007aff; box-shadow: 0 0 0 3px rgba(0, 122, 255, 0.14); }
  .sg-btns { display: flex; gap: 8px; flex-wrap: wrap; }
  .sg-note { font-size: 11px; line-height: 1.6; }
  .sg-text { flex: 1; display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .sg-text b { font-size: 12.5px; color: #1c1c1e; }
  .sg-text .dim { font-size: 10.5px; color: #8e8e93; line-height: 1.5; }
  .sg-data { font-size: 12px; flex: 1; min-width: 0; }
  .sg-data b { color: #1c1c1e; font-weight: 650; }
  .sg-data .sep { margin: 0 4px; color: #c7c7cc; }
  .sg .ios-sw { width: 40px; height: 24px; flex: none; accent-color: #34c759; }
  .sg .mini { flex: none; }
  .fld input { background: #e3e3e8; border: none; color: #1c1c1e; border-radius: 10px; }
  .fld input::placeholder { color: #8e8e93; }
  .me-pop .mini { background: #ffffff; border: 1px solid #d1d1d6; color: #1c1c1e; font-weight: 600; }
  .me-pop .mini.ok { background: #007aff; border-color: #007aff; color: #fff; }
  .me-pop .mini.ok:hover { background: #0a84ff; }
  .me-pop .mini:hover { background: #f7f7fa; }
  .me-pop .mini.ok { background: #007aff; border-color: #007aff; color: #fff; }
  .me-pop .mini.ok:hover { background: #0a84ff; }
  .me-pop .mini.bad { color: #ff3b30; border-color: rgba(255, 59, 48, 0.4); }
  .me-pop .mini.bad:hover { background: rgba(255, 59, 48, 0.08); }
  .me-pop .seg button { background: #e3e3e8; border: none; color: #3a3a3c; }
  .me-pop .seg button.sel { background: #ffffff; color: #007aff; box-shadow: 0 1px 3px rgba(0, 0, 0, 0.12); }
  .me-pop .bcast-list { border-top: 1px solid #e5e5ea; }
  .me-pop .brow + .brow { border-top: 1px solid #f2f2f7; }
  .me-pop .brow .dim { color: #8e8e93; }
  .me-pop .brow input[type="checkbox"] { accent-color: #007aff; }
  .me-pop .pop-actions .dim { color: #8e8e93; }
  .me-pop .ios-sw { accent-color: #34c759; }
  .me-pop .lay-lab { color: #1c1c1e; }
  .me-pop .me-warn { color: #b25000; background: rgba(255, 149, 0, 0.12); }
  .me-pop .me-fp { color: #8e8e93; }
  .me-pop .me-fp .mono { color: #007aff; }
  .me-pop .inv-row .mini { background: #ffffff; }
  .me-pop .mini.sel-inv { background: #eaf3ff; border-color: #007aff; color: #007aff; }
  .me-pop ::-webkit-scrollbar { width: 8px; }
  .me-pop ::-webkit-scrollbar-thumb { background: #c7c7cc; border-radius: 999px; border: 2px solid #f2f2f7; }
  .me-pop ::-webkit-scrollbar-track { background: transparent; }
  .av-grid { display: grid; grid-template-columns: repeat(10, 1fr); gap: 4px; padding: 0 14px 10px; }
  .av {
    aspect-ratio: 1; border-radius: 10px; border: 2px solid transparent; cursor: pointer;
    display: flex; align-items: center; justify-content: center; font-size: 15px;
    filter: saturate(0.75); transition: filter 0.15s, border-color 0.15s;
  }
  .av:hover { filter: saturate(1); }
  .av.sel { border-color: #0f172a; filter: saturate(1); }
  .fld { display: flex; align-items: center; gap: 8px; }
  /* v0.9.17 弹层表单统一 iOS 浅色控件：不再随应用主题变量抖动 */
  .me-pop .fld > span { font-size: 12px; color: #8e8e93; font-weight: 600; flex: none; min-width: 72px; } /* v0.9.24 72px：容纳 6 字标签，全部行左缘对齐 */
  .me-pop .fld input {
    flex: 1; min-width: 0; padding: 8px 11px; border-radius: 10px;
    border: 1px solid #e3e3e8; background: #ffffff; color: #1c1c1e; font-size: 13px;
    transition: border-color 0.15s, box-shadow 0.15s;
  }
  .me-pop .fld input:focus { border-color: #007aff; box-shadow: 0 0 0 3px rgba(0, 122, 255, 0.12); outline: none; }
  .fld > span { font-size: 12px; color: var(--text-dim); flex: none; width: 30px; }
  .fld input {
    flex: 1; min-width: 0; padding: 7px 10px; border-radius: 9px;
    border: 1px solid var(--stroke); background: var(--panel); color: var(--text); font-size: 13px;
  }
  .seg { display: flex; gap: 6px; flex: 1; }
  .seg button {
    flex: 1; padding: 6px 0; border-radius: 8px; cursor: pointer; font-size: 12.5px;
    border: 1px solid var(--stroke); background: var(--panel); color: var(--text-dim);
  }
  .seg button.sel { border-color: var(--accent); color: var(--accent); font-weight: 600; background: var(--accent-bg); }
  .me-fp { font-size: 11px; }
  .pop-actions { display: flex; align-items: center; gap: 8px; padding: 12px 16px 0; }
  .mini.sel-inv { border-color: var(--accent); color: var(--accent); background: var(--accent-bg); }
  .inv-row { display: flex; flex-wrap: wrap; gap: 5px; flex: 1; }
  .bcast-quick { display: flex; gap: 6px; padding: 8px 16px; flex-wrap: wrap; }
  .me-pop .bcast-quick .mini { background: #e9e9eb; border-color: transparent; color: #007aff; border-radius: 999px; }
  .me-pop .bcast-quick .mini:hover { background: #dcdce1; }
  .bcast-list { border-top: 1px solid rgba(60, 60, 67, 0.12); margin-top: 6px; max-height: 300px; overflow-y: auto; }
  .brow { display: flex; align-items: center; gap: 10px; padding: 8px 16px; font-size: 12.5px; cursor: pointer; }
  .brow + .brow { border-top: 1px solid var(--stroke-soft); }
  .brow.off { opacity: 0.6; }
  /* v0.9.17 iOS 开关替换原生复选框 */
  .me-pop .brow input[type="checkbox"] {
    appearance: none; -webkit-appearance: none; width: 40px; height: 24px; border-radius: 999px;
    background: #e9e9eb; position: relative; cursor: pointer; flex: none; margin: 0;
    border: none; transition: background 0.2s ease;
  }
  .me-pop .brow input[type="checkbox"]::after {
    content: ""; position: absolute; top: 2px; left: 2px; width: 20px; height: 20px; border-radius: 50%;
    background: #ffffff; box-shadow: 0 2px 6px rgba(0, 0, 0, 0.25);
    transition: transform 0.2s cubic-bezier(0.32, 0.72, 0, 1);
  }
  .me-pop .brow input[type="checkbox"]:checked { background: #34c759; }
  .me-pop .brow input[type="checkbox"]:checked::after { transform: translateX(16px); }
  .brow .dim { font-size: 10.5px; margin-left: auto; }
  .member-list { max-height: 320px; }
  .chip.role { font-size: 10.5px; color: #d97706; border-color: rgba(217, 119, 6, 0.4); }
  .chip.role.adm { color: #2563eb; border-color: rgba(37, 99, 235, 0.4); }
  /* 记录查看器：iOS 风格（浅色毛玻璃面板 + iMessage 气泡 + 分段控件） */
  .me-pop.hist {
    background: #f2f2f7; color: #1c1c1e; border: none; border-radius: 18px;
    box-shadow: 0 24px 70px rgba(0, 0, 0, 0.35);
    padding: 14px 0 16px;
  }
  .me-pop.hist .me-head b { font-size: 16px; font-weight: 700; letter-spacing: 0.2px; }
  .me-pop.hist .me-head .dim { color: #8e8e93; }
  .me-pop.hist .me-close { color: #8e8e93; border-color: #d1d1d6; }
  .me-pop.hist .me-close:hover { color: #ff3b30; border-color: #ff3b30; }
  /* v0.9.18 转发面板：搜索 + 分节列表，iOS 浅色 */
  .fwd-pop { width: 320px; }
  .fwd-search {
    display: flex; align-items: center; gap: 7px; margin: 8px 16px 4px; padding: 7px 11px;
    background: #e9e9eb; border-radius: 10px;
  }
  .fwd-search span { font-size: 12px; }
  .fwd-search input { flex: 1; min-width: 0; border: none; outline: none; background: transparent; color: #1c1c1e; font-size: 13px; }
  .fwd-list { max-height: 320px; overflow-y: auto; margin-top: 4px; }
  .fwd-sec { padding: 8px 16px 4px; font-size: 11px; font-weight: 700; color: #8e8e93; letter-spacing: 0.4px; }
  .frow {
    display: flex; align-items: center; gap: 10px; width: 100%; text-align: left;
    padding: 8px 16px; border: none; background: transparent; cursor: pointer; font-size: 13px; color: #1c1c1e;
  }
  .frow:hover { background: rgba(0, 122, 255, 0.07); }
  .frow:active { background: rgba(0, 122, 255, 0.12); }
  .frow b { font-weight: 600; }
  .frow .dim { margin-left: auto; font-size: 11px; color: #8e8e93; }
  .frow .farr { color: #c7c7cc; flex: none; }
  .frow:disabled { opacity: 0.55; cursor: wait; }
  /* v0.9.14 文件卡右键菜单（iOS 浮动菜单） */
  .ctx-veil { position: fixed; inset: 0; z-index: 80; }
  .ctx-menu {
    position: fixed; width: 196px; padding: 5px;
    background: rgba(255, 255, 255, 0.92); backdrop-filter: blur(18px) saturate(1.6);
    border: 1px solid rgba(0, 0, 0, 0.06); border-radius: 13px;
    box-shadow: 0 12px 34px rgba(0, 0, 0, 0.18), 0 2px 8px rgba(0, 0, 0, 0.08);
    display: flex; flex-direction: column;
  }
  .ctx-menu button {
    display: flex; align-items: center; gap: 9px;
    border: 0; background: transparent; color: #1c1c1e;
    font-size: 13px; font-weight: 550; padding: 9px 10px;
    border-radius: 8px; cursor: pointer; text-align: left;
  }
  .ctx-menu button:hover { background: rgba(0, 122, 255, 0.1); color: #007aff; }
  .ctx-menu button:active { background: rgba(0, 122, 255, 0.18); }
  .ctx-menu button + button { margin-top: 1px; }
  .ctx-menu button svg { color: #8e8e93; flex: none; }
  .ctx-menu button:hover svg { color: #007aff; }
  .ih-head { display: flex; align-items: center; gap: 10px; padding: 2px 16px 10px; flex: none; }
  /* v0.9.13 记录查看器操作组：iOS 破坏性浅红胶囊 */
  .ih-act { display: flex; align-items: center; gap: 6px; flex: none; }
  .ih-act .ih-btn {
    display: inline-flex; align-items: center; gap: 5px;
    border: 0; background: rgba(255, 59, 48, 0.08); color: #ff3b30;
    border-radius: 999px; padding: 5px 12px; font-size: 12px; font-weight: 600; cursor: pointer;
  }
  .ih-act .ih-btn:hover { background: rgba(255, 59, 48, 0.15); }
  .ih-act .ih-btn:active { background: rgba(255, 59, 48, 0.22); }
  .iseg {
    display: flex; gap: 2px; background: #e3e3e8; border-radius: 9px; padding: 2px; flex: none;
  }
  .iseg button {
    border: none; background: transparent; color: #3a3a3c; font-size: 12.5px; font-weight: 600;
    padding: 5px 16px; border-radius: 7px; cursor: pointer; transition: all 0.15s;
  }
  .iseg button.on { background: #ffffff; color: #007aff; box-shadow: 0 1px 3px rgba(0, 0, 0, 0.12); }
  .isearch {
    flex: 1; min-width: 0; display: flex; align-items: center; gap: 6px;
    background: #e3e3e8; border-radius: 10px; padding: 6px 10px;
  }
  .isearch span { font-size: 12px; opacity: 0.6; }
  .isearch input { flex: 1; min-width: 0; border: none; outline: none; background: transparent; font-size: 13px; color: #1c1c1e; }
  .isearch input::placeholder { color: #8e8e93; }
  .isearch button { border: none; background: transparent; color: #8e8e93; cursor: pointer; font-size: 11px; padding: 2px; }
  .isearch button:hover { color: #ff3b30; }
  .hist-list { padding: 4px 16px 4px; }
  .hrow { display: flex; flex-direction: column; gap: 3px; padding: 5px 0; }
  .htime {
    font-size: 10.5px; color: #8e8e93; text-align: center;
    font-variant-numeric: tabular-nums; letter-spacing: 0.2px;
  }
  .hrow .ibub {
    max-width: 78%; padding: 8px 13px; border-radius: 18px 18px 18px 5px;
    background: #e5e5ea; color: #000; font-size: var(--chat-fs, 13.5px); line-height: 1.5;
    word-break: break-word; white-space: pre-wrap; align-self: flex-start;
  }
  .hrow.out .ibub {
    background: linear-gradient(180deg, #1e9dff, #007aff); color: #fff;
    border-radius: 18px 18px 5px 18px; align-self: flex-end;
  }
  .iwho { color: #007aff; font-size: 11.5px; display: block; margin-bottom: 1px; font-weight: 600; }
  .hrow.out .iwho { color: rgba(255, 255, 255, 0.85); }
  .ifile {
    max-width: 86%; align-self: flex-start; display: flex; align-items: center; gap: 10px;
    background: #ffffff; border-radius: 14px; padding: 10px 14px;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.08);
  }
  .hrow.out .ifile { align-self: flex-end; }
  .iico {
    width: 34px; height: 34px; border-radius: 9px; flex: none;
    background: #eaf3ff; display: flex; align-items: center; justify-content: center; font-size: 17px;
  }
  .imeta { display: flex; flex-direction: column; gap: 1px; min-width: 0; }
  .imeta b { font-size: 13px; color: #1c1c1e; max-width: 260px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .imeta span { font-size: 11px; color: #8e8e93; }
  .iempty { text-align: center; color: #8e8e93; font-size: 12.5px; padding: 40px 0; }
  .me-pop.hist .plist::-webkit-scrollbar { width: 8px; }
  .me-pop.hist .plist::-webkit-scrollbar-thumb { background: #c7c7cc; border-radius: 999px; border: 2px solid #f2f2f7; }
  .me-pop.hist .plist::-webkit-scrollbar-track { background: transparent; }
  .lay-row { flex: 1; display: flex; align-items: center; justify-content: space-between; gap: 8px; min-width: 0; }
  .lay-lab { font-size: 12px; color: var(--text); min-width: 0; }
  /* 档案收纳：右下角 sticky 悬浮 */
  /* v0.9.5 底部行：切换器几何居中、徽章右对齐、两者同一水平线 */
  .bottom-row { flex: none; display: flex; align-items: center; gap: 8px; padding: 4px 4px 10px; }
  .bspacer { flex: 1 1 0; display: flex; }
  .bsp-r { justify-content: flex-end; }
  .me-chip {
    display: inline-flex; align-items: center; gap: 8px;
    padding: 5px 12px 5px 5px; border-radius: 999px; cursor: pointer;
    border: 1px solid var(--stroke-soft); background: var(--panel);
    box-shadow: 0 2px 10px rgba(15, 23, 42, 0.12); transition: box-shadow 0.15s, transform 0.15s;
  }
  .me-chip:hover { transform: translateY(-1px); box-shadow: 0 4px 14px rgba(15, 23, 42, 0.18); }
  .me-name { font-size: 12px; font-weight: 600; max-width: 120px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  /* v0.9.4 面板/聊天模式切换：iOS 拟物滑块（深色凹槽 + 白色浮起滑块） */
  .mode-sw {
    position: relative; display: inline-flex; border-radius: 999px; padding: 3px;
    background: rgba(2, 6, 23, 0.55);
    box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.5), inset 0 -1px 1px rgba(255, 255, 255, 0.07),
      0 1px 0 rgba(255, 255, 255, 0.06);
  }
  .mode-sw .thumb {
    position: absolute; top: 3px; bottom: 3px; left: 3px; width: calc(50% - 3px);
    border-radius: 999px; background: linear-gradient(180deg, #fdfdfe, #e2e5ea);
    box-shadow: 0 2px 5px rgba(0, 0, 0, 0.4), inset 0 1px 0 #ffffff;
    transition: transform 0.26s cubic-bezier(0.3, 0.9, 0.3, 1);
  }
  .mode-sw .thumb.right { transform: translateX(100%); }
  .mode-sw button {
    position: relative; z-index: 1; border: none; background: transparent; cursor: pointer;
    padding: 7px 20px; min-width: 92px; font-size: 12px; font-weight: 600; letter-spacing: 0.5px;
    color: #8ea0b5; transition: color 0.2s;
  }
  .mode-sw button.on { color: #0f172a; text-shadow: 0 1px 0 rgba(255, 255, 255, 0.8); }
  .chip-gear { font-size: 11px; color: var(--text-dim); flex: none; }
</style>
