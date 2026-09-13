import type { ScanCandidate } from "@/core/api";

export function summarise(candidates: ScanCandidate[]): string {
  const networks = byNetwork(candidates).length;
  const gateways = `${candidates.length} gateway${candidates.length === 1 ? "" : "s"}`;
  if (networks === 1) return `${gateways} answered, all on one network.`;
  return `${gateways} answered across ${networks} networks.`;
}

/// Gateways grouped by the network they sit in, best first.
///
/// A flat list sorted by latency is dominated by whichever range is nearest,
/// which reads as "the scanner only finds one place". Grouping shows which
/// networks answer at all -- the thing worth knowing when the usual one is
/// being throttled.
export function byNetwork(candidates: ScanCandidate[]) {
  const groups = new Map<string, ScanCandidate[]>();
  for (const candidate of candidates) {
    const network = networkOf(candidate.peer);
    const existing = groups.get(network);
    if (existing) existing.push(candidate);
    else groups.set(network, [candidate]);
  }
  return [...groups.entries()]
    .map(([network, members]) => ({ network, members }))
    .sort((a, b) => a.members[0].rttMs - b.members[0].rttMs);
}

/// The /24 for IPv4, the /48 for IPv6 — the granularity Cloudflare allocates
/// these gateways in, so it matches the ranges actually being sampled.
export function networkOf(peer: string): string {
  if (peer.startsWith("[")) {
    const address = peer.slice(1, peer.indexOf("]"));
    return `${address.split(":").slice(0, 3).join(":")}::/48`;
  }
  const octets = peer.split(":")[0].split(".");
  return octets.length === 4 ? `${octets.slice(0, 3).join(".")}.0/24` : peer;
}
