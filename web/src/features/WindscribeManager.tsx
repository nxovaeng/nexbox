import { useEffect, useState, useMemo, useRef } from "react";
import {
  CheckCircle2,
  XCircle,
  AlertCircle,
  RefreshCw,
  Globe,
  Play,
  Square,
  Server,
  Network,
  Save,
  Shield,
  User,
  Mail,
  Lock,
  ArrowRight,
} from "lucide-react";
import {
  getWindscribeStatus,
  loginWindscribe,
  registerWindscribe,
  refreshWindscribe,
  saveWindscribeConfig,
  startWindscribe,
  stopWindscribe,
} from "@/core/api";
import type {
  WindscribeStatusResponse,
} from "@/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";

export function WindscribeManager() {
  const [data, setData] = useState<WindscribeStatusResponse | null>(null);
  const [actionLoading, setActionLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  // 账号表单
  const [authTab, setAuthTab] = useState<"login" | "register">("login");
  const [loginUser, setLoginUser] = useState("");
  const [loginPass, setLoginPass] = useState("");
  const [regEmail, setRegEmail] = useState("");
  const [upstreamProxy, setUpstreamProxy] = useState<string>(() => {
    try {
      return localStorage.getItem("windscribe_upstream_proxy") || "";
    } catch {
      return "";
    }
  });

  // 独立配置
  const [listenAddress, setListenAddress] = useState("127.0.0.1");
  const [listenPort, setListenPort] = useState<number>(10809);
  const [selectedCountry, setSelectedCountry] = useState<string>("");
  const [selectedServerTag, setSelectedServerTag] = useState<string>("");
  const [savingConfig, setSavingConfig] = useState(false);

  const isInitializedRef = useRef(false);

  const handleProxyChange = (val: string) => {
    setUpstreamProxy(val);
    try {
      localStorage.setItem("windscribe_upstream_proxy", val);
    } catch {
      // ignore
    }
  };

  const fetchStatus = async () => {
    try {
      const res = await getWindscribeStatus();
      setData(res);
      if (!isInitializedRef.current && res.settings) {
        isInitializedRef.current = true;
        if (res.settings.listenAddress) setListenAddress(res.settings.listenAddress);
        if (res.settings.listenPort) setListenPort(res.settings.listenPort);
        if (res.settings.upstreamProxy) {
          setUpstreamProxy(res.settings.upstreamProxy);
          try {
            localStorage.setItem("windscribe_upstream_proxy", res.settings.upstreamProxy);
          } catch {}
        }
        if (res.settings.country) setSelectedCountry(res.settings.country);
        if (res.settings.serverTag) setSelectedServerTag(res.settings.serverTag);
      }
    } catch {
      // ignore
    }
  };

  useEffect(() => {
    void fetchStatus();
    const timer = setInterval(() => {
      void fetchStatus();
    }, 3000);
    return () => clearInterval(timer);
  }, []);

  const snapshot = data?.snapshot;
  const account = snapshot?.account;
  const isRunning = snapshot?.isRunning || snapshot?.state === "connected";
  const servers = snapshot?.servers || [];

  // 按国家分组
  const countryList = useMemo(() => {
    const map = new Map<string, { loc: string; locName: string; count: number }>();
    for (const s of servers) {
      if (!map.has(s.loc)) {
        map.set(s.loc, { loc: s.loc, locName: s.locName, count: 0 });
      }
      map.get(s.loc)!.count += 1;
    }
    return Array.from(map.values());
  }, [servers]);

  // 当前选中国家的节点
  const filteredServers = useMemo(() => {
    if (!selectedCountry) return servers;
    return servers.filter((s) => s.loc === selectedCountry);
  }, [servers, selectedCountry]);

  // 格式化流量
  const formatQuota = (usedBytes: number, maxBytes: number) => {
    const usedGb = (usedBytes / 1073741824).toFixed(2);
    const maxGb = (maxBytes / 1073741824).toFixed(2);
    const percent = maxBytes > 0 ? Math.min(100, Math.round((usedBytes / maxBytes) * 100)) : 0;
    return { usedGb, maxGb, percent };
  };

  function parseError(e: any): string {
    if (!e) return "未知错误";
    if (typeof e === "string") return e;
    if (e.error && typeof e.error === "string") return e.error;
    if (e.errorMessage && typeof e.errorMessage === "string") return e.errorMessage;
    if (e.message && typeof e.message === "string") return e.message;
    return JSON.stringify(e);
  }

  const handleLogin = async () => {
    if (!loginUser.trim() || !loginPass.trim()) {
      setError("请输入用户名和密码");
      return;
    }
    setError(null);
    setMsg(null);
    setActionLoading(true);
    try {
      const res = await loginWindscribe({
        username: loginUser.trim(),
        password: loginPass.trim(),
        upstreamProxy: upstreamProxy.trim() || undefined,
      });
      if (!res.success) {
        throw new Error(res.error || "登录失败");
      }
      setMsg("登录成功，已载入节点凭证与服务器列表");
      setLoginPass("");
      await fetchStatus();
    } catch (e: any) {
      setError(parseError(e));
    } finally {
      setActionLoading(false);
    }
  };

  const handleRegister = async () => {
    setError(null);
    setMsg(null);
    setActionLoading(true);
    try {
      const res = await registerWindscribe({
        email: regEmail.trim() || undefined,
        upstreamProxy: upstreamProxy.trim() || undefined,
      });
      if (!res.success) {
        throw new Error(res.error || "注册失败");
      }
      if (regEmail.trim()) {
        setMsg("账号开通成功！验证邮件已发送至邮箱，激活后可永久享受 10GB/月 额度。");
      } else {
        setMsg("匿名账号开通成功（2GB/月体验额度）。");
      }
      await fetchStatus();
    } catch (e: any) {
      setError(parseError(e));
    } finally {
      setActionLoading(false);
    }
  };

  const handleRefresh = async () => {
    setActionLoading(true);
    setError(null);
    setMsg(null);
    try {
      const res = await refreshWindscribe({
        upstreamProxy: upstreamProxy.trim() || undefined,
      });
      if (!res.success) {
        throw new Error(res.error || "刷新失败");
      }
      setMsg("账号额度与服务器列表已刷新");
      await fetchStatus();
    } catch (e: any) {
      setError(parseError(e));
    } finally {
      setActionLoading(false);
    }
  };

  const handleSaveConfig = async () => {
    setSavingConfig(true);
    setError(null);
    setMsg(null);
    try {
      const res = await saveWindscribeConfig({
        listenAddress: listenAddress.trim() || "127.0.0.1",
        listenPort: listenPort || 10809,
        country: selectedCountry || undefined,
        serverTag: selectedServerTag || undefined,
        upstreamProxy: upstreamProxy.trim() || undefined,
      });
      if (!res.success) throw new Error(res.error || "保存失败");
      setMsg("配置保存成功");
      await fetchStatus();
    } catch (e: any) {
      setError(parseError(e));
    } finally {
      setSavingConfig(false);
    }
  };

  const handleToggleStart = async () => {
    setActionLoading(true);
    setError(null);
    setMsg(null);
    try {
      if (isRunning) {
        await stopWindscribe();
        setMsg("已停止 Windscribe 本地通道");
      } else {
        const res = await startWindscribe({
          country: selectedCountry || undefined,
          serverTag: selectedServerTag || undefined,
          listenAddress: listenAddress.trim() || "127.0.0.1",
          listenPort: listenPort || 10809,
        });
        if (!res.success) {
          throw new Error(res.error || "启动失败");
        }
        setMsg(`Windscribe 已启动，本地监听: ${res.address || `${listenAddress}:${listenPort}`}`);
      }
      await fetchStatus();
    } catch (e: any) {
      setError(parseError(e));
    } finally {
      setActionLoading(false);
    }
  };

  const quota = account ? formatQuota(account.trafficUsed, account.trafficMax) : null;

  return (
    <div className="space-y-6">
      {/* 顶部运行状态 Banner */}
      <Card className="border-border/60 bg-gradient-to-r from-card/80 to-card/40 backdrop-blur">
        <CardHeader className="pb-3">
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
            <div className="space-y-1">
              <div className="flex items-center gap-2">
                <CardTitle className="text-xl font-bold flex items-center gap-2">
                  <Shield className="w-5 h-5 text-indigo-500" />
                  Windscribe 独立节点
                </CardTitle>
                <Badge
                  variant={isRunning ? "default" : "secondary"}
                  className={
                    isRunning
                      ? "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400 border-emerald-500/30"
                      : "bg-muted text-muted-foreground"
                  }
                >
                  {isRunning ? (
                    <span className="flex items-center gap-1.5">
                      <span className="w-2 h-2 rounded-full bg-emerald-500 animate-pulse" />
                      已连接 SOCKS5
                    </span>
                  ) : (
                    "已就绪 / 空闲"
                  )}
                </Badge>
              </div>
              <CardDescription>
                Windscribe 官方纯净 HTTPS 代理节点，经由本地轻量级 SOCKS5 转发器输出。
              </CardDescription>
            </div>

            <div className="flex items-center gap-3">
              <Button
                variant={isRunning ? "destructive" : "default"}
                size="sm"
                onClick={handleToggleStart}
                disabled={actionLoading || !account}
                className="gap-2 shadow-sm"
              >
                {actionLoading ? (
                  <RefreshCw className="w-4 h-4 animate-spin" />
                ) : isRunning ? (
                  <>
                    <Square className="w-4 h-4" /> 停止转发
                  </>
                ) : (
                  <>
                    <Play className="w-4 h-4" /> 启动独立节点
                  </>
                )}
              </Button>
            </div>
          </div>
        </CardHeader>

        {isRunning && snapshot?.activeAddress && (
          <CardContent className="pt-0 pb-4">
            <div className="p-3 bg-emerald-500/5 dark:bg-emerald-950/20 border border-emerald-500/20 rounded-lg flex flex-wrap items-center justify-between gap-2 text-sm">
              <div className="flex items-center gap-2">
                <CheckCircle2 className="w-4 h-4 text-emerald-500" />
                <span className="text-muted-foreground">本地出口:</span>
                <code className="font-mono font-semibold text-foreground px-1.5 py-0.5 bg-background/60 rounded">
                  socks5://{snapshot.activeAddress}
                </code>
              </div>
              {snapshot.currentServer && (
                <div className="flex items-center gap-2 text-muted-foreground">
                  <span>目标节点:</span>
                  <Badge variant="outline" className="font-medium bg-background/50">
                    {snapshot.currentServer.tag} ({snapshot.currentServer.host})
                  </Badge>
                </div>
              )}
            </div>
          </CardContent>
        )}
      </Card>

      {/* 提示与报错信息 */}
      {error && (
        <div className="p-3.5 bg-destructive/10 border border-destructive/20 text-destructive rounded-lg space-y-2 text-sm">
          <div className="flex items-start gap-2.5">
            <XCircle className="w-4 h-4 shrink-0 mt-0.5" />
            <div className="flex-1 font-medium">{error}</div>
          </div>
          {(error.includes("Rate limited") || error.includes("429") || error.includes("IP")) && (
            <div className="pl-6 text-xs text-muted-foreground bg-background/40 p-2 rounded border border-border/40">
              💡 <strong>解决方案提示</strong>：Windscribe 对公网机房/共享出口 IP 有开号频率保护。请在下方配置已开启的「开户/登录前置代理」（例如本地运行的客户端代理 <code className="font-mono bg-muted px-1 rounded">http://127.0.0.1:7890</code> 或 <code className="font-mono bg-muted px-1 rounded">socks5://127.0.0.1:10808</code>），即可轻松绕过限制完成开号。
            </div>
          )}
        </div>
      )}

      {msg && (
        <div className="p-3.5 bg-emerald-500/10 border border-emerald-500/20 text-emerald-600 dark:text-emerald-400 rounded-lg flex items-start gap-2.5 text-sm">
          <CheckCircle2 className="w-4 h-4 shrink-0 mt-0.5" />
          <div className="flex-1">{msg}</div>
        </div>
      )}

      {/* 两列布局：账号管理 & 节点配置 */}
      <div className="grid grid-cols-1 lg:grid-cols-12 gap-6">
        {/* 左侧：账号信息与登录/注册 */}
        <div className="lg:col-span-6 space-y-6">
          <Card className="border-border/60">
            <CardHeader className="pb-3">
              <CardTitle className="text-base font-semibold flex items-center justify-between">
                <span className="flex items-center gap-2">
                  <User className="w-4 h-4 text-indigo-500" />
                  Windscribe 账号凭据
                </span>
                {account && (
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={handleRefresh}
                    disabled={actionLoading}
                    className="h-8 gap-1 text-xs"
                  >
                    <RefreshCw className={`w-3.5 h-3.5 ${actionLoading ? "animate-spin" : ""}`} />
                    刷新额度
                  </Button>
                )}
              </CardTitle>
              <CardDescription>
                管理您的 Windscribe 认证会话。支持开通新号或登录已有账号。
              </CardDescription>
            </CardHeader>

            <CardContent className="space-y-4">
              {account ? (
                /* 已登录信息展示 */
                <div className="space-y-4">
                  <div className="p-4 bg-muted/40 rounded-lg border border-border/40 space-y-3">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-2">
                        <span className="font-semibold text-base text-foreground">
                          {account.username}
                        </span>
                        {account.userId && (
                          <Badge variant="outline" className="text-xs">
                            ID: {account.userId}
                          </Badge>
                        )}
                      </div>
                      <Badge
                        variant={account.status === 1 ? "default" : "destructive"}
                        className="text-xs"
                      >
                        {account.status === 1 ? "正常账号" : "降额受限"}
                      </Badge>
                    </div>

                    {/* 流量额度进度条 */}
                    {quota && (
                      <div className="space-y-1.5">
                        <div className="flex justify-between text-xs text-muted-foreground">
                          <span>已用流量: {quota.usedGb} GB</span>
                          <span>总额度: {quota.maxGb} GB</span>
                        </div>
                        <div className="w-full h-2 bg-muted rounded-full overflow-hidden">
                          <div
                            className={`h-full transition-all duration-300 ${
                              quota.percent > 85 ? "bg-amber-500" : "bg-indigo-500"
                            }`}
                            style={{ width: `${quota.percent}%` }}
                          />
                        </div>
                        <div className="text-right text-[11px] text-muted-foreground">
                          使用率: {quota.percent}%
                        </div>
                      </div>
                    )}

                    {account.email ? (
                      <div className="flex items-center gap-1.5 text-xs text-muted-foreground pt-1 border-t border-border/40">
                        <Mail className="w-3.5 h-3.5 text-indigo-400" />
                        <span>绑定邮箱: {account.email}</span>
                      </div>
                    ) : (
                      <div className="text-xs text-amber-600 dark:text-amber-400 bg-amber-500/10 p-2 rounded flex items-center gap-2">
                        <AlertCircle className="w-3.5 h-3.5 shrink-0" />
                        <span>当前为匿名账号(2GB/月)。重新注册时填入邮箱并在邮件中确认可升级为 10GB/月。</span>
                      </div>
                    )}
                  </div>

                  <div className="pt-2 flex gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => {
                        setData((prev) => (prev ? { ...prev, snapshot: { ...prev.snapshot, account: null } } : null));
                      }}
                      className="text-xs text-muted-foreground"
                    >
                      切换其他账号
                    </Button>
                  </div>
                </div>
              ) : (
                /* 未登录：登录 / 注册 Tabs */
                <div className="space-y-4">
                  <div className="flex border-b border-border/60">
                    <button
                      type="button"
                      onClick={() => setAuthTab("login")}
                      className={`pb-2 px-4 text-sm font-medium border-b-2 transition-colors ${
                        authTab === "login"
                          ? "border-indigo-500 text-foreground"
                          : "border-transparent text-muted-foreground hover:text-foreground"
                      }`}
                    >
                      登录已有账号
                    </button>
                    <button
                      type="button"
                      onClick={() => setAuthTab("register")}
                      className={`pb-2 px-4 text-sm font-medium border-b-2 transition-colors ${
                        authTab === "register"
                          ? "border-indigo-500 text-foreground"
                          : "border-transparent text-muted-foreground hover:text-foreground"
                      }`}
                    >
                      快速开通新账号
                    </button>
                  </div>

                  {authTab === "login" ? (
                    <div className="space-y-3">
                      <div>
                        <label className="text-xs font-medium text-muted-foreground mb-1 block">
                          用户名 / Username
                        </label>
                        <Input
                          placeholder="Windscribe 用户名"
                          value={loginUser}
                          onChange={(e) => setLoginUser(e.target.value)}
                        />
                      </div>
                      <div>
                        <label className="text-xs font-medium text-muted-foreground mb-1 block">
                          密码 / Password
                        </label>
                        <Input
                          type="password"
                          placeholder="Windscribe 密码"
                          value={loginPass}
                          onChange={(e) => setLoginPass(e.target.value)}
                        />
                      </div>
                      <Button
                        className="w-full gap-2 mt-2"
                        onClick={handleLogin}
                        disabled={actionLoading}
                      >
                        {actionLoading ? <RefreshCw className="w-4 h-4 animate-spin" /> : <Lock className="w-4 h-4" />}
                        登录并获取凭据
                      </Button>
                    </div>
                  ) : (
                    <div className="space-y-3">
                      <div>
                        <label className="text-xs font-medium text-muted-foreground mb-1 block flex items-center justify-between">
                          <span>绑定邮箱 (可选)</span>
                          <span className="text-[11px] text-indigo-500 font-semibold">填入可激活 10GB/月 额度</span>
                        </label>
                        <Input
                          type="email"
                          placeholder="例如: your-email@example.com (可留空获取2GB)"
                          value={regEmail}
                          onChange={(e) => setRegEmail(e.target.value)}
                        />
                        <p className="text-[11px] text-muted-foreground mt-1">
                          留空直接开通 2GB/月 匿名体验账号；填入真实邮箱后，在收到的验证邮件中点击 Confirm Email 即可激活 10GB 免费流量。
                        </p>
                      </div>
                      <Button
                        className="w-full gap-2 mt-2"
                        onClick={handleRegister}
                        disabled={actionLoading}
                      >
                        {actionLoading ? <RefreshCw className="w-4 h-4 animate-spin" /> : <ArrowRight className="w-4 h-4" />}
                        一键开户注册
                      </Button>
                    </div>
                  )}

                  {/* 前置代理辅助设置 */}
                  <div className="pt-2 border-t border-border/40">
                    <label className="text-xs font-medium text-muted-foreground mb-1 block flex items-center gap-1.5">
                      <Network className="w-3.5 h-3.5 text-indigo-400" />
                      <span>开户/登录前置代理 (可选)</span>
                    </label>
                    <Input
                      placeholder="如: http://127.0.0.1:7890 或 socks5://127.0.0.1:10808"
                      value={upstreamProxy}
                      onChange={(e) => handleProxyChange(e.target.value)}
                      className="text-xs"
                    />
                    <p className="text-[11px] text-muted-foreground mt-1">
                      提示：Windscribe 会限制机房公网 IP 开号。如果您所处的网络环境被限速（429），可配置已连接的代理端口发起注册/登录。
                    </p>
                  </div>
                </div>
              )}
            </CardContent>
          </Card>
        </div>

        {/* 右侧：独立运行配置与节点选择 */}
        <div className="lg:col-span-6 space-y-6">
          <Card className="border-border/60">
            <CardHeader className="pb-3">
              <CardTitle className="text-base font-semibold flex items-center gap-2">
                <Server className="w-4 h-4 text-indigo-500" />
                节点选择与本地配置
              </CardTitle>
              <CardDescription>
                配置本地 SOCKS5 监听端点，选择需要出站连接的免费国家节点。
              </CardDescription>
            </CardHeader>

            <CardContent className="space-y-4">
              {/* 国家过滤选择 */}
              <div>
                <label className="text-xs font-medium text-muted-foreground mb-1 block flex items-center justify-between">
                  <span className="flex items-center gap-1.5">
                    <Globe className="w-3.5 h-3.5 text-indigo-400" />
                    目标国家 / 地区 (共 {countryList.length} 个地区)
                  </span>
                  {servers.length > 0 && (
                    <span className="text-[11px] text-muted-foreground">
                      共可用节点: {servers.length}
                    </span>
                  )}
                </label>
                <select
                  value={selectedCountry}
                  onChange={(e) => {
                    setSelectedCountry(e.target.value);
                    setSelectedServerTag("");
                  }}
                  className="w-full bg-background border border-input rounded-md px-3 py-2 text-sm focus:outline-none focus:ring-1 focus:ring-ring"
                >
                  <option value="">自动最优国家 (Auto)</option>
                  {countryList.map((c) => (
                    <option key={c.loc} value={c.loc}>
                      {c.locName} ({c.loc}) - {c.count} 个节点
                    </option>
                  ))}
                </select>
              </div>

              {/* 具体节点选择 */}
              <div>
                <label className="text-xs font-medium text-muted-foreground mb-1 block">
                  指定节点 (可选)
                </label>
                <select
                  value={selectedServerTag}
                  onChange={(e) => setSelectedServerTag(e.target.value)}
                  className="w-full bg-background border border-input rounded-md px-3 py-2 text-sm focus:outline-none focus:ring-1 focus:ring-ring"
                >
                  <option value="">随机/首选该地区节点</option>
                  {filteredServers.map((s) => (
                    <option key={s.tag} value={s.tag}>
                      {s.tag} ({s.host})
                    </option>
                  ))}
                </select>
              </div>

              <Separator className="my-2" />

              {/* 本地监听参数 */}
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="text-xs font-medium text-muted-foreground mb-1 block">
                    本地监听地址
                  </label>
                  <Input
                    value={listenAddress}
                    onChange={(e) => setListenAddress(e.target.value)}
                    placeholder="127.0.0.1"
                  />
                </div>
                <div>
                  <label className="text-xs font-medium text-muted-foreground mb-1 block">
                    本地 SOCKS5 端口
                  </label>
                  <Input
                    type="number"
                    value={listenPort}
                    onChange={(e) => setListenPort(parseInt(e.target.value, 10) || 10809)}
                    placeholder="10809"
                  />
                </div>
              </div>

              {/* 前置代理 (可选) */}
              <div>
                <label className="text-xs font-medium text-muted-foreground mb-1 block flex items-center gap-1.5">
                  <Network className="w-3.5 h-3.5 text-indigo-400" />
                  <span>前置代理 / 上游代理 (可选)</span>
                </label>
                <Input
                  value={upstreamProxy}
                  onChange={(e) => handleProxyChange(e.target.value)}
                  placeholder="如: http://127.0.0.1:7890 或 socks5://127.0.0.1:10808"
                  className="text-xs"
                />
                <p className="text-[11px] text-muted-foreground mt-1">
                  用于请求 Windscribe API 以及节点连接（若直连遇到阻断或 429 限速）。
                </p>
              </div>

              <div className="pt-2 flex justify-end">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleSaveConfig}
                  disabled={savingConfig}
                  className="gap-2"
                >
                  <Save className="w-4 h-4" />
                  保存设置
                </Button>
              </div>
            </CardContent>
          </Card>
        </div>
      </div>
    </div>
  );
}
