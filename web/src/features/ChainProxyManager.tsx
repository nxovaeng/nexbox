import { useState, useEffect, useRef } from "react";
import { useT } from "@/core/useT";
import {
  ArrowRight,
  Shield,
  Zap,
  Radio,
  Search,
  Save,
  Route,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Row, Seg, RulesField } from "./panels";
import {
  carrierChainHas,
  carrierChainLabel,
  carrierChainLast,
  type CarrierKind,
  type ConnectionProfile,
} from "@/types";
import {
  carriersAvailable,
  fetchBridges,
  isDesktopRuntime,
  psiphonStatus,
  setPsiphonRegion,
  startCore,
  stopCore,
} from "@/core/api";
import {
  ATTEMPT_CAP_MS,
  isImpossible,
  searchOrder,
  type SearchAttempt,
} from "./carrierSearch";

/**
 * The ways out of the network, and what each one costs.
 */
export const CARRIERS: Array<{ id: CarrierKind; label: string; detail: string }> = [
  {
    id: "aether",
    label: "Aether (WARP)",
    detail: "Cloudflare 高速网络出口，低延迟，近源节点（不改变所在国家）。",
  },
  {
    id: "psiphon",
    label: "Psiphon",
    detail: "自带审查穿透与混淆协议，支持自由指定多国出口，连接相对稍慢。",
  },
  {
    id: "tor",
    label: "Tor (洋葱路由)",
    detail: "三层加密中继节点，匿名性极高，连接较慢，不支持纯 UDP 转发。",
  },
];

export const CARRIER_NAME: Record<CarrierKind, string> = {
  aether: "Aether",
  psiphon: "Psiphon",
  tor: "Tor",
};

/**
 * What each carrier means in the second position, where it decides the exit.
 */
export const AS_EXIT: Record<CarrierKind, string> = {
  aether: "需要本机已完成 Aether 身份注册（不可跨载体注册新账号）。",
  psiphon: "可在下方锁定指定的出口国家/地区。",
  tor: "经由 Tor 随机出口节点出站（不支持纯 UDP）。",
};

/** Where the traffic comes out, given the hop that ends the chain. */
export const EXIT_NOTE: Record<CarrierKind, string> = {
  aether: "流量经由 Cloudflare 边缘节点出站，通常离您本地较近，不改变外部 IP 所属国家。",
  psiphon: "流量经由 Psiphon 服务器出站，可自由在下方指定目标国家。",
  tor: "流量经由 Tor 出口中继出站，外部 IP 随机轮换。",
};

/**
 * Whether a carrier passes datagrams.
 */
export const CARRIES_UDP: Record<CarrierKind, boolean> = {
  aether: true,
  psiphon: false,
  tor: false,
};

/** One choice in either hop picker. */
export function CarrierButton({
  label,
  detail,
  on,
  onClick,
}: {
  label: string;
  detail: string;
  on: boolean;
  onClick: () => void;
}) {
  const t = useT();
  return (
    <button
      type="button"
      aria-pressed={on}
      onClick={onClick}
      className={`rounded-lg border p-3 text-left transition ${
        on ? "border-primary bg-primary/10 shadow-xs" : "border-border hover:border-primary/40 hover:bg-muted/40"
      }`}
    >
      <div className="text-[13.5px] font-semibold text-foreground">{t(label)}</div>
      <div className="mt-1 text-[12px] leading-snug text-muted-foreground">{t(detail)}</div>
    </button>
  );
}

const BRIDGE_MODES: Array<[ConnectionProfile["tor"]["bridges"], string]> = [
  ["none", "Off (关闭)"],
  ["built-in", "Built-in (内置)"],
  ["custom", "Pasted (自定义)"],
];

const BRIDGE_TRANSPORTS: Array<[string, string]> = [
  ["obfs4", "obfs4"],
  ["snowflake", "snowflake"],
  ["meek", "meek"],
];

