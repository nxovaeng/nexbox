import { useEffect, useState, useRef } from "react";
import {
  CheckCircle2,
  XCircle,
  AlertCircle,
  Upload,
  RefreshCw,
  Globe,
  Radio,
  Play,
  Square,
  Key,
  Settings,
  Server,
  Network,
  Save,
  Shield,
} from "lucide-react";
import { useT } from "@/core/useT";
import {
  type PsiphonInfo,
  getPsiphonInfo,
  uploadPsiphonServerList,
  savePsiphonConfig,
  startPsiphonStandalone,
  stopPsiphonStandalone,
} from "@/core/api";
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

export function PsiphonManager() {
  const t = useT();
  const [info, setInfo] = useState<PsiphonInfo | null>(null);
  const [uploading, setUploading] = useState(false);
  const [actionLoading, setActionLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  // 公用基础配置
  const [sigKey, setSigKey] = useState("");
  const [channelId, setChannelId] = useState("");
  const [sponsorId, setSponsorId] = useState("");

  // 独立启动参数
  const [listenAddress, setListenAddress] = useState("127.0.0.1");
  const [listenPort, setListenPort] = useState<number>(10808);
  const [egressRegion, setEgressRegion] = useState("");

  const [savingConfig, setSavingConfig] = useState(false);
  const fileInputRef = useRef<HTMLInputElement | null>(null);

  const fetchInfo = async () => {
    try {
      const res = await getPsiphonInfo();
      setInfo(res);
      setSigKey(res.customSignatureKey ?? "");
      setListenAddress(res.listenAddress || res.defaultStandaloneAddress || "127.0.0.1");
      setListenPort(res.listenPort ?? res.defaultStandalonePort);
      if (res.egressRegion !== undefined) setEgressRegion(res.egressRegion);
      setChannelId(res.propagationChannelId ?? "");
      setSponsorId(res.sponsorId ?? "");
    } catch {
      // Ignore background poll errors
    }
  };

  useEffect(() => {
    void fetchInfo();
    const timer = setInterval(() => {
      void fetchInfo();
    }, 3000);
    return () => clearInterval(timer);
  }, []);

  const handleUploadList = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    setUploading(true);
    setError(null);
    setMsg(null);
    try {
      const buffer = await file.arrayBuffer();
      const res = await uploadPsiphonServerList(new Uint8Array(buffer));
      setMsg(t(`已成功上传引导节点列表，共加载 ${res.count} 个节点！`));
      await fetchInfo();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setUploading(false);
      if (fileInputRef.current) fileInputRef.current.value = "";
    }
  };

  const handleSaveAll = async () => {
    setSavingConfig(true);
    setError(null);
    setMsg(null);
    try {
      await savePsiphonConfig({
        signaturePublicKey: sigKey,
        listenPort,
        listenAddress,
        egressRegion,
        propagationChannelId: channelId,
        sponsorId,
      });
      setMsg(t("Psiphon 配置保存成功！"));
      await fetchInfo();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSavingConfig(false);
    }
  };

  const handleRegionChange = async (newRegion: string) => {
    setEgressRegion(newRegion);
    setError(null);
    try {
      await savePsiphonConfig({
        signaturePublicKey: sigKey,
        listenPort,
        listenAddress,
        egressRegion: newRegion,
        propagationChannelId: channelId,
        sponsorId,
      });
      if (isRunning) {
        setActionLoading(true);
        setMsg(t("Moving the exit. This reconnects, so it takes as long as connecting does."));
        await startPsiphonStandalone({ egressRegion: newRegion, listenPort, listenAddress });
        setMsg(t("Psiphon 出口已切换"));
      }
      await fetchInfo();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setActionLoading(false);
    }
  };

  const handleResetSigKey = () => {
    if (info) {
      setSigKey(info.defaultSignatureKey);
    }
  };

  const isRunning = info?.snapshot.state === "connected" || info?.snapshot.state === "connecting";

  const handleToggleStandalone = async () => {
    setActionLoading(true);
    setError(null);
    setMsg(null);
    try {
      if (isRunning) {
        await stopPsiphonStandalone();
        setMsg(t("Psiphon 独立运行已停止"));
      } else {
        await startPsiphonStandalone({ egressRegion, listenPort, listenAddress });
        setMsg(t("Psiphon 独立运行已启动"));
      }
      await fetchInfo();
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setActionLoading(false);
    }
  };

  return (
    <div className="space-y-4">
      {/* Top Header Card */}
      <Card className="border border-border/80">
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between flex-wrap gap-2">
            <div className="flex items-center gap-2.5">
              <div className="p-2 rounded-lg bg-primary/10 text-primary">
                <Radio className="size-5" />
              </div>
              <div>
                <CardTitle className="text-base font-semibold">
                  Psiphon 配置管理
                </CardTitle>
                <CardDescription className="text-xs mt-0.5">
                  独立管理 Psiphon 载体，解耦公用基础配置与独立启动参数，实现各程序独立配置。
                </CardDescription>
              </div>
            </div>

            {/* Status Badges */}
            <div className="flex items-center gap-2 flex-wrap">
              {info?.installed ? (
                <Badge variant="ok" className="text-xs gap-1">
                  <CheckCircle2 className="size-3" />
                  核心已就绪
                </Badge>
              ) : (
                <Badge variant="destructive" className="text-xs gap-1">
                  <XCircle className="size-3" />
                  核心未就绪
                </Badge>
              )}
              {isRunning ? (
                <Badge variant="ok" className="text-xs animate-pulse gap-1">
                  <span className="size-1.5 rounded-full bg-current" />
                  {info?.snapshot.state === "connected" ? "独立运行中" : "正在连接..."}
                </Badge>
              ) : (
                <Badge variant="outline" className="text-xs text-muted-foreground">
                  未独立运行
                </Badge>
              )}
              {info?.snapshot.socksPort ? (
                <span className="text-xs font-mono px-2 py-0.5 rounded bg-muted font-medium text-foreground">
                  SOCKS5: {listenAddress}:{info.snapshot.socksPort}
                </span>
              ) : null}
              {info?.snapshot.exitRegion && (
                <Badge variant="outline" className="text-xs text-emerald-600 dark:text-emerald-400 font-mono">
                  出口: {info.snapshot.exitRegion}
                </Badge>
              )}
            </div>
          </div>
        </CardHeader>
      </Card>

      {/* Group 1: 公用基础配置 (Common / Bootstrap Settings) */}
      <Card className="border border-border/70">
        <CardHeader className="pb-2">
          <div className="flex items-center justify-between">
            <CardTitle className="text-sm font-semibold flex items-center gap-2">
              <Shield className="size-4 text-primary" />
              公用基础配置 (General & Bootstrap)
            </CardTitle>
            <Badge variant="secondary" className="text-[11px]">
              级联与独立模式通用
            </Badge>
          </div>
          <CardDescription className="text-xs">
            用于向 Psiphon 核心提供身份验证、节点发现候选池与分发标识，无论是在多跳级联还是独立运行中均共享生效。
          </CardDescription>
        </CardHeader>

        <CardContent className="space-y-4 pt-1">
          {/* Server List */}
          <div className="rounded-lg border border-border/60 bg-muted/20 p-3 space-y-2">
            <div className="flex items-center justify-between flex-wrap gap-2">
              <div className="flex items-center gap-2">
                <Server className="size-4 text-muted-foreground" />
                <span className="text-xs font-medium">引导节点列表 (psiphon_server_entries.txt)</span>
              </div>
              <div>
                <input
                  ref={fileInputRef}
                  type="file"
                  accept=".txt"
                  className="hidden"
                  onChange={(e) => void handleUploadList(e)}
                />
                <Button
                  variant="outline"
                  size="sm"
                  className="h-7 text-xs gap-1.5"
                  disabled={uploading}
                  onClick={() => fileInputRef.current?.click()}
                >
                  <Upload className="size-3.5" />
                  {uploading ? "正在上传..." : "上传节点列表"}
                </Button>
              </div>
            </div>
            <div className="text-xs text-muted-foreground flex flex-col gap-0.5">
              {info?.serverListPath ? (
                <div className="flex items-center gap-1.5 text-emerald-600 dark:text-emerald-400 font-medium">
                  <CheckCircle2 className="size-3.5" />
                  <span>
                    已加载 {info.serverListCount} 个引导节点 ({info.serverListPath})
                  </span>
                </div>
              ) : (
                <div className="flex items-center gap-1.5 text-amber-600 dark:text-amber-400">
                  <AlertCircle className="size-3.5" />
                  <span>未检测到引导列表文件，首次启动前请上传 psiphon_server_entries.txt</span>
                </div>
              )}
              <span className="text-[11px] text-muted-foreground/80">
                用于初次启动时握手并建立首批隧道，连通后核心会在隧道内自动拉取并维护最新可用节点池。
              </span>
            </div>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            {/* Signature Public Key */}
            <div className="space-y-1.5 md:col-span-2">
              <label className="text-xs font-medium flex items-center justify-between">
                <span className="flex items-center gap-1.5">
                  <Key className="size-3.5 text-muted-foreground" />
                  节点签名公钥 (ServerEntrySignaturePublicKey)
                </span>
                <button
                  type="button"
                  onClick={handleResetSigKey}
                  className="text-[11px] text-primary hover:underline"
                >
                  恢复官方默认
                </button>
              </label>
              <Input
                value={sigKey}
                onChange={(e) => setSigKey(e.target.value)}
                placeholder={info?.defaultSignatureKey || "留空使用内置默认公钥"}
                className="h-8 text-xs font-mono"
              />
              <p className="text-[11px] text-muted-foreground">
                用于校验节点列表签名的 Base64 Ed25519 验证公钥。当前生效：
                <span className="font-mono text-foreground/80 ml-1">
                  {info?.activeSignatureKey.slice(0, 20)}...
                </span>
              </p>
            </div>

            {/* Propagation Channel ID */}
            <div className="space-y-1.5">
              <label className="text-xs font-medium">传播渠道 ID (PropagationChannelId)</label>
              <Input
                value={channelId}
                onChange={(e) => setChannelId(e.target.value)}
                placeholder="9E258C5A3F0E4540"
                className="h-8 text-xs font-mono"
              />
              <p className="text-[11px] text-muted-foreground">
                分发渠道标识，留空自动使用官方生产高可用渠道。
              </p>
            </div>

            {/* Sponsor ID */}
            <div className="space-y-1.5">
              <label className="text-xs font-medium">赞助商 ID (SponsorId)</label>
              <Input
                value={sponsorId}
                onChange={(e) => setSponsorId(e.target.value)}
                placeholder="F6AC81EBF343EE50"
                className="h-8 text-xs font-mono"
              />
              <p className="text-[11px] text-muted-foreground">
                出口带宽赞助归属，留空使用配套官方赞助 ID。
              </p>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Group 2: 独立启动参数 (Standalone Launch Parameters) */}
      <Card className="border border-border/70">
        <CardHeader className="pb-2">
          <div className="flex items-center justify-between">
            <CardTitle className="text-sm font-semibold flex items-center gap-2">
              <Network className="size-4 text-primary" />
              独立启动参数 (Standalone Launch Parameters)
            </CardTitle>
            <Badge variant="outline" className="text-[11px] font-mono">
              standalone_config.json
            </Badge>
          </div>
          <CardDescription className="text-xs">
            独立运行 Psiphon 核心时的专用 SOCKS5 参数，独立保存在数据目录中，与主 VPN Profile 配置文件完全隔离。
          </CardDescription>
        </CardHeader>

        <CardContent className="space-y-4 pt-1">
          <div className="grid grid-cols-1 md:grid-cols-3 gap-3">
            {/* Listen Address / Interface */}
            <div className="space-y-1.5">
              <label className="text-xs font-medium flex items-center justify-between">
                <span className="flex items-center gap-1.5">
                  <Network className="size-3.5 text-muted-foreground" />
                  本地监听地址 (Host)
                </span>
              </label>
              <Input
                value={listenAddress}
                onChange={(e) => setListenAddress(e.target.value)}
                placeholder="127.0.0.1"
                className="h-8 text-xs font-mono"
              />
              <div className="flex items-center gap-1 pt-0.5">
                <button
                  type="button"
                  onClick={() => setListenAddress("127.0.0.1")}
                  className={`text-[10.5px] px-1.5 py-0.5 rounded border ${
                    listenAddress === "127.0.0.1"
                      ? "border-primary bg-primary/10 text-primary font-medium"
                      : "border-border text-muted-foreground hover:text-foreground"
                  }`}
                >
                  127.0.0.1 (仅本机)
                </button>
                <button
                  type="button"
                  onClick={() => setListenAddress("0.0.0.0")}
                  className={`text-[10.5px] px-1.5 py-0.5 rounded border ${
                    listenAddress === "0.0.0.0"
                      ? "border-primary bg-primary/10 text-primary font-medium"
                      : "border-border text-muted-foreground hover:text-foreground"
                  }`}
                >
                  0.0.0.0 (局域网共享)
                </button>
              </div>
            </div>

            {/* Listen Port */}
            <div className="space-y-1.5">
              <label className="text-xs font-medium flex items-center gap-1.5">
                <Settings className="size-3.5 text-muted-foreground" />
                固定 SOCKS5 监听端口 (Port)
              </label>
              <Input
                type="number"
                value={listenPort}
                onChange={(e) => setListenPort(Number(e.target.value))}
                placeholder="10808"
                className="h-8 text-xs font-mono"
              />
              <p className="text-[11px] text-muted-foreground">
                独立运行时的固定监听端口（默认 10808）。
              </p>
            </div>

            {/* Exit Country */}
            <div className="space-y-1.5">
              <label className="text-xs font-medium flex items-center justify-between">
                <span className="flex items-center gap-1.5">
                  <Globe className="size-3.5 text-muted-foreground" />
                  {t("Exit country")}
                </span>
                {info?.snapshot.exitRegion && (
                  <span className="text-[11px] text-emerald-600 dark:text-emerald-400 font-medium font-mono">
                    当前: {info.snapshot.exitRegion}
                  </span>
                )}
              </label>
              <select
                value={egressRegion}
                disabled={actionLoading}
                onChange={(e) => void handleRegionChange(e.target.value)}
                className="h-8 w-full rounded-md border border-input bg-background px-2 text-xs font-mono"
              >
                <option value="">{t("Best available")}</option>
                {egressRegion && !info?.availableRegions?.includes(egressRegion) && (
                  <option value={egressRegion}>{egressRegion}</option>
                )}
                {info?.availableRegions?.map((region) => (
                  <option key={region} value={region}>
                    {region}
                  </option>
                ))}
              </select>
              <p className="text-[11px] text-muted-foreground">
                偏好国家二字码，未指定时选择最佳可用出口。
              </p>
            </div>
          </div>

          <p className="text-[11.5px] text-muted-foreground italic bg-muted/20 p-2 rounded border border-border/40">
            {t("A preference, not a guarantee. Psiphon keeps trying rather than substituting, so a country with no capacity is a slow connect.")}
          </p>

          <Separator className="bg-border/60" />

          {/* Action Toolbar */}
          <div className="flex items-center justify-between pt-1 flex-wrap gap-2">
            <div className="flex items-center gap-2">
              <Button
                size="sm"
                variant={isRunning ? "destructive" : "default"}
                disabled={actionLoading || !info?.installed}
                onClick={() => void handleToggleStandalone()}
                className="h-8 text-xs gap-1.5 font-medium"
              >
                {actionLoading ? (
                  <RefreshCw className="size-3.5 animate-spin" />
                ) : isRunning ? (
                  <Square className="size-3.5" />
                ) : (
                  <Play className="size-3.5" />
                )}
                {isRunning ? "停止 Psiphon" : "独立启动 Psiphon"}
              </Button>

              {info?.snapshot.socksPort ? (
                <span className="text-xs font-mono text-muted-foreground font-medium">
                  监听中: {listenAddress}:{info.snapshot.socksPort}
                </span>
              ) : null}
            </div>

            <Button
              size="sm"
              variant="outline"
              disabled={savingConfig}
              onClick={() => void handleSaveAll()}
              className="h-8 text-xs gap-1.5"
            >
              <Save className="size-3.5" />
              {savingConfig ? "正在保存..." : "保存全部配置"}
            </Button>
          </div>

          {/* Feedback Messages */}
          {msg && (
            <div className="flex items-center gap-2 rounded-md border border-primary/30 bg-primary/10 px-3 py-1.5 text-xs text-primary">
              <CheckCircle2 className="size-3.5 shrink-0" />
              <span>{msg}</span>
            </div>
          )}
          {error && (
            <div className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              <XCircle className="size-4 shrink-0 mt-0.5" />
              <span className="break-words">{error}</span>
            </div>
          )}
          {info?.snapshot.lastError && (
            <div className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
              <XCircle className="size-4 shrink-0 mt-0.5" />
              <span className="break-words">{info.snapshot.lastError}</span>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
