// 数据提取核心 —— 从复杂文本中提取 IPv4 / IPv6 / URL / 域名 / 主机名
// 设计原则：宽松候选 + 严格校验（与 CyberChef 同思路），保证低误报

export type ExtractKind = "ipv4" | "ipv6" | "url" | "domain" | "hostname";

export const EXTRACT_LABELS: Record<ExtractKind, string> = {
  ipv4: "IPv4",
  ipv6: "IPv6",
  url: "URL",
  domain: "域名",
  hostname: "主机名"
};

// ---------------------------------------------------------------------------
// 校验器
// ---------------------------------------------------------------------------
export function isIPv4(s: string): boolean {
  const m = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(s);
  if (!m) return false;
  return m.slice(1).every((x) => +x <= 255);
}

/** 严格 IPv6 校验（含 :: 压缩、内嵌 IPv4 尾部、%zone 忽略） */
export function isIPv6(s: string): boolean {
  let v = s;
  const pct = v.indexOf("%");
  if (pct >= 0) v = v.slice(0, pct);
  if (v.length < 2) return false;
  const dc = v.split("::");
  if (dc.length > 2) return false;

  // 降噪：8 组全为 1-2 位纯十进制数字的串（如 11:22:33:44:55:66:77:88）
  // 语法上是合法 IPv6，但实际几乎都是时间/编号序列，直接排除
  if (!v.includes("::")) {
    const groups = v.split(":");
    if (groups.length === 8 && groups.every((g) => /^\d{1,2}$/.test(g))) return false;
  }

  const badGroup = (g: string) => !/^[0-9A-Fa-f]{1,4}$/.test(g);
  const head = dc[0] === "" ? [] : dc[0].split(":");
  for (const g of head) if (badGroup(g)) return false;

  let tailCount = 0;
  if (dc.length === 2) {
    const tail = dc[1] === "" ? [] : dc[1].split(":");
    for (let i = 0; i < tail.length; i++) {
      const g = tail[i];
      if (g.includes(".")) {
        // 内嵌 IPv4 只能作为最后一组，且占 2 个组
        if (i !== tail.length - 1 || !isIPv4(g)) return false;
        tailCount += 2;
      } else {
        if (badGroup(g)) return false;
        tailCount += 1;
      }
    }
    return head.length + tailCount <= 7; // "::" 至少压缩一个全零组
  }
  return head.length === 8;
}

// ---------------------------------------------------------------------------
// 候选正则
// ---------------------------------------------------------------------------
const IPV4_CAND = /(?<![\w.])((?:\d{1,3}\.){3}\d{1,3})(?![\w.])/g;

// 经典 IPv6 全形态（含 :: 压缩的各种位置）；随后用 isIPv6 严格过滤
const IPV6_CAND = new RegExp(
  "(?<![\\w:.])(" +
    "(?:[A-Fa-f0-9]{1,4}:){7}[A-Fa-f0-9]{1,4}" +
    "|(?:[A-Fa-f0-9]{1,4}:){1,7}:" +
    "|(?:[A-Fa-f0-9]{1,4}:){1,6}:[A-Fa-f0-9]{1,4}" +
    "|(?:[A-Fa-f0-9]{1,4}:){1,5}(?::[A-Fa-f0-9]{1,4}){1,2}" +
    "|(?:[A-Fa-f0-9]{1,4}:){1,4}(?::[A-Fa-f0-9]{1,4}){1,3}" +
    "|(?:[A-Fa-f0-9]{1,4}:){1,3}(?::[A-Fa-f0-9]{1,4}){1,4}" +
    "|(?:[A-Fa-f0-9]{1,4}:){1,2}(?::[A-Fa-f0-9]{1,4}){1,5}" +
    "|[A-Fa-f0-9]{1,4}:(?::[A-Fa-f0-9]{1,4}){1,6}" +
    "|::(?:(?:[A-Fa-f0-9]{1,4}:){0,6}[A-Fa-f0-9]{1,4})?" +
    ")(?![\\w:.])",
  "g"
);
// 内嵌 IPv4 尾部的 IPv6（::ffff:192.168.1.1 / 64:ff9b::1.2.3.4 等）
const IPV6_V4_CAND = /(?<![\w:.])((?:[A-Fa-f0-9]{1,4}:){1,6}:(?:(?:[A-Fa-f0-9]{1,4}:){0,4})?(?:\d{1,3}\.){3}\d{1,3})(?![\w.])/g;