/** Tor's bridges: off, Tor's own list, or lines the user was given. */
export function TorPanel({
  profile,
  onChange,
}: {
  profile: ConnectionProfile;
  onChange: (profile: ConnectionProfile) => void;
}) {
  const t = useT();
  const set = (patch: Partial<ConnectionProfile["tor"]>) =>
    onChange({ ...profile, tor: { ...profile.tor, ...patch } });

  if (profile.carriers.second === "tor") {
    return (
      <p className="pt-2 text-[12.5px] leading-snug text-muted-foreground">
        {t("当 Tor 作为第二跳时无需配置网桥：前置载体已穿透本地封锁，Tor 直接走常规公网中继。")}
      </p>
    );
  }

  return (
    <div className="space-y-3 pt-2">
      <Row
        title="Tor 网桥模式 (Bridges)"
        help="仅在 Tor 直连被阻断的环境下启用。常规未受审查网络关闭网桥速度更快。"
      >
        <Seg value={profile.tor.bridges} options={BRIDGE_MODES} onChange={(bridges) => set({ bridges })} />
      </Row>

      {profile.tor.bridges === "built-in" ? (
        <Row
          title="网桥传输混淆协议"
          help="Tor 内置分发的公共混淆网桥，易遭深度特征封锁。"
        >
          <Seg
            value={profile.tor.transport || "obfs4"}
            options={BRIDGE_TRANSPORTS}
            onChange={(transport) => set({ transport })}
          />
        </Row>
      ) : null}

      {profile.tor.bridges === "custom" ? (
        <div className="space-y-2">
          <RulesField
            label="自定义网桥线路"
            help="每行一条，来自 bridges.torproject.org 或私人节点。"
            value={profile.tor.customBridges}
            onChange={(customBridges) => set({ customBridges })}
            placeholder="obfs4 1.2.3.4:443 FINGERPRINT cert=… iat-mode=0"
          />
          <BridgeFetch
            onFetched={(lines) =>
              set({
                customBridges: [profile.tor.customBridges.trim(), lines.join("\n")]
                  .filter(Boolean)
                  .join("\n"),
              })
            }
          />
        </div>
      ) : null}
    </div>
  );
}

