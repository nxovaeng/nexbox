import { useState, useEffect, useCallback, useMemo } from "react";
import {
  Check,
  Copy,
  Globe,
  KeyRound,
  Lock,
  Play,
  Plus,
  RefreshCw,
  Server,
  Settings,
  Square,
  Unlock,
  Wifi,
  Zap,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import {
  listSocksInstances,
  startSocksInstance,
  stopSocksInstance,
  testSocksConnectivity,
  testSocksSpeed,
  getAetherProfiles,
  type AetherProfileConfig,
  type SocksInstanceView,
} from "@/core/api";

const CURL_TARGETS = [
  { label: "IPWho.is (高可用/详细)", url: "https://ipwho.is" },
  { label: "IP.SB (纯净 GeoIP)", url: "https://api.ip.sb/geoip" },
  { label: "ifconfig.co (简洁 JSON)", url: "https://ifconfig.co/json" },
  { label: "ipapi.co (高精度)", url: "https://ipapi.co/json/" },
  { label: "Cloudflare (Trace)", url: "https://cloudflare.com/cdn-cgi/trace" },
  { label: "ipinfo.io (原站点/易429)", url: "https://ipinfo.io" },
];

interface SocksDashboardProps {
  onManageClick: () => void;
  onToast: (title: string, message: string, error?: boolean) => void;
}

export function SocksDashboard({ onManageClick, onToast }: SocksDashboardProps) {
  const [instances, setInstances] = useState<SocksInstanceView[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [aetherProfiles, setAetherProfiles] = useState<AetherProfileConfig[]>([]);
  const [loading, setLoading] = useState(true);
  const [testingConn, setTestingConn] = useState<Record<string, boolean>>({});
  const [testingSpeed, setTestingSpeed] = useState<Record<string, boolean>>({});
  const [actionLoading, setActionLoading] = useState<Record<string, boolean>>({});
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [curlTarget, setCurlTarget] = useState<string>("https://ipwho.is");

  // Fetch instances list
  const refresh = useCallback(async () => {
    try {
      const [list, plist] = await Promise.all([
        listSocksInstances(),
        getAetherProfiles().catch(() => []),
      ]);
      setInstances(list);
      if (plist && plist.length > 0) {
        setAetherProfiles(plist);
      }
      if (list.length > 0 && !selectedId) {
        // Select first running or first instance
        const running = list.find((i) => i.status.isRunning);
        setSelectedId(running ? running.id : list[0].id);
      }
    } catch (e) {
      console.error("Failed to load SOCKS5 instances:", e);
    } finally {
      setLoading(false);
    }
  }, [selectedId]);

  useEffect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), 4000);
    return () => clearInterval(timer);
  }, [refresh]);

  const selected = useMemo(() => {
    return instances.find((i) => i.id === selectedId) || instances[0] || null;
  }, [instances, selectedId]);

  // Actions
  const handleToggleInstance = async (id: string, currentlyRunning: boolean) => {
    setActionLoading((prev) => ({ ...prev, [id]: true }));
    try {
      if (currentlyRunning) {
        await stopSocksInstance(id);
        onToast("服务已停止", `SOCKS5 实例 [${id}] 已成功关闭端口监听`);
      } else {
        await startSocksInstance(id);
        onToast("服务已启动", `SOCKS5 实例 [${id}] 已成功监听并就绪`);
      }
      await refresh();
    } catch (e) {
      onToast("操作失败", e instanceof Error ? e.message : String(e), true);
    } finally {
      setActionLoading((prev) => ({ ...prev, [id]: false }));
    }
  };

  const handleTestConnectivity = async (id: string) => {
    setTestingConn((prev) => ({ ...prev, [id]: true }));
    try {
      const res = await testSocksConnectivity(id);
      if (res.success) {
        onToast(
          "真连通测试成功",
          `出口IP: ${res.ip ?? "未知"} | 国家: ${res.country ?? "未知"} | 延迟: ${res.latencyMs ? res.latencyMs + "ms" : "OK"}`
        );
      } else {
        onToast("真连通测试失败", res.error ?? "未能穿透连接验证", true);
      }
      await refresh();
    } catch (e) {
      onToast("测试出错", e instanceof Error ? e.message : String(e), true);
    } finally {
      setTestingConn((prev) => ({ ...prev, [id]: false }));
    }
  };

  const handleTestSpeed = async (id: string) => {
    setTestingSpeed((prev) => ({ ...prev, [id]: true }));
    try {
      const res = await testSocksSpeed(id);
      if (res.success && res.mbps !== undefined && res.mbps !== null) {
        onToast("测速完成", `实测带宽速率: ${res.mbps} Mbps (耗时 ${res.durationSecs}s)`);
      } else {
        onToast("测速未达标", res.error ?? "测速未能完整传输数据包", true);
      }
      await refresh();
    } catch (e) {
      onToast("测速异常", e instanceof Error ? e.message : String(e), true);
    } finally {
      setTestingSpeed((prev) => ({ ...prev, [id]: false }));
    }
  };

  const copyToClipboard = (text: string, key: string) => {
    navigator.clipboard.writeText(text);
    setCopiedKey(key);
    setTimeout(() => setCopiedKey(null), 2000);
    onToast("已复制", text);
  };

  // Compute summary stats
  const activeCount = instances.filter((i) => i.status.isRunning).length;
  const totalCount = instances.length;
  const vpsHost = window.location.hostname || "127.0.0.1";

  return (
    <div className="flex h-full flex-col overflow-y-auto bg-background p-6">
      {/* ── Top Metric Cards ────────────────────────────────────────────── */}
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <Card className="border-border/60 bg-card/60 backdrop-blur">
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              活跃 SOCKS5 端口
            </CardTitle>
            <Server className="size-4 text-primary" />
          </CardHeader>
          <CardContent>
            <div className="flex items-baseline gap-2">
              <span className="text-2xl font-bold tracking-tight text-foreground">{activeCount}</span>
              <span className="text-xs text-muted-foreground">/ {totalCount} 已配置</span>
            </div>
            <p className="mt-1 text-[11.5px] text-muted-foreground">
              {activeCount > 0 ? "全部已启用的 VPS 端口监听中" : "当前无运行中的 SOCKS5 程序"}
            </p>
          </CardContent>
        </Card>

        <Card className="border-border/60 bg-card/60 backdrop-blur">
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              出口国家 / 地区
            </CardTitle>
            <Globe className="size-4 text-blue-500" />
          </CardHeader>
          <CardContent>
            <div className="flex items-center gap-1.5 overflow-hidden">
              {instances.filter((i) => i.status.isRunning).map((i) => (
                <span
                  key={i.id}
                  className="inline-flex items-center rounded border bg-muted/60 px-1.5 py-0.5 font-mono text-[11px] font-medium"
                >
                  {i.status.lastConnectivity?.country || getCountryHint(i)}
                </span>
              ))}
              {activeCount === 0 && <span className="text-sm font-medium text-muted-foreground">--</span>}
            </div>
            <p className="mt-1 text-[11.5px] text-muted-foreground">支持单机多地区并发出口</p>
          </CardContent>
        </Card>

        <Card className="border-border/60 bg-card/60 backdrop-blur">
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              出口真连通检测
            </CardTitle>
            <Wifi className="size-4 text-emerald-500" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold tracking-tight text-foreground">
              {selected?.status.lastConnectivity?.latencyMs
                ? `${selected.status.lastConnectivity.latencyMs} ms`
                : selected?.status.isRunning
                  ? "就绪待测"
                  : "--"}
            </div>
            <p className="mt-1 text-[11.5px] text-muted-foreground">
              {selected?.status.lastConnectivity?.ip
                ? `公网出口: ${selected.status.lastConnectivity.ip}`
                : "点击程序卡片测试穿透真实IP"}
            </p>
          </CardContent>
        </Card>

        <Card className="border-border/60 bg-card/60 backdrop-blur">
          <CardHeader className="flex flex-row items-center justify-between pb-2">
            <CardTitle className="text-xs font-semibold uppercase tracking-wider text-muted-foreground">
              实测带宽吞吐
            </CardTitle>
            <Zap className="size-4 text-amber-500" />
          </CardHeader>
          <CardContent>
            <div className="text-2xl font-bold tracking-tight text-foreground">
              {selected?.status.lastSpeed?.mbps ? `${selected.status.lastSpeed.mbps} Mbps` : "--"}
            </div>
            <p className="mt-1 text-[11.5px] text-muted-foreground">
              {selected?.status.lastSpeed ? `测试耗时 ${selected.status.lastSpeed.durationSecs} 秒` : "支持穿透测速"}
            </p>
          </CardContent>
        </Card>
      </div>

      {/* ── Program Switcher Section ─────────────────────────────────────── */}
      <div className="mt-6 flex items-center justify-between">
        <div>
          <h3 className="text-base font-semibold tracking-tight text-foreground">已配置 SOCKS5 程序</h3>
          <p className="text-xs text-muted-foreground">切换查看各端口实时运行状态、鉴权凭据及出口节点详情</p>
        </div>
        <Button variant="outline" size="sm" onClick={onManageClick} className="gap-1.5 text-xs">
          <Settings className="size-3.5" />
          管理所有端口
        </Button>
      </div>

      {/* ── Switcher Card Carousel / Grid ───────────────────────────────── */}
      <div className="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2 md:grid-cols-3 lg:grid-cols-4">
        {loading && instances.length === 0 ? (
          <div className="col-span-full py-12 flex flex-col items-center justify-center text-muted-foreground text-xs">
            <RefreshCw className="size-5 animate-spin mb-2 text-primary" />
            <span>正在连接 VPS 守护进程...</span>
          </div>
        ) : instances.map((inst) => {
          const isSelected = inst.id === selectedId;
          const isRunning = inst.status.isRunning;
          const hasAuth = Boolean(inst.username && inst.password);

          return (
            <div
              key={inst.id}
              role="button"
              tabIndex={0}
              onClick={() => setSelectedId(inst.id)}
              className={[
                "group relative flex flex-col justify-between rounded-xl border p-4 text-start transition-all cursor-pointer",
                isSelected
                  ? "border-primary/50 bg-primary/[0.04] shadow-md ring-1 ring-primary/30"
                  : "border-border/60 bg-card hover:border-primary/30 hover:bg-card/80",
              ].join(" ")}
            >
              <div>
                <div className="flex items-start justify-between gap-2">
                  <div className="flex items-center gap-2">
                    <span
                      className={[
                        "size-2 rounded-full",
                        isRunning ? "bg-emerald-500 shadow-[0_0_8px_rgba(16,185,129,0.7)] animate-pulse" : "bg-muted-foreground/40",
                      ].join(" ")}
                    />
                    <span className="font-semibold text-[13.5px] text-foreground truncate max-w-[140px]">
                      {inst.name}
                    </span>
                  </div>
                  <Badge variant={isRunning ? "ok" : "outline"} className="text-[10.5px] px-1.5 py-0">
                    {isRunning ? "监听中" : "已停止"}
                  </Badge>
                </div>

                <div className="mt-2.5 flex flex-wrap items-center gap-1.5">
                  <span className="rounded bg-muted/80 px-2 py-0.5 font-mono text-[11px] font-medium text-foreground">
                    :{inst.listenPort}
                  </span>
                  <span className="rounded bg-accent/60 px-1.5 py-0.5 text-[10.5px] text-muted-foreground uppercase font-medium">
                    {inst.upstreamType}
                    {inst.upstreamType === "warp"
                      ? ` (${(() => {
                        const p = aetherProfiles.find((x) => x.id === inst.upstreamConfig?.profileId);
                        return p ? p.name : inst.upstreamConfig?.profileId || "默认";
                      })()})`
                      : inst.upstreamConfig?.country
                        ? ` (${inst.upstreamConfig.country})`
                        : ""}
                  </span>
                  {hasAuth ? (
                    <span className="inline-flex items-center gap-1 rounded bg-amber-500/10 px-1.5 py-0.5 text-[10.5px] font-medium text-amber-500">
                      <Lock className="size-2.5" /> 鉴权
                    </span>
                  ) : (
                    <span className="inline-flex items-center gap-1 rounded bg-muted/50 px-1.5 py-0.5 text-[10.5px] text-muted-foreground">
                      <Unlock className="size-2.5" /> 免密
                    </span>
                  )}
                </div>
              </div>

              <div className="mt-3 flex items-center justify-between border-t border-border/40 pt-2 text-[11px] text-muted-foreground">
                <span className="truncate">
                  {inst.status.lastConnectivity?.country
                    ? `出口: ${inst.status.lastConnectivity.country}`
                    : getCountryHint(inst)}
                </span>
                <span className="font-mono text-primary/80 group-hover:underline">查看详情 &rarr;</span>
              </div>
            </div>
          );
        })}

        {/* Quick Add Card */}
        <div
          role="button"
          tabIndex={0}
          onClick={onManageClick}
          className="flex min-h-[108px] flex-col items-center justify-center rounded-xl border border-dashed border-border/80 bg-muted/20 p-4 text-center transition-colors hover:border-primary/50 hover:bg-muted/40 cursor-pointer"
        >
          <Plus className="size-5 text-muted-foreground mb-1" />
          <span className="text-xs font-semibold text-foreground">添加 SOCKS5 端口</span>
          <span className="text-[11px] text-muted-foreground">绑定新出口与鉴权</span>
        </div>
      </div>

      {/* ── Selected Program Detailed Inspector Card ───────────────────── */}
      {selected && (
        <Card className="mt-6 border-border/80 bg-card shadow-sm">
          <CardHeader className="border-b border-border/50 pb-4">
            <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <div>
                <div className="flex items-center gap-2.5">
                  <div
                    className={[
                      "flex size-9 items-center justify-center rounded-lg border",
                      selected.status.isRunning ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-500" : "border-border bg-muted text-muted-foreground",
                    ].join(" ")}
                  >
                    <Server className="size-5" />
                  </div>
                  <div>
                    <div className="flex items-center gap-2">
                      <CardTitle className="text-lg font-bold tracking-tight text-foreground">
                        {selected.name}
                      </CardTitle>
                      <Badge variant={selected.status.isRunning ? "ok" : "bad"}>
                        {selected.status.isRunning ? "已启动运行" : "未运行"}
                      </Badge>
                      {selected.autostart && (
                        <Badge variant="outline" className="border-primary/30 text-primary text-[10.5px]">
                          开机自启
                        </Badge>
                      )}
                    </div>
                    <CardDescription className="text-xs text-muted-foreground">
                      监听: <code>{selected.listenHost}:{selected.listenPort}</code> &middot; 上游出口:{" "}
                      <span className="font-semibold text-foreground capitalize">
                        {selected.upstreamType}
                        {selected.upstreamType === "warp" && (
                          <span className="font-normal text-muted-foreground">
                            {" "}· 方案: {(() => {
                              const p = aetherProfiles.find((x) => x.id === selected.upstreamConfig?.profileId);
                              return p ? p.name : selected.upstreamConfig?.profileId || "默认方案";
                            })()}
                          </span>
                        )}
                      </span>
                    </CardDescription>
                  </div>
                </div>
              </div>

              {/* Main Actions for this Program */}
              <div className="flex flex-wrap items-center gap-2">
                <Button
                  size="sm"
                  variant={selected.status.isRunning ? "destructive" : "default"}
                  disabled={actionLoading[selected.id]}
                  onClick={() => handleToggleInstance(selected.id, selected.status.isRunning)}
                  className="gap-1.5"
                >
                  {selected.status.isRunning ? (
                    <>
                      <Square className="size-3.5 fill-current" /> 停止程序
                    </>
                  ) : (
                    <>
                      <Play className="size-3.5 fill-current" /> 独立启动
                    </>
                  )}
                </Button>

                <Button
                  size="sm"
                  variant="outline"
                  disabled={!selected.status.isRunning || testingConn[selected.id]}
                  onClick={() => handleTestConnectivity(selected.id)}
                  className="gap-1.5 border-emerald-500/30 text-emerald-500 hover:bg-emerald-500/10"
                >
                  <RefreshCw className={["size-3.5", testingConn[selected.id] ? "animate-spin" : ""].join(" ")} />
                  {testingConn[selected.id] ? "探测真连通中..." : "测试真连通"}
                </Button>

                <Button
                  size="sm"
                  variant="outline"
                  disabled={!selected.status.isRunning || testingSpeed[selected.id]}
                  onClick={() => handleTestSpeed(selected.id)}
                  className="gap-1.5 border-amber-500/30 text-amber-500 hover:bg-amber-500/10"
                >
                  <Zap className={["size-3.5", testingSpeed[selected.id] ? "animate-spin" : ""].join(" ")} />
                  {testingSpeed[selected.id] ? "测速中 (5MB)..." : "测速"}
                </Button>
              </div>
            </div>
          </CardHeader>

          <CardContent className="pt-5">
            <div className="grid grid-cols-1 gap-5 md:grid-cols-2">
              {/* Left Column: Real connectivity & Speed result */}
              <div className="flex flex-col gap-4">
                <div className="rounded-xl border border-border/70 bg-card-subtle/50 p-4">
                  <div className="flex items-center justify-between mb-3">
                    <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground flex items-center gap-1.5">
                      <Globe className="size-3.5 text-blue-500" />
                      真实公网出口穿透状态
                    </span>
                    {selected.status.lastConnectivity && (
                      <span className="text-[11px] text-muted-foreground">
                        {formatTime(selected.status.lastConnectivity.checkedAt)}
                      </span>
                    )}
                  </div>

                  {selected.status.lastConnectivity ? (
                    <div className="space-y-2 text-xs">
                      <div className="flex justify-between py-1 border-b border-border/30">
                        <span className="text-muted-foreground">真实出口 IP:</span>
                        <div className="flex items-center gap-1 font-mono font-bold text-foreground">
                          {selected.status.lastConnectivity.ip || "未获取"}
                          {selected.status.lastConnectivity.ip && (
                            <button
                              type="button"
                              onClick={() => copyToClipboard(selected.status.lastConnectivity?.ip || "", "ip")}
                              className="text-muted-foreground hover:text-foreground"
                            >
                              <Copy className="size-3" />
                            </button>
                          )}
                        </div>
                      </div>

                      <div className="flex justify-between py-1 border-b border-border/30">
                        <span className="text-muted-foreground">出口国家 / 地区:</span>
                        <span className="font-semibold text-foreground">
                          {selected.status.lastConnectivity.country || "未知"}{" "}
                          {selected.status.lastConnectivity.colo ? `(${selected.status.lastConnectivity.colo})` : ""}
                        </span>
                      </div>

                      {selected.status.lastConnectivity.provider && (
                        <div className="flex justify-between py-1 border-b border-border/30">
                          <span className="text-muted-foreground">探测服务提供源:</span>
                          <span className="font-medium text-primary flex items-center gap-1">
                            <span className="inline-block w-1.5 h-1.5 rounded-full bg-emerald-500"></span>
                            {selected.status.lastConnectivity.provider}
                          </span>
                        </div>
                      )}

                      <div className="flex justify-between py-1 border-b border-border/30">
                        <span className="text-muted-foreground">真实握手延迟 RTT:</span>
                        <span
                          className={[
                            "font-bold font-mono",
                            (selected.status.lastConnectivity.latencyMs ?? 999) < 100
                              ? "text-emerald-500"
                              : (selected.status.lastConnectivity.latencyMs ?? 999) < 250
                                ? "text-amber-500"
                                : "text-red-500",
                          ].join(" ")}
                        >
                          {selected.status.lastConnectivity.latencyMs
                            ? `${selected.status.lastConnectivity.latencyMs} ms`
                            : "--"}
                        </span>
                      </div>

                      <div className="flex justify-between py-1">
                        <span className="text-muted-foreground">连通状态:</span>
                        <span className={selected.status.lastConnectivity.success ? "text-emerald-500 font-medium" : "text-destructive font-medium"}>
                          {selected.status.lastConnectivity.success ? "真连通成功 (Verified)" : `检测失败: ${selected.status.lastConnectivity.error}`}
                        </span>
                      </div>
                    </div>
                  ) : (
                    <div className="flex flex-col items-center justify-center py-6 text-center text-muted-foreground">
                      <Wifi className="size-7 stroke-[1.5] mb-2 opacity-50" />
                      <p className="text-xs">尚未进行真连通探测</p>
                      <p className="text-[11px] text-muted-foreground/70 mt-0.5">
                        点击上方“测试真连通”按钮验证此端口真实公网 IP 与时延
                      </p>
                    </div>
                  )}
                </div>

                {/* Speed test card */}
                <div className="rounded-xl border border-border/70 bg-card-subtle/50 p-4">
                  <div className="flex items-center justify-between mb-3">
                    <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground flex items-center gap-1.5">
                      <Zap className="size-3.5 text-amber-500" />
                      实际带宽测速
                    </span>
                    {selected.status.lastSpeed && (
                      <span className="text-[11px] text-muted-foreground">
                        {formatTime(selected.status.lastSpeed.testedAt)}
                      </span>
                    )}
                  </div>

                  {selected.status.lastSpeed ? (
                    <div className="space-y-2 text-xs">
                      <div className="flex justify-between py-1 border-b border-border/30">
                        <span className="text-muted-foreground">实测下载带宽:</span>
                        <span className="font-mono text-base font-bold text-amber-500">
                          {selected.status.lastSpeed.mbps ?? 0} Mbps
                        </span>
                      </div>
                      <div className="flex justify-between py-1 border-b border-border/30">
                        <span className="text-muted-foreground">下载测试数据量:</span>
                        <span className="font-mono text-foreground">
                          {((selected.status.lastSpeed.bytes || 0) / 1024 / 1024).toFixed(2)} MB
                        </span>
                      </div>
                      <div className="flex justify-between py-1">
                        <span className="text-muted-foreground">测试耗时:</span>
                        <span className="font-mono text-foreground">{selected.status.lastSpeed.durationSecs} 秒</span>
                      </div>
                    </div>
                  ) : (
                    <div className="flex flex-col items-center justify-center py-6 text-center text-muted-foreground">
                      <Zap className="size-7 stroke-[1.5] mb-2 opacity-50" />
                      <p className="text-xs">暂无测速记录</p>
                      <p className="text-[11px] text-muted-foreground/70 mt-0.5">
                        点击上方“测速”下载 5MB 数据包评估实时出口速率
                      </p>
                    </div>
                  )}
                </div>
              </div>

              {/* Right Column: Connection Strings & Quick Usage */}
              <div className="flex flex-col gap-4">
                <div className="rounded-xl border border-border/70 bg-card-subtle/50 p-4">
                  <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground flex items-center gap-1.5 mb-3">
                    <KeyRound className="size-3.5 text-primary" />
                    鉴权与参数
                  </span>

                  <div className="space-y-2.5 text-xs">
                    <div className="flex items-center justify-between">
                      <span className="text-muted-foreground">认证状态:</span>
                      {selected.username && selected.password ? (
                        <span className="inline-flex items-center gap-1 text-amber-500 font-semibold">
                          <Lock className="size-3" /> 需要用户名/密码鉴权
                        </span>
                      ) : (
                        <span className="inline-flex items-center gap-1 text-emerald-500 font-semibold">
                          <Unlock className="size-3" /> 免密开放连接
                        </span>
                      )}
                    </div>

                    {selected.username && selected.password && (
                      <>
                        <div className="flex items-center justify-between font-mono bg-muted/40 px-2.5 py-1.5 rounded">
                          <span className="text-muted-foreground text-[11px]">用户名:</span>
                          <span className="text-foreground font-semibold">{selected.username}</span>
                        </div>
                        <div className="flex items-center justify-between font-mono bg-muted/40 px-2.5 py-1.5 rounded">
                          <span className="text-muted-foreground text-[11px]">密　码:</span>
                          <span className="text-foreground font-semibold">{selected.password}</span>
                        </div>
                      </>
                    )}

                    <div className="flex items-center justify-between">
                      <span className="text-muted-foreground">开机自启:</span>
                      <span className="font-medium text-foreground">{selected.autostart ? "已开启" : "未开启"}</span>
                    </div>

                    <div className="flex items-center justify-between">
                      <span className="text-muted-foreground">绑定上游参数:</span>
                      <span className="font-mono text-muted-foreground truncate max-w-[200px]">
                        {JSON.stringify(selected.upstreamConfig)}
                      </span>
                    </div>
                  </div>
                </div>

                {/* Quick Client Export */}
                <div className="rounded-xl border border-border/70 bg-card-subtle/50 p-4">
                  <span className="text-xs font-semibold uppercase tracking-wider text-muted-foreground flex items-center gap-1.5 mb-3">
                    <Copy className="size-3.5 text-primary" />
                    客户端配置与命令
                  </span>

                  <div className="space-y-3">
                    <div>
                      <div className="flex items-center justify-between text-[11.5px] mb-1">
                        <span className="font-medium text-muted-foreground">SOCKS5 代理地址:</span>
                        <button
                          type="button"
                          onClick={() => {
                            const creds = selected.username && selected.password ? `${selected.username}:${selected.password}@` : "";
                            copyToClipboard(`socks5://${creds}${vpsHost}:${selected.listenPort}`, "socks5-url");
                          }}
                          className="text-primary hover:underline flex items-center gap-1"
                        >
                          {copiedKey === "socks5-url" ? <Check className="size-3" /> : <Copy className="size-3" />}
                          复制
                        </button>
                      </div>
                      <div className="rounded bg-muted/60 p-2 font-mono text-[11.5px] break-all text-foreground select-all">
                        socks5://
                        {selected.username && selected.password ? `${selected.username}:${selected.password}@` : ""}
                        {vpsHost}:{selected.listenPort}
                      </div>
                    </div>

                    <div>
                      <div className="flex flex-wrap items-center justify-between gap-1 text-[11.5px] mb-1.5">
                        <span className="font-medium text-muted-foreground">cURL 代理测试命令:</span>
                        <div className="flex items-center gap-1.5">
                          <select
                            value={curlTarget}
                            onChange={(e) => setCurlTarget(e.target.value)}
                            className="bg-muted text-foreground border border-border/60 rounded px-1.5 py-0.5 text-[11px] focus:outline-none focus:ring-1 focus:ring-primary"
                          >
                            {CURL_TARGETS.map((t) => (
                              <option key={t.url} value={t.url}>
                                {t.label}
                              </option>
                            ))}
                          </select>
                          <button
                            type="button"
                            onClick={() => {
                              const creds = selected.username && selected.password ? `${selected.username}:${selected.password}@` : "";
                              copyToClipboard(`curl -x socks5h://${creds}${vpsHost}:${selected.listenPort} ${curlTarget}`, "curl-cmd");
                            }}
                            className="text-primary hover:underline flex items-center gap-1"
                          >
                            {copiedKey === "curl-cmd" ? <Check className="size-3" /> : <Copy className="size-3" />}
                            复制
                          </button>
                        </div>
                      </div>
                      <div className="rounded bg-muted/60 p-2 font-mono text-[11.5px] break-all text-foreground select-all">
                        curl -x socks5h://
                        {selected.username && selected.password ? `${selected.username}:${selected.password}@` : ""}
                        {vpsHost}:{selected.listenPort} {curlTarget}
                      </div>
                      {curlTarget.includes("ipinfo.io") && (
                        <p className="text-[10.5px] text-amber-500/90 mt-1 leading-normal">
                          提示: ipinfo.io 免费查询接口极易触发 HTTP 429 频繁限制，建议选用 IPWho.is 或 IP.SB。
                        </p>
                      )}
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </CardContent>
        </Card>
      )}
    </div>
  );
}

function getCountryHint(inst: SocksInstanceView, aetherProfiles?: AetherProfileConfig[]): string {
  if (inst.upstreamConfig?.country) {
    return inst.upstreamConfig.country.toUpperCase();
  }
  if (inst.upstreamConfig?.region) {
    return inst.upstreamConfig.region.toUpperCase();
  }
  if (inst.upstreamType === "warp") {
    const p = aetherProfiles?.find((x) => x.id === inst.upstreamConfig?.profileId);
    return `Warp (${p ? p.name : "默认方案"})`;
  }
  return "自动出口";
}

function formatTime(ts: number): string {
  if (!ts) return "";
  const d = new Date(ts * 1000);
  return d.toLocaleTimeString();
}
