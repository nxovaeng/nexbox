import type { ConnectionProfile } from "../types";

export function buildCoreCommand(profile: ConnectionProfile): string {
  const args: string[] = [
    // Protocol: --masque / --wg / --gool  (format!("--{}", protocol) in Rust)
    `--${profile.protocol}`,
    "--scan", profile.scanMode,
    "--ip", profile.ipFamily,
    "--bind", quote(profile.socksAddress),
    "--validate-secs", String(profile.validateSecs),
    "--startup-secs", String(profile.startupSecs),
    "--reconnect-secs", String(profile.reconnectSecs),
    "--dns", quote(profile.dns.join(",")),
    "--noize", profile.noize,
    "--log-level", processLogLevel(profile.logLevel),
    "--config", "<identity-file>",
  ];

  // keepalive: only when > 0 (zero means "let the engine decide")
  if (profile.keepaliveSecs > 0) {
    args.push("--keepalive", String(profile.keepaliveSecs));
  }

  // --h2 flag: only when MASQUE H2
  if (profile.protocol === "masque" && profile.masqueTransport === "h2") {
    args.push("--h2");
  }

  if (!profile.dataCheck) {
    args.push("--no-data-check");
  }

  args.push(profile.quickReconnect ? "--quick-reconnect" : "--no-quick-reconnect");

  // TLS fragmentation: MASQUE H2 only
  if (
    profile.fragmentClientHello &&
    profile.protocol === "masque" &&
    profile.masqueTransport === "h2"
  ) {
    args.push(
      "--fragment",
      "--fragment-size", quote(profile.fragmentSize),
      "--fragment-delay", quote(profile.fragmentDelay),
    );
  }

  if (!profile.profileRetry) {
    args.push("--no-profile-retry");
  }

  // Endpoint: only when not automatic
  if (profile.endpointMode !== "automatic" && profile.peer?.trim()) {
    args.push("--peer", quote(profile.peer));
  }
  if (profile.wgPeer?.trim()) {
    args.push("--wg-peer", quote(profile.wgPeer));
  }
  if (profile.h2Peer?.trim()) {
    args.push("--h2-peer", quote(profile.h2Peer));
  }

  // ECH: omit when "off" or empty
  const ech = profile.ech?.trim();
  if (ech && ech !== "off") {
    args.push("--ech", quote(ech));
  }
  if (profile.tlsGroups?.trim()) {
    args.push("--tls-groups", quote(profile.tlsGroups));
  }

  if (profile.performanceProfile !== "auto") {
    args.push("--perf", profile.performanceProfile);
  }

  if (profile.routeBlock.trim()) {
    args.push("--route-block", quote(profile.routeBlock));
  }
  if (profile.routeDirect.trim()) {
    args.push("--route-direct", quote(profile.routeDirect));
  }
  if (profile.routesFile?.trim()) {
    args.push("--routes", quote(profile.routesFile));
  }

  if (profile.gateway) {
    args.push("--gateway");
  }

  return `aether ${args.join(" ")}`;
}

/**
 * Connection state, the selected edge and the latency are all read out of
 * info-level core output, so the supervisor never runs the child below info.
 * Extra verbosity is passed through.
 */
export function processLogLevel(level: ConnectionProfile["logLevel"]): string {
  return level === "debug" || level === "trace" ? level : "info";
}

function quote(value: string): string {
  // Single quotes, not JSON.stringify: its double quotes still expand $(...) and backticks, and
  // this string is offered to the user as the command to run. Also restores real newlines, which
  // JSON.stringify turned into a literal \n.
  return `'${value.replace(/\n/g, "\\n").replace(/'/g, `'\\''`)}'`;
}