function BridgeFetch({ onFetched }: { onFetched: (lines: string[]) => void }) {
  const t = useT();
  const [country, setCountry] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);

  const ask = async () => {
    setFailure(null);
    setNote(null);
    setBusy(true);
    try {
      const lines = await fetchBridges(country);
      onFetched(lines);
      setNote(`${lines.length} ${lines.length === 1 ? "条网桥已添加" : "条网桥已添加"}`);
    } catch (error) {
      setFailure(String(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Row
      title="在线获取专属可用网桥"
      help="输入您当前所在的双字符国家代码（如 CN / IR），经由当前代理通道向 Tor 官方安全请求。"
    >
      <div className="flex items-center gap-2">
        <Input
          value={country}
          onChange={(event) => setCountry(event.target.value.toUpperCase().slice(0, 2))}
          placeholder="CN"
          className="h-8 w-16 text-center font-mono text-xs uppercase"
        />
        <Button variant="outline" size="sm" className="h-8 text-xs" disabled={busy || !country} onClick={ask}>
          {busy ? t("正在获取...") : t("请求网桥")}
        </Button>
        {note ? <span className="text-xs text-emerald-500 font-medium">{note}</span> : null}
        {failure ? <span className="text-xs text-destructive">{failure}</span> : null}
      </div>
    </Row>
  );
}

/**
 * Smart automatic path searcher that tries singles and pairs.
 */
export function WayOutSearch({
  profile,
  onChange,
  available,
}: {
  profile: ConnectionProfile;
  onChange: (profile: ConnectionProfile) => void;
  available: CarrierKind[] | null;
}) {
  const t = useT();
  const [attempts, setAttempts] = useState<SearchAttempt[]>([]);
  const [searching, setSearching] = useState(false);
  const [searchFailure, setSearchFailure] = useState<string | null>(null);
  const cancelled = useRef(false);

  const search = async () => {
    setSearching(true);
    setSearchFailure(null);
    cancelled.current = false;
    const order = searchOrder(available);
    setAttempts(order.map((candidate) => ({ chain: candidate, outcome: "pending" })));

    await stopCore().catch(() => {});

    let settled = false;
    for (const [index, candidate] of order.entries()) {
      if (cancelled.current) break;
      setAttempts((current) =>
        current.map((entry, at) => (at === index ? { ...entry, outcome: "trying" } : entry)),
      );

      const timer = window.setTimeout(() => void stopCore().catch(() => {}), ATTEMPT_CAP_MS);
      let failure: string | null = null;
      try {
        await startCore({ ...profile, carriers: candidate });
      } catch (error) {
        failure = error instanceof Error ? error.message : String(error);
      } finally {
        window.clearTimeout(timer);
      }

      if (!failure) {
        setAttempts((current) =>
          current.map((entry, at) => (at === index ? { ...entry, outcome: "connected" } : entry)),
        );
        onChange({ ...profile, carriers: candidate });
        settled = true;
        break;
      }

      setAttempts((current) =>
        current.map((entry, at) =>
          at === index
            ? { ...entry, outcome: isImpossible(failure) ? "skipped" : "failed", detail: failure }
            : entry,
        ),
      );
      await stopCore().catch(() => {});
    }

    if (!settled && !cancelled.current) {
      setSearchFailure(t("未找到可用通路：所有单跳与双跳组合均尝试失败，详见上方各组合失败原因。"));
    }
    setSearching(false);
  };

  return (
    <Card className="border-primary/40 bg-primary/[0.04]">
      <CardHeader className="pb-3">
        <CardTitle className="text-[15px] flex items-center gap-2">
          <Search className="size-4 text-primary" />
          {t("不确定哪个通路可用？智能自动测径")}
        </CardTitle>
        <CardDescription>
          {t("按响应速度依次测试各单跳载体；若单跳全部被阻断，则自动尝试双跳组合穿透，直到找到可用链路。")}
        </CardDescription>
      </CardHeader>
      <CardContent className="flex flex-col gap-3 pt-0">
        <div className="flex flex-wrap items-center gap-3">
          <Button
            variant={searching ? "outline" : "default"}
            size="sm"
            onClick={() => {
              if (searching) {
                cancelled.current = true;
                void stopCore().catch(() => {});
                return;
              }
              void search();
            }}
          >
            {searching ? t("停止测试") : t("一键自动测径并连接")}
          </Button>
          {searching ? (
            <p className="text-[12px] leading-snug text-muted-foreground">
              {t("每项测试耗时最多 90 秒，点击停止将在当前尝试完成后中断。")}
            </p>
          ) : null}
        </div>

        {searchFailure ? <p className="text-[12px] text-destructive">{searchFailure}</p> : null}

        {attempts.length > 0 ? (
          <ul className="flex flex-col gap-1 rounded-md border border-border/50 bg-background/60 p-2.5 font-mono text-xs">
            {attempts.map((entry) => {
              const name = carrierChainLabel(entry.chain, (kind) => t(CARRIER_NAME[kind]));
              const mark = {
                pending: "·",
                trying: "…",
                connected: "✓",
                failed: "✕",
                skipped: "–",
              }[entry.outcome];
              const tone = {
                pending: "text-muted-foreground/60",
                trying: "text-amber-500 font-bold",
                connected: "text-emerald-500 font-bold",
                failed: "text-red-400",
                skipped: "text-muted-foreground/70",
              }[entry.outcome];
              return (
                <li key={name} className={`flex items-center gap-2 py-0.5 leading-snug ${tone}`}>
                  <span className="w-4 text-center">{mark}</span>
                  <span className="font-semibold">{name}</span>
                  {entry.outcome === "connected" ? (
                    <span className="text-emerald-500 font-normal"> — 链路验证成功已载入流量</span>
                  ) : null}
                  {entry.detail && entry.outcome !== "connected" ? (
                    <span className="text-muted-foreground font-normal text-[11px]"> — {entry.detail}</span>
                  ) : null}
                </li>
              );
            })}
          </ul>
        ) : null}
      </CardContent>
    </Card>
  );
}

/**
 * Main CarrierPanel multi-hop selector.
 */
export function CarrierPanel({
  profile,
  onChange,
}: {
  profile: ConnectionProfile;
  onChange: (profile: ConnectionProfile) => void;
}) {
  const t = useT();
  const set = (patch: Partial<ConnectionProfile>) => onChange({ ...profile, ...patch });
  const [regions, setRegions] = useState<string[]>([]);
  const [moving, setMoving] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const [available, setAvailable] = useState<CarrierKind[] | null>(null);

  const chain = profile.carriers;
  const offered = CARRIERS.filter((option) => !available || available.includes(option.id));
  const carriesUdp = CARRIES_UDP[chain.first] && (chain.second === null || CARRIES_UDP[chain.second]);

  useEffect(() => {
    if (!isDesktopRuntime()) return;
    let cancelled = false;
    carriersAvailable()
      .then((list) => {
        if (!cancelled) setAvailable(list);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!carrierChainHas(profile.carriers, "psiphon") || !isDesktopRuntime()) return;
    let cancelled = false;
    const read = () =>
      psiphonStatus()
        .then((status) => {
          if (!cancelled) setRegions(status.availableRegions);
        })
        .catch(() => {});
    read();
    const timer = window.setInterval(read, 5_000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [profile.carriers.first, profile.carriers.second]);

  const chooseRegion = async (region: string) => {
    set({ psiphon: { ...profile.psiphon, egressRegion: region } });
    if (!isDesktopRuntime()) return;
    setFailure(null);
    setMoving(true);
    try {
      await setPsiphonRegion(region);
    } catch (error) {
      setFailure(String(error));
    } finally {
      setMoving(false);
    }
  };

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-[15px] flex items-center gap-2">
            <Route className="size-4 text-primary" />
            {t("出口载体与多跳编排 (Way Out & Multi-Hop)")}
          </CardTitle>
          <CardDescription>
            {t("选择携带流量穿透本地网络的第一跳载体；可配置第二跳更改实际公网出口位置。")}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4 pt-0">
          <div>
            <p className="pb-2 text-xs font-semibold uppercase tracking-wider text-muted-foreground flex items-center gap-1">
              <Zap className="size-3 text-primary" />
              {t("第一跳：离开本机网络 (Leaves this network through)")}
            </p>
            <div className="grid grid-cols-1 md:grid-cols-3 gap-2.5">
              {offered.map((option) => (
                <CarrierButton
                  key={option.id}
                  label={option.label}
                  detail={option.detail}
                  on={chain.first === option.id}
                  onClick={() =>
                    set({
                      carriers: { first: option.id, second: chain.second === option.id ? null : chain.second },
                    })
                  }
                />
              ))}
            </div>
          </div>

          <div>
            <p className="pb-2 pt-1 text-xs font-semibold uppercase tracking-wider text-muted-foreground flex items-center gap-1">
              <ArrowRight className="size-3 text-primary" />
              {t("第二跳：最终出口中继 (Then out through)")}
            </p>
            <div className="grid grid-cols-1 md:grid-cols-3 gap-2.5">
              <CarrierButton
                label="不使用第二跳 (单跳直达)"
                detail="单跳出站：流量直接从第一跳所连接的网络出口发出。"
                on={chain.second === null}
                onClick={() => set({ carriers: { first: chain.first, second: null } })}
              />
              {offered
                .filter((option) => option.id !== chain.first)
                .map((option) => (
                  <CarrierButton
                    key={option.id}
                    label={option.label}
                    detail={AS_EXIT[option.id]}
                    on={chain.second === option.id}
                    onClick={() => set({ carriers: { first: chain.first, second: option.id } })}
                  />
                ))}
            </div>
          </div>

          {/* Current chain summary card */}
          <div className="rounded-lg border border-border bg-muted/30 p-3.5 space-y-2">
            <div className="flex items-center gap-2">
              <Badge variant="outline" className="font-semibold text-xs border-primary/40 text-primary bg-primary/5">
                当前拓扑
              </Badge>
              <span className="text-[13.5px] font-semibold text-foreground">
                {carrierChainLabel(chain, (kind) => t(CARRIER_NAME[kind]))}
              </span>
            </div>
            <p className="text-xs text-muted-foreground leading-relaxed">
              {t(EXIT_NOTE[carrierChainLast(chain)])}
            </p>
            {chain.second === "aether" ? (
              <p className="text-xs leading-snug text-amber-500 font-medium">
                {t("提示：Aether 无法通过其他代理注册新设备，该组合仅在之前已成功连接过 Aether 时可用。")}
              </p>
            ) : null}
            {chain.second && !carriesUdp ? (
              <p className="text-xs leading-snug text-muted-foreground">
                {t("注意：此多跳组合不支持 UDP 转发，QUIC 与纯 UDP DNS 将自动降级走 TCP 隧道。")}
              </p>
            ) : null}
          </div>

          {/* Psiphon options if included */}
          {carrierChainHas(profile.carriers, "psiphon") ? (
            <div className="rounded-lg border border-border/70 p-3 space-y-2 bg-muted/20">
              <span className="text-xs font-semibold text-foreground flex items-center gap-1.5">
                <Radio className="size-3.5 text-primary" />
                Psiphon 出口国家选择
              </span>
              <Row title="指定出口国家" help="设置出口国家偏好；Psiphon 将优先连接该国家节点。">
                <select
                  value={profile.psiphon.egressRegion}
                  disabled={moving}
                  onChange={(event) => void chooseRegion(event.target.value)}
                  className="h-8 rounded-md border border-input bg-background px-2 text-xs"
                >
                  <option value="">{t("自动优选可用最佳地区")}</option>
                  {regions.map((region) => (
                    <option key={region} value={region}>
                      {region}
                    </option>
                  ))}
                </select>
              </Row>
              {moving ? (
                <p className="text-xs text-amber-500">{t("正在切换出口国家并重新建立连接...")}</p>
              ) : null}
              {failure ? <p className="text-xs text-destructive">{failure}</p> : null}
              {regions.length === 0 ? (
                <p className="text-[11px] text-muted-foreground">
                  {t("国家列表将在 Psiphon 成功握手后自动更新加载。")}
                </p>
              ) : null}
            </div>
          ) : null}

          {/* Tor options if included */}
          {carrierChainHas(profile.carriers, "tor") ? (
            <div className="rounded-lg border border-border/70 p-3 space-y-2 bg-muted/20">
              <span className="text-xs font-semibold text-foreground flex items-center gap-1.5">
                <Shield className="size-3.5 text-primary" />
                Tor 洋葱网络网桥与中继设置
              </span>
              <TorPanel profile={profile} onChange={onChange} />
            </div>
          ) : null}
        </CardContent>
      </Card>

      <WayOutSearch profile={profile} onChange={onChange} available={available} />
    </div>
  );
}

export interface ChainProxyManagerProps {
  profile: ConnectionProfile;
  onChange: (profile: ConnectionProfile) => void;
  onSave?: () => void;
  onToast?: (title: string, message: string, error?: boolean) => void;
}

export function ChainProxyManager({
  profile,
  onChange,
  onSave,
}: ChainProxyManagerProps) {
  const t = useT();
  return (
    <div className="space-y-4">
      <CarrierPanel profile={profile} onChange={onChange} />

      {onSave && (
        <div className="flex justify-end pt-2 pb-4">
          <Button onClick={onSave} className="gap-2 shadow-sm">
            <Save className="size-4" />
            {t("保存链式代理与多跳配置")}
          </Button>
        </div>
      )}
    </div>
  );
}
