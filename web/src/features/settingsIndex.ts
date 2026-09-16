/**
 * What the settings search can find.
 *
 * Written out rather than derived from the rendered controls: the words someone
 * types are rarely the words on the label. Someone looking for "kill switch"
 * will not search for "Reach", and someone who wants to stop DNS leaking will
 * not know the setting is called "Resolvers".
 */
export type SectionId =
  | "aether"
  | "proton"
  | "windscribe"
  | "psiphon"
  | "traffic"
  | "identity"
  | "core"
  | "logs"
  | "diagnostics"
  | "licences";

export interface SettingEntry {
  label: string;
  section: SectionId;
  /** Where it lives, shown so the result is findable again without searching. */
  where: string;
  /** Extra words that should match it, including the ones people actually use. */
  keywords: string;
}

export const SECTION_LABELS: Record<SectionId, string> = {
  aether: "Aether (WARP) 方案管理",
  proton: "Proton 配置管理",
  windscribe: "Windscribe 节点",
  psiphon: "Psiphon 配置管理",
  traffic: "Traffic & DNS",
  identity: "Cloudflare Identity",
  core: "Core Management",
  logs: "实时运行日志",
  diagnostics: "Diagnostics & Logs",
  licences: "Licences & notices",
};

export const SETTINGS: SettingEntry[] = [
  { label: "Aether 参数方案", section: "aether", where: "Aether (WARP) 方案管理", keywords: "warp aether profile scheme direct stealth noize fragment h2 h3" },
  { label: "海外 VPS 极速直连预设", section: "aether", where: "Aether (WARP) 方案管理", keywords: "overseas direct turbo h3 quic fast low latency" },
  { label: "受管控抗封锁 H2 预设", section: "aether", where: "Aether (WARP) 方案管理", keywords: "stealth h2 fragment firewall obfuscation" },
  { label: "TLS ClientHello 分片", section: "aether", where: "Aether (WARP) 方案管理", keywords: "fragment client hello dpi censorship filtering bypass" },
  { label: "Noize 混流伪装", section: "aether", where: "Aether (WARP) 方案管理", keywords: "noize noise padding gfw firewall aggressive hide traffic fingerprint" },
  { label: "Aether 端点选择与优选", section: "aether", where: "Aether (WARP) 方案管理", keywords: "scan gateways find ip addresses test candidates rank peer endpoint" },
  { label: "WireGuard 原生直连", section: "aether", where: "Aether (WARP) 方案管理", keywords: "wireguard wg udp native handshake" },

  { label: "Proton 节点管理", section: "proton", where: "Proton 配置管理", keywords: "proton wireproxy cert account guest login free servers" },
  { label: "Windscribe 节点", section: "windscribe", where: "Windscribe 节点", keywords: "windscribe https credentials proxy nodes" },
  { label: "Psiphon 节点", section: "psiphon", where: "Psiphon 配置管理", keywords: "psiphon egress region server list" },

  { label: "Traffic & DNS", section: "traffic", where: "Traffic & DNS", keywords: "dns resolvers socks5 1819 routing rules" },
  { label: "Cloudflare Zero Trust", section: "identity", where: "Cloudflare Identity", keywords: "team access client id secret token enrolment organisation login" },
  { label: "Core 内核程序", section: "core", where: "Core Management", keywords: "core binary download upload inventory aether wireproxy psiphon" },
  { label: "实时内核日志", section: "logs", where: "实时运行日志", keywords: "live logs realtime stdout stderr events streaming monitor console" },
  { label: "运行日志与诊断", section: "diagnostics", where: "Diagnostics & Logs", keywords: "log detail level report debug verbose" },
  { label: "Licences & notices", section: "licences", where: "Licences & notices", keywords: "licence license gpl agpl open source" },
];

/**
 * Ranks matches so the closest one is first and can be taken with Enter.
 *
 * A label match outranks a keyword match, and a match at the start of the label
 * outranks one in the middle -- typing "dns" should reach "DNS resolvers"
 * before "Send web traffic to Gateway", which only mentions it in passing.
 */
export function searchSettings(
  query: string,
  t: (key: string) => string = (key) => key,
  limit = 8,
): SettingEntry[] {
  const needle = query.trim().toLowerCase();
  if (!needle) return SETTINGS.slice(0, limit);

  const scored = SETTINGS.map((entry, order) => {
    const label = entry.label.toLowerCase();
    // Matched in the reader's language and in English both. Someone reading a
    // Persian interface types Persian; someone who learned these settings by
    // their English names, or who is reading a forum post, types English. Only
    // checking one of the two makes half the search useless.
    const translated = t(entry.label).toLowerCase();
    const translatedWhere = t(entry.where).toLowerCase();
    let score = 0;
    if (label.startsWith(needle) || translated.startsWith(needle)) score = 100;
    else if (label.includes(needle) || translated.includes(needle)) score = 70;
    else if (entry.where.toLowerCase().includes(needle) || translatedWhere.includes(needle)) score = 40;
    else if (entry.keywords.includes(needle)) score = 30;
    return { entry, score, order };
  }).filter((candidate) => candidate.score > 0);

  // Ties break on the order these are written in, not alphabetically. Searching
  // a section name matches every setting in it equally, and the one worth
  // offering first is the control the section exists for -- which is the one
  // listed first. Sorting by label instead handed back whichever happened to
  // start with the earliest letter.
  scored.sort((a, b) => b.score - a.score || a.order - b.order);
  return scored.slice(0, limit).map((candidate) => candidate.entry);
}
