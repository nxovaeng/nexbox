import { useEffect, useState, useCallback } from "react";
import {
  Key, RefreshCw, Play, Square, Settings, Server, Globe,
  CheckCircle2, AlertCircle, Clock, Zap, Smartphone, Check, Loader2
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import {
  getProtonInfo,
  loginProtonGuest,
  renewProtonCert,
  refreshProtonServers,
  saveProtonConfig,
  startProton,
  stopProton,
  type ProtonInfo,
} from "@/core/api";

const COUNTRY_NAMES: Record<string, string> = {
  US: "美国 (United States)",
  NL: "荷兰 (Netherlands)",
  JP: "日本 (Japan)",
  CH: "瑞士 (Switzerland)",
  RO: "罗马尼亚 (Romania)",
  PL: "波兰 (Poland)",
  MX: "墨西哥 (Mexico)",
  SG: "新加坡 (Singapore)",
  CA: "加拿大 (Canada)",
  NO: "挪威 (Norway)",
  DE: "德国 (Germany)",
  FR: "法国 (France)",
  GB: "英国 (United Kingdom)",
};

export function ProtonManager() {
  const [info, setInfo] = useState<ProtonInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [actionLoading, setActionLoading] = useState<string | null>(null);
  const [successMsg, setSuccessMsg] = useState<string | null>(null);

  // Standalone settings local state
  const [listenAddress, setListenAddress] = useState("127.0.0.1");
  const [listenPort, setListenPort] = useState(10810);
  const [selectedCountry, setSelectedCountry] = useState("US");
  const [selectedServer, setSelectedServer] = useState("");
  const [autoFailover, setAutoFailover] = useState(true);

  const fetchInfo = useCallback(async () => {
    try {
      const res = await getProtonInfo();
      setInfo(res);
      setListenAddress(res.settings.listenAddress || "127.0.0.1");
      setListenPort(res.settings.listenPort || 10810);
      setSelectedCountry(res.settings.country || "US");
      setSelectedServer(res.settings.serverName || "");
      setAutoFailover(res.settings.autoFailover ?? true);
      setError(null);
    } catch (e: any) {
      setError(e.message || "无法获取 Proton 状态");
    } finally {
      setLoading(false);
    }
  }, []);

  const handleSelectCountry = (countryCode: string) => {
    setSelectedCountry(countryCode);
    if (selectedServer && countryCode) {
      const s = info?.servers?.find((item) => item.name === selectedServer);
      if (s && s.country !== countryCode) {
        setSelectedServer("");
      }
    }
  };

  useEffect(() => {
    fetchInfo();
    const timer = setInterval(fetchInfo, 5000);
    return () => clearInterval(timer);
  }, [fetchInfo]);

  const showSuccess = (msg: string) => {
    setSuccessMsg(msg);
    setTimeout(() => setSuccessMsg(null), 4000);
  };

  const handleGuestLogin = async () => {
    setActionLoading("guest_login");
    setError(null);
    try {
      const res = await loginProtonGuest();
      showSuccess(`Guest 访客登录成功！已自动申请 7 天证书 (UID: ${res.uid?.slice(0, 10)}...)`);
      await fetchInfo();
    } catch (e: any) {
      setError(e.message || "Guest 登录失败");
    } finally {
      setActionLoading(null);
    }
  };

  const handleRenewCert = async () => {
    setActionLoading("renew_cert");
    setError(null);
    try {
      await renewProtonCert();
      showSuccess("7 天长效证书 (Duration: 10080 min) 重新签发成功！");
      await fetchInfo();
    } catch (e: any) {
      setError(e.message || "证书申请失败");
    } finally {
      setActionLoading(null);
    }
  };

  const handleRefreshServers = async () => {
    setActionLoading("refresh_servers");
    setError(null);
    try {
      const res = await refreshProtonServers();
      showSuccess(`成功从 Proton 官方同步 ${res.count} 个节点与最新负载！`);
      await fetchInfo();
    } catch (e: any) {
      setError(e.message || "同步节点失败");
    } finally {
      setActionLoading(null);
    }
  };

  const handleSaveConfig = async () => {
    setActionLoading("save_config");
    setError(null);
    try {
      await saveProtonConfig({
        listenAddress,
        listenPort,
        country: selectedCountry || undefined,
        serverName: selectedServer || undefined,
        autoFailover,
      });
      showSuccess("Proton 独立启动配置已成功保存！");
      await fetchInfo();
    } catch (e: any) {
      setError(e.message || "保存配置失败");
    } finally {
      setActionLoading(null);
    }
  };

  const handleStart = async () => {
    setActionLoading("start");
    setError(null);
    try {
      const res = await startProton({
        listenAddress,
        listenPort,
        country: selectedCountry || undefined,
        serverName: selectedServer || undefined,
      });
      showSuccess(`WireProxy 已成功启动！SOCKS5 监听于 ${res.address || `${listenAddress}:${listenPort}`}`);
      await fetchInfo();
    } catch (e: any) {
      setError(e.message || "启动 WireProxy 失败");
    } finally {
      setActionLoading(null);
    }
  };

  const handleStop = async () => {
    setActionLoading("stop");
    setError(null);
    try {
      await stopProton();
      showSuccess("WireProxy 代理已停止");
      await fetchInfo();
    } catch (e: any) {
      setError(e.message || "停止 WireProxy 失败");
    } finally {
      setActionLoading(null);
    }
  };

  if (loading && !info) {
    return (
      <div className="p-8 text-center text-sm text-muted-foreground flex items-center justify-center gap-2">
        <Loader2 className="w-4 h-4 animate-spin" />
        正在加载 Proton / Wireproxy 配置...
      </div>
    );
  }

  const certExp = info?.certExpiresAt;
  const certDays = info?.certDaysRemaining;
  const expDateStr = certExp ? new Date(certExp * 1000).toLocaleString() : null;

  const countryServers = (info?.servers || []).filter(
    (s) => !selectedCountry || s.country === selectedCountry
  );

  return (
    <div className="space-y-6">
      {/* 顶部全局提示 */}
      {error && (
        <div className="p-3 bg-red-500/10 border border-red-500/20 text-red-500 rounded-md text-sm flex items-center gap-2">
          <AlertCircle className="w-4 h-4 shrink-0" />
          <span>{error}</span>
        </div>
      )}
      {successMsg && (
        <div className="p-3 bg-emerald-500/10 border border-emerald-500/20 text-emerald-500 rounded-md text-sm flex items-center gap-2">
          <CheckCircle2 className="w-4 h-4 shrink-0" />
          <span>{successMsg}</span>
        </div>
      )}

      {/* 顶部状态总览面板 */}
      <Card className="border-border/60 shadow-sm bg-gradient-to-r from-card to-card/60">
        <CardContent className="p-4 grid grid-cols-2 sm:grid-cols-4 gap-4">
          <div className="flex flex-col gap-1">
            <span className="text-xs text-muted-foreground flex items-center gap-1.5">
              <Server className="w-3.5 h-3.5 text-blue-500" />
              WireProxy 内核
            </span>
            <div className="flex items-center gap-2 mt-0.5">
              {info?.wireproxyInstalled ? (
                <Badge variant="outline" className="bg-emerald-500/10 text-emerald-500 border-emerald-500/20 font-mono text-xs">
                  <Check className="w-3 h-3 mr-1" /> 已就绪
                </Badge>
              ) : (
                <Badge variant="destructive" className="text-xs">未安装</Badge>
              )}
            </div>
          </div>

          <div className="flex flex-col gap-1">
            <span className="text-xs text-muted-foreground flex items-center gap-1.5">
              <Smartphone className="w-3.5 h-3.5 text-purple-500" />
              会话与用户层级
            </span>
            <div className="flex items-center gap-2 mt-0.5">
              {info?.sessionActive ? (
                <Badge variant="outline" className="bg-emerald-500/10 text-emerald-500 border-emerald-500/20 text-xs">
                  {info.userTier === 0 ? "Free 免费层级 (Tier 0)" : `Plus 付费层级 (Tier ${info.userTier})`}
                </Badge>
              ) : (
                <Badge variant="outline" className="text-muted-foreground text-xs">未登录</Badge>
              )}
            </div>
          </div>

          <div className="flex flex-col gap-1">
            <span className="text-xs text-muted-foreground flex items-center gap-1.5">
              <Clock className="w-3.5 h-3.5 text-amber-500" />
              客户端证书状态
            </span>
            <div className="flex items-center gap-2 mt-0.5">
              {certDays !== null && certDays !== undefined && certDays > 0 ? (
                <Badge variant="outline" className="bg-amber-500/10 text-amber-500 border-amber-500/20 text-xs font-mono">
                  {certDays > 1.0 ? `剩余 ${certDays.toFixed(1)} 天` : `剩余 ${(certDays * 24).toFixed(1)} 小时`}
                </Badge>
              ) : (
                <Badge variant="outline" className="text-red-500 border-red-500/20 text-xs">已过期/未申请</Badge>
              )}
            </div>
          </div>

          <div className="flex flex-col gap-1">
            <span className="text-xs text-muted-foreground flex items-center gap-1.5">
              <Zap className="w-3.5 h-3.5 text-emerald-500" />
              SOCKS5 服务
            </span>
            <div className="flex items-center gap-2 mt-0.5">
              {info?.isRunning ? (
                <Badge className="bg-emerald-600 text-white font-mono text-xs animate-pulse">
                  运行中 : {info.activePort || listenPort}
                </Badge>
              ) : (
                <Badge variant="secondary" className="text-xs text-muted-foreground">空闲已停止</Badge>
              )}
            </div>
          </div>
        </CardContent>

        {info?.isRunning && (
          <div className="px-4 pb-3 pt-1 border-t border-border/40 text-xs flex items-center justify-between text-muted-foreground">
            <div className="flex items-center gap-2">
              <span className="font-medium text-foreground">出口节点:</span>
              <span className="font-mono text-emerald-500">{info.activeServer || "自动选择"}</span>
              <span className="text-border">|</span>
              <span>地区:</span>
              <span className="font-semibold text-foreground">{info.activeCountry || "US"}</span>
            </div>
            <div className="font-mono text-[11px] text-muted-foreground/80">
              本地代理: socks5://{info.activeAddress || listenAddress}:{info.activePort || listenPort}
            </div>
          </div>
        )}
      </Card>

      {/* 分组一：公用基础配置 */}
      <Card className="border-border/60 shadow-sm">
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <div className="space-y-1">
              <CardTitle className="text-base font-semibold flex items-center gap-2">
                <Key className="w-4 h-4 text-primary" />
                公用基础配置 (Common & Authentication)
              </CardTitle>
              <CardDescription className="text-xs">
                模拟 Google Pixel 9 (Android 14) 设备指纹与 Proton 官方 API 交互，自动维护认证凭证与层级节点列表。
              </CardDescription>
            </div>
          </div>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {/* 模拟设备指纹 */}
            <div className="p-3 bg-muted/20 border border-border/40 rounded-lg space-y-2">
              <div className="flex items-center justify-between">
                <span className="text-xs font-semibold flex items-center gap-1.5">
                  <Smartphone className="w-3.5 h-3.5 text-purple-500" />
                  模拟客户端环境
                </span>
                <Badge variant="secondary" className="text-[10px] font-mono">Google Pixel 9</Badge>
              </div>
              <p className="text-xs text-muted-foreground leading-relaxed">
                使用 Android 14 专属挑战载荷 <code>vpn-android-v4-challenge-0</code>，根据宿主机特征计算唯一哈希指纹，无缝兼容 Proton 官方客户端风控体系。
              </p>
              <div className="pt-1 flex items-center gap-2">
                <Button
                  size="sm"
                  variant="outline"
                  className="h-7 text-xs gap-1.5"
                  onClick={handleGuestLogin}
                  disabled={actionLoading !== null}
                >
                  {actionLoading === "guest_login" ? (
                    <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  ) : (
                    <Key className="w-3.5 h-3.5 text-primary" />
                  )}
                  {info?.sessionActive ? "重新获取 Guest 访客会话" : "一键 Guest 免密登录"}
                </Button>
              </div>
            </div>

            {/* 证书与有效期 */}
            <div className="p-3 bg-muted/20 border border-border/40 rounded-lg space-y-2">
              <div className="flex items-center justify-between">
                <span className="text-xs font-semibold flex items-center gap-1.5">
                  <Clock className="w-3.5 h-3.5 text-amber-500" />
                  WireGuard 客户端证书
                </span>
                <span className="text-[11px] text-muted-foreground font-mono">
                  {certDays !== null && certDays !== undefined && certDays > 0
                    ? (certDays > 1.0 ? `剩余 ${certDays.toFixed(1)} 天` : `剩余 ${(certDays * 24).toFixed(1)} 小时`)
                    : "未激活"}
                </span>
              </div>
              <p className="text-xs text-muted-foreground leading-relaxed">
                {expDateStr ? (
                  <>证书有效期至：<strong className="text-foreground">{expDateStr}</strong>（后台将在到期前 1 小时自动静默续签）</>
                ) : (
                  "当前未签发有效证书，点击下方按钮申请客户端证书凭据。"
                )}
              </p>
              <div className="pt-1 flex items-center gap-2">
                <Button
                  size="sm"
                  variant="outline"
                  className="h-7 text-xs gap-1.5"
                  onClick={handleRenewCert}
                  disabled={actionLoading !== null || !info?.sessionActive}
                >
                  {actionLoading === "renew_cert" ? (
                    <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  ) : (
                    <RefreshCw className="w-3.5 h-3.5 text-amber-500" />
                  )}
                  申请 / 续签客户端证书
                </Button>
              </div>
            </div>
          </div>

          <Separator className="my-2" />

          {/* 节点列表与负载同步 */}
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 p-3 bg-muted/10 rounded-lg border border-border/30">
            <div className="space-y-0.5">
              <div className="text-xs font-medium flex items-center gap-1.5">
                <Globe className="w-3.5 h-3.5 text-blue-500" />
                <span>节点缓存数据 (data/proton/servers.json)</span>
              </div>
              <div className="text-[11px] text-muted-foreground">
                当前层级可用节点 <strong className="text-foreground font-mono">{info?.totalServers || 0}</strong> 个，涵盖 <strong className="text-foreground font-mono">{info?.countries?.length || 0}</strong> 个国家/地区。
              </div>
            </div>
            <Button
              size="sm"
              variant="secondary"
              className="h-8 text-xs gap-1.5 self-start sm:self-auto"
              onClick={handleRefreshServers}
              disabled={actionLoading !== null || !info?.sessionActive}
            >
              {actionLoading === "refresh_servers" ? (
                <Loader2 className="w-3.5 h-3.5 animate-spin" />
              ) : (
                <RefreshCw className="w-3.5 h-3.5 text-blue-500" />
              )}
              同步最新官方节点与负载
            </Button>
          </div>
        </CardContent>
      </Card>

      {/* 分组二：独立启动参数 */}
      <Card className="border-border/60 shadow-sm">
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <div className="space-y-1">
              <CardTitle className="text-base font-semibold flex items-center gap-2">
                <Settings className="w-4 h-4 text-emerald-500" />
                独立启动参数 (wire_proton.conf)
              </CardTitle>
              <CardDescription className="text-xs">
                配置独立运行的 WireProxy SOCKS5 代理参数，供局域网设备、系统代理或浏览器直连。
              </CardDescription>
            </div>
            <span className="font-mono text-[11px] text-muted-foreground">data/proton/wire_proton.conf</span>
          </div>
        </CardHeader>
        <CardContent className="space-y-5">
          {/* 出口国家/地区选择（保留免费级全部国家） */}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <label className="text-xs font-medium flex items-center gap-1.5">
                <Globe className="w-3.5 h-3.5 text-primary" />
                第一步：选择出口国家 (已保留全部免费级可用国家)
              </label>
              <span className="text-[11px] text-muted-foreground">
                切换国家无需重签证书，秒级切换
              </span>
            </div>

            {/* 所有免费层级国家按钮网格 */}
            <div className="grid grid-cols-2 sm:grid-cols-5 gap-2">
              {(info?.countries && info.countries.length > 0
                ? info.countries
                : [
                    { code: "US", count: 0, lowestLoad: 20 },
                    { code: "NL", count: 0, lowestLoad: 15 },
                    { code: "JP", count: 0, lowestLoad: 25 },
                    { code: "RO", count: 0, lowestLoad: 18 },
                    { code: "PL", count: 0, lowestLoad: 30 },
                  ]
              ).map((c) => {
                const cc = c.code;
                const count = c.count;
                const load = c.lowestLoad;
                const isSelected = selectedCountry === cc;
                return (
                  <button
                    key={cc}
                    type="button"
                    onClick={() => handleSelectCountry(cc)}
                    className={`p-2 rounded-md border text-left transition-all ${
                      isSelected
                        ? "border-primary bg-primary/10 text-primary shadow-sm"
                        : "border-border/50 hover:bg-muted/40 text-muted-foreground"
                    }`}
                  >
                    <div className="text-xs font-semibold flex items-center justify-between">
                      <span>{cc}</span>
                      {load !== undefined && (
                        <span className={`text-[10px] font-mono px-1 rounded ${load < 50 ? "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400" : "bg-muted/60"}`}>
                          负载 {load}%
                        </span>
                      )}
                    </div>
                    <div className="text-[11px] truncate mt-0.5 font-medium">
                      {COUNTRY_NAMES[cc]?.split(" ")[0] || cc}
                    </div>
                    <div className="text-[10px] text-muted-foreground/80 mt-0.5">
                      {count} 个可用节点
                    </div>
                  </button>
                );
              })}
            </div>

            <div className="flex items-center gap-3 pt-1">
              <select
                className="h-8 rounded-md border border-input bg-background px-3 py-1 text-xs shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                value={selectedCountry}
                onChange={(e) => handleSelectCountry(e.target.value)}
              >
                <option value="">自动选择最佳国家 (全局最低负载)</option>
                {info?.countries.map((c) => (
                  <option key={c.code} value={c.code}>
                    {c.code} - {COUNTRY_NAMES[c.code] || c.code} ({c.count} 节点, 最低负载 {c.lowestLoad}%)
                  </option>
                ))}
              </select>
              <span className="text-[11px] text-muted-foreground">
                可从所有免费国家中任意指定，下方节点列表将联动筛选
              </span>
            </div>
          </div>

          <Separator className="my-1" />

          {/* 出口节点选择（级联筛选与自动选择） */}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <label className="text-xs font-medium flex items-center gap-1.5">
                <Server className="w-3.5 h-3.5 text-blue-500" />
                第二步：选择节点 (Node - 可自动选)
              </label>
              <span className="text-[11px] text-muted-foreground font-mono">
                {selectedCountry
                  ? `${COUNTRY_NAMES[selectedCountry]?.split(" ")[0] || selectedCountry} 筛选出 ${countryServers.length} 个可用节点`
                  : `全部国家共 ${info?.totalServers || 0} 个节点`}
              </span>
            </div>

            <div className="flex flex-col sm:flex-row items-stretch sm:items-center gap-2">
              <select
                className="h-8 flex-1 rounded-md border border-input bg-background px-3 py-1 text-xs shadow-sm focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring font-mono"
                value={selectedServer}
                onChange={(e) => setSelectedServer(e.target.value)}
              >
                <option value="">
                  ★ 自动选择最佳节点 ({selectedCountry ? `${selectedCountry} 最低负载` : "全局最低负载"})
                </option>
                {countryServers.map((s) => (
                  <option key={s.name} value={s.name}>
                    {s.name} - 负载 {s.load}% {s.city ? `(${s.city})` : ""}
                  </option>
                ))}
              </select>

              {selectedServer && (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="h-8 text-xs text-muted-foreground hover:text-foreground shrink-0"
                  onClick={() => setSelectedServer("")}
                >
                  恢复自动选节点
                </Button>
              )}
            </div>

            <div className="text-[11px] text-muted-foreground bg-muted/20 p-2.5 rounded border border-border/30">
              {selectedServer ? (
                <div>
                  已锁定指定节点：<strong className="text-foreground font-mono">{selectedServer}</strong>
                  {autoFailover && "（已开启断流自动故障转移：若该节点临时故障将自动平滑切回本国其他低负载节点）"}
                </div>
              ) : (
                <div>
                  当前处于 <strong className="text-emerald-500">自动选节点模式</strong>：启动时将从{" "}
                  <strong className="text-foreground">
                    {selectedCountry ? COUNTRY_NAMES[selectedCountry]?.split(" ")[0] || selectedCountry : "全部国家"}
                  </strong>{" "}
                  中自动选取实时负载最低的节点。
                </div>
              )}
            </div>
          </div>

          <Separator className="my-2" />

          {/* 监听地址与端口 */}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <div className="space-y-1.5">
              <div className="flex items-center justify-between">
                <label className="text-xs font-medium">监听地址 (Listen Address)</label>
                <div className="flex items-center gap-1 text-[11px]">
                  <button
                    type="button"
                    onClick={() => setListenAddress("127.0.0.1")}
                    className={`px-1.5 py-0.5 rounded text-[10px] transition-colors ${
                      listenAddress === "127.0.0.1"
                        ? "bg-primary text-primary-foreground font-semibold"
                        : "text-muted-foreground hover:text-foreground"
                    }`}
                  >
                    127.0.0.1 (本机)
                  </button>
                  <span className="text-border">|</span>
                  <button
                    type="button"
                    onClick={() => setListenAddress("0.0.0.0")}
                    className={`px-1.5 py-0.5 rounded text-[10px] transition-colors ${
                      listenAddress === "0.0.0.0"
                        ? "bg-primary text-primary-foreground font-semibold"
                        : "text-muted-foreground hover:text-foreground"
                    }`}
                  >
                    0.0.0.0 (局域网)
                  </button>
                </div>
              </div>
              <Input
                value={listenAddress}
                onChange={(e) => setListenAddress(e.target.value)}
                placeholder="127.0.0.1"
                className="font-mono text-xs h-8"
              />
            </div>

            <div className="space-y-1.5">
              <label className="text-xs font-medium">监听端口 (Listen Port)</label>
              <Input
                type="number"
                value={listenPort}
                onChange={(e) => setListenPort(parseInt(e.target.value, 10) || 10810)}
                placeholder="10810"
                className="font-mono text-xs h-8"
              />
            </div>
          </div>

          {/* 自动故障转移 */}
          <div className="flex items-center justify-between p-3 rounded-lg border border-border/30 bg-muted/10">
            <div className="space-y-0.5">
              <div className="text-xs font-medium">断流自动故障转移 (Auto Failover)</div>
              <div className="text-[11px] text-muted-foreground">
                当前节点异常或超时时，自动平滑切换同国家/地区的其他低负载节点。
              </div>
            </div>
            <Switch
              checked={autoFailover}
              onCheckedChange={setAutoFailover}
            />
          </div>

          {/* 底部按钮控制区 */}
          <div className="pt-2 flex flex-col sm:flex-row items-center justify-between gap-3 border-t border-border/40">
            <div className="text-xs text-muted-foreground">
              {info?.isRunning ? (
                <span className="flex items-center gap-1.5 text-emerald-500 font-medium">
                  <span className="w-2 h-2 rounded-full bg-emerald-500 animate-ping" />
                  SOCKS5 代理服务已激活
                </span>
              ) : (
                "保存参数后可一键启动 WireProxy 守护进程。"
              )}
            </div>

            <div className="flex items-center gap-2 w-full sm:w-auto">
              <Button
                variant="outline"
                size="sm"
                onClick={handleSaveConfig}
                disabled={actionLoading !== null}
                className="h-8 text-xs flex-1 sm:flex-initial"
              >
                {actionLoading === "save_config" && <Loader2 className="w-3.5 h-3.5 animate-spin mr-1" />}
                保存全部配置
              </Button>

              {info?.isRunning ? (
                <Button
                  variant="destructive"
                  size="sm"
                  onClick={handleStop}
                  disabled={actionLoading !== null}
                  className="h-8 text-xs flex-1 sm:flex-initial gap-1.5"
                >
                  {actionLoading === "stop" ? (
                    <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  ) : (
                    <Square className="w-3.5 h-3.5" />
                  )}
                  停止 WireProxy
                </Button>
              ) : (
                <Button
                  size="sm"
                  onClick={handleStart}
                  disabled={actionLoading !== null || !info?.sessionActive || !info?.wireproxyInstalled}
                  className="h-8 text-xs flex-1 sm:flex-initial gap-1.5 bg-emerald-600 hover:bg-emerald-700 text-white"
                >
                  {actionLoading === "start" ? (
                    <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  ) : (
                    <Play className="w-3.5 h-3.5 fill-current" />
                  )}
                  独立启动 WireProxy
                </Button>
              )}
            </div>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