const URL_CAND = /(?:https?|ftp):\/\/[^\s<>"'`\u3000-\u303F\uFF00-\uFFEF]+/gi;
const DOMAIN_CAND = /(?<![\w.-])((?:[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?\.)+[A-Za-z]{2,63})(?![\w-])/g;

// 主机名：按上下文提取（避免把普通单词当主机名）
// 1) UNC 路径 \\HOST\share
const HOST_UNC = /\\\\([A-Za-z0-9][A-Za-z0-9.-]{0,60})\\/g;
// 2) 键值对：Computer/Hostname/Host/主机名 等
const HOST_KV = /(?<![\w-])(?:computer(?:[_ ]?name)?|hostname|host[_ ]?name|host|machine(?:[_ ]?name)?|server(?:[_ ]?name)?|device|node|主机名|计算机名|服务器名|主机)\s*[:=：]\s*"?([A-Za-z0-9][A-Za-z0-9-]{0,60})"?/gi;
// 3) 常见 Windows 计算机命名形态（大写前缀-后缀，含数字）
const HOST_SHAPE = /\b(?:DESKTOP|LAPTOP|WIN|PC|SRV|SERVER|WEB|DB|APP|FILE|PRINT|AD|DC|CA|EXCH|SQL|TEST|DEV|UAT|PROD|OA|VPN|FW|NAS|VM|K8S|NODE|WS|POS|ATM|ERP|HR|FIN|MAIL|DNS|DHCP|FTP|GIT|JENKINS|ZABBIX|ESXI|VSPHERE|BOOK|NANO|HOME|OFFICE)-[A-Za-z0-9]{2,}(?:[-_][A-Za-z0-9]+)*/g;

function dedupe(list: string[]): string[] {
  return [...new Set(list)];
}

function cleanUrl(u: string): string {
  let out = u.trim();
  // 去尾部常见标点/引号/中文括号
  out = out.replace(/[.,;:!?"'）〉》】\]]+$/, "");
  // 去掉未配对的右括号
  while (out.endsWith(")") && (out.match(/\(/g) || []).length < (out.match(/\)/g) || []).length) {
    out = out.slice(0, -1);
  }
  return out;
}

function runOne(kind: ExtractKind, input: string): string[] {
  switch (kind) {
    case "ipv4": {
      const out: string[] = [];
      for (const m of input.matchAll(IPV4_CAND)) if (isIPv4(m[1])) out.push(m[1]);
      return dedupe(out);
    }
    case "ipv6": {
      const out: string[] = [];
      for (const m of input.matchAll(IPV6_CAND)) if (isIPv6(m[1])) out.push(m[1]);
      for (const m of input.matchAll(IPV6_V4_CAND)) if (isIPv6(m[1])) out.push(m[1]);
      return dedupe(out);
    }
    case "url": {
      const out: string[] = [];
      for (const m of input.matchAll(URL_CAND)) {
        const u = cleanUrl(m[0]);
        if (u.length > 4) out.push(u);
      }
      return dedupe(out);
    }
    case "domain": {
      const out: string[] = [];
      for (const m of input.matchAll(DOMAIN_CAND)) out.push(m[1].toLowerCase());
      return dedupe(out);
    }
    case "hostname": {
      const out: string[] = [];
      for (const m of input.matchAll(HOST_UNC)) out.push(m[1].replace(/\.+$/, ""));
      for (const m of input.matchAll(HOST_KV)) {
        const v = m[1];
        if (/[A-Za-z]/.test(v)) out.push(v);
      }
      for (const m of input.matchAll(HOST_SHAPE)) {
        // 过滤不含数字的纯单词组合（如 TEST-DRIVEN），计算机名一般含数字
        if (/\d/.test(m[0])) out.push(m[0]);
      }
      return dedupe(out);
    }
  }
}

export interface ExtractResult {
  items: string[];
  counts: Partial<Record<ExtractKind, number>>;
}

/** 按所选类型提取；结果顺序 = 类型顺序 + 出现顺序；可选去重/排序 */
export function extractData(
  input: string,
  kinds: ExtractKind[],
  opts: { dedupe: boolean; sort: boolean }
): ExtractResult {
  const counts: Partial<Record<ExtractKind, number>> = {};
  const seen = new Set<string>();
  const items: string[] = [];
  for (const k of kinds) {
    const res = runOne(k, input);
    counts[k] = res.length;
    for (const it of res) {
      if (opts.dedupe) {
        if (seen.has(it)) continue;
        seen.add(it);
      }
      items.push(it);
    }
  }
  if (opts.sort) items.sort((a, b) => a.localeCompare(b, undefined, { numeric: true }));
  return { items, counts };
}
