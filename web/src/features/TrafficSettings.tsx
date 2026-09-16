import { useEffect, useRef, useState } from "react";
import { useT } from "@/core/useT";
import {
  Save,
  CheckCircle2,
  Trash2,
  Upload,
  FileCode,
  AlertCircle,
  ShieldAlert,
  Zap,
  Globe,
  Compass,
  ChevronDown,
  ChevronUp,
  BookOpen,
  Route as RouteIcon,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import {
  lanShareStatus,
  setLanShare,
  getRoutingRulesInfo,
  uploadDirectRules,
  uploadBlockRules,
  uploadRoutesFile,
  clearRoutingList,
  type LanStatus,
  type RoutingRulesInfo,
} from "@/core/api";
import { Row, RulesField, TextField } from "./panels";
import type { ConnectionProfile, CoreSnapshot, LanSettings } from "@/types";

export interface TrafficSettingsProps {
  profile: ConnectionProfile;
  onChange: (profile: ConnectionProfile) => void;
  runtime: string;
  snapshot: CoreSnapshot;
  onToast: (title: string, message: string, error?: boolean) => void;
  onSave: () => void;
}

export function TrafficSettings({
  profile,
  onChange,
  runtime,
  snapshot,
  onToast,
  onSave,
}: TrafficSettingsProps) {
  const t = useT();
  const set = (patch: Partial<ConnectionProfile>) => onChange({ ...profile, ...patch });
  return (
    <>
      <Card>
        <CardHeader className="pb-1"><CardTitle className="text-[15px]">{t("Reach")}</CardTitle></CardHeader>
        <CardContent className="pt-0">
          <Row first title="Set the system proxy while connected" help={systemProxyHelp(runtime, t)}>
            <Switch checked={profile.systemProxy} onCheckedChange={(systemProxy) => set({ systemProxy })} />
          </Row>
          <Row
            title="Keep me connected"
            help="Search again when a route drops. Off, a dead session stays dead, which is what you want while testing a network."
          >
            <Switch
              checked={profile.autoReconnect}
              onCheckedChange={(autoReconnect) => set({ autoReconnect })}
            />
          </Row>
          <Row
            title="Block traffic if the tunnel drops"
            help="Applications fail rather than send traffic in the clear. Until a route comes back or you disconnect, this machine has no working proxy."
          >
            <Switch checked={profile.killSwitch} onCheckedChange={(killSwitch) => set({ killSwitch })} />
          </Row>
          <p className="pb-1 text-[13px] text-muted-foreground">
            {t("Put back on disconnect. If the app is killed rather than closed, the next launch restores it.")}
          </p>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-1"><CardTitle className="text-[15px]">{t("Local proxy and DNS")}</CardTitle></CardHeader>
        <CardContent className="grid grid-cols-2 gap-4 pt-2">
          <TextField
            label="Proxy address" mono value={profile.socksAddress}
            onChange={(socksAddress) => set({ socksAddress })}
            help="Where the SOCKS5 listener binds."
          />
          <TextField
            label="DNS resolvers" mono value={profile.dns.join(", ")}
            onChange={(value) => set({ dns: value.split(",").map((item) => item.trim()).filter(Boolean) })}
            help="One to eight addresses, comma separated."
          />
        </CardContent>
      </Card>

      <LanSharing
        profile={profile}
        onChange={onChange}
        connected={snapshot.state === "connected"}
        onToast={onToast}
      />

      <RoutingRulesSection
        profile={profile}
        onChange={onChange}
        onToast={onToast}
      />

      <div className="flex justify-end pt-2 pb-4">
        <Button onClick={onSave} className="gap-2 shadow-sm">
          <Save className="size-4" />
          {t("保存分流与网络设置")}
        </Button>
      </div>
    </>
  );
}

function RoutingRulesSection({
  profile,
  onChange,
  onToast,
}: {
  profile: ConnectionProfile;
  onChange: (profile: ConnectionProfile) => void;
  onToast: (title: string, message: string, error?: boolean) => void;
}) {
  const t = useT();
  const [info, setInfo] = useState<RoutingRulesInfo | null>(null);
  const [loading, setLoading] = useState(false);
  const [showGuide, setShowGuide] = useState(true);
  const directInputRef = useRef<HTMLInputElement | null>(null);
  const blockInputRef = useRef<HTMLInputElement | null>(null);
  const combinedInputRef = useRef<HTMLInputElement | null>(null);

  const set = (patch: Partial<ConnectionProfile>) => onChange({ ...profile, ...patch });

  const fetchInfo = async () => {
    try {
      const res = await getRoutingRulesInfo();
      setInfo(res);
    } catch {
      // ignore background poll
    }
  };

  useEffect(() => {
    void fetchInfo();
  }, []);

  const handleUploadDirect = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    setLoading(true);
    try {
      const buf = await file.arrayBuffer();
      const res = await uploadDirectRules(new Uint8Array(buf));
      onToast(t("上传成功"), t(`已成功加载 ${res.count} 条允许/直连规则！`));
      await fetchInfo();
    } catch (err) {
      onToast(t("上传失败"), String(err), true);
    } finally {
      setLoading(false);
      if (directInputRef.current) directInputRef.current.value = "";
    }
  };

  const handleUploadBlock = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    setLoading(true);
    try {
      const buf = await file.arrayBuffer();
      const res = await uploadBlockRules(new Uint8Array(buf));
      onToast(t("上传成功"), t(`已成功加载 ${res.count} 条阻止规则！`));
      await fetchInfo();
    } catch (err) {
      onToast(t("上传失败"), String(err), true);
    } finally {
      setLoading(false);
      if (blockInputRef.current) blockInputRef.current.value = "";
    }
  };

  const handleUploadCombined = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    setLoading(true);
    try {
      const buf = await file.arrayBuffer();
      const res = await uploadRoutesFile(new Uint8Array(buf));
      onToast(
        t("上传成功"),
        t(`已成功解析导入：${res.directCount} 条直连规则，${res.blockCount} 条阻止规则！`)
      );
      await fetchInfo();
    } catch (err) {
      onToast(t("上传失败"), String(err), true);
    } finally {
      setLoading(false);
      if (combinedInputRef.current) combinedInputRef.current.value = "";
    }
  };

  const handleClear = async (type: "direct" | "block" | "all") => {
    try {
      await clearRoutingList(type);
      onToast(t("已清空"), t("已清空指定规则列表文件"));
      await fetchInfo();
    } catch (err) {
      onToast(t("清空失败"), String(err), true);
    }
  };

  const isBypassEnabled = profile.bypassDirectRoutes ?? profile.bypassIranSites ?? false;

  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between flex-wrap gap-2">
          <CardTitle className="text-[15px] flex items-center gap-2">
            <RouteIcon className="size-4 text-primary" />
            {t("Routing rules (路由规则)")}
          </CardTitle>
          <div className="flex items-center gap-2">
            <input
              ref={combinedInputRef}
              type="file"
              accept=".conf,.txt,.list"
              className="hidden"
              onChange={(e) => void handleUploadCombined(e)}
            />
            <Button
              variant="outline"
              size="sm"
              className="h-7 text-xs gap-1.5"
              disabled={loading}
              onClick={() => combinedInputRef.current?.click()}
            >
              <Upload className="size-3" />
              {t("导入完整 rules.conf")}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="h-7 text-xs gap-1 text-muted-foreground hover:text-foreground"
              onClick={() => setShowGuide(!showGuide)}
            >
              <BookOpen className="size-3.5" />
              {showGuide ? t("收起说明") : t("策略与语法说明")}
              {showGuide ? <ChevronUp className="size-3" /> : <ChevronDown className="size-3" />}
            </Button>
          </div>
        </div>
        <CardDescription className="text-xs">
          {t("Aether 路由分流引擎：阻止列表优先，其次允许直连，其余流量进入加密隧道。支持域名、完整匹配、关键字、正则、IP/CIDR 网段及端口。")}
        </CardDescription>
      </CardHeader>

      <CardContent className="flex flex-col gap-4 pt-1">
        {/* Enable Direct Bypass Switch */}
        <Row
          first
          title="启用分流与直连规则"
          help="开启后，匹配允许/直连规则的流量将绕过 VPN 隧道由本机网络直接发出，不消耗隧道流量，适合局域网、国内站点及免翻直连服务。"
        >
          <Switch
            checked={isBypassEnabled}
            onCheckedChange={(val) => set({ bypassDirectRoutes: val, bypassIranSites: val })}
          />
        </Row>

        {/* Custom Upload Cards */}
        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
          {/* Direct List */}
          <div className="rounded-lg border border-border/70 bg-muted/20 p-3 space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-xs font-semibold flex items-center gap-1.5 text-foreground">
                <CheckCircle2 className="size-3.5 text-emerald-500" />
                {t("允许直连列表 (Direct list)")}
              </span>
              <div className="flex items-center gap-1.5">
                <input
                  ref={directInputRef}
                  type="file"
                  accept=".txt,.conf,.list"
                  className="hidden"
                  onChange={(e) => void handleUploadDirect(e)}
                />
                <Button
                  variant="outline"
                  size="sm"
                  className="h-6 text-[11px] px-2 gap-1"
                  disabled={loading}
                  onClick={() => directInputRef.current?.click()}
                >
                  <Upload className="size-3" />
                  {t("上传列表")}
                </Button>
                {info && info.directCount > 0 && (
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-6 text-[11px] px-1.5 text-muted-foreground hover:text-destructive"
                    title={t("清空已上传直连列表")}
                    onClick={() => void handleClear("direct")}
                  >
                    <Trash2 className="size-3" />
                  </Button>
                )}
              </div>
            </div>
            <div className="text-[11px] text-muted-foreground">
              {info && info.directCount > 0 ? (
                <span className="text-emerald-500 font-medium font-mono">
                  已加载 {info.directCount} 条直连规则 (direct.txt)
                </span>
              ) : (
                <span>未上传直连文件，可上传包含域名或 CIDR 的文本</span>
              )}
            </div>
            <RulesField
              label="Bypass the tunnel (自定义直连规则)"
              value={profile.routeDirect}
              placeholder={"private\n10.0.0.0/8\nexample.com"}
              onChange={(routeDirect) => set({ routeDirect })}
            />
          </div>

          {/* Block List */}
          <div className="rounded-lg border border-border/70 bg-muted/20 p-3 space-y-2">
            <div className="flex items-center justify-between">
              <span className="text-xs font-semibold flex items-center gap-1.5 text-foreground">
                <AlertCircle className="size-3.5 text-destructive" />
                {t("阻止拦截列表 (Block list)")}
              </span>
              <div className="flex items-center gap-1.5">
                <input
                  ref={blockInputRef}
                  type="file"
                  accept=".txt,.conf,.list"
                  className="hidden"
                  onChange={(e) => void handleUploadBlock(e)}
                />
                <Button
                  variant="outline"
                  size="sm"
                  className="h-6 text-[11px] px-2 gap-1"
                  disabled={loading}
                  onClick={() => blockInputRef.current?.click()}
                >
                  <Upload className="size-3" />
                  {t("上传列表")}
                </Button>
                {info && info.blockCount > 0 && (
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-6 text-[11px] px-1.5 text-muted-foreground hover:text-destructive"
                    title={t("清空已上传阻止列表")}
                    onClick={() => void handleClear("block")}
                  >
                    <Trash2 className="size-3" />
                  </Button>
                )}
              </div>
            </div>
            <div className="text-[11px] text-muted-foreground">
              {info && info.blockCount > 0 ? (
                <span className="text-destructive font-medium font-mono">
                  已加载 {info.blockCount} 条阻止规则 (block.txt)
                </span>
              ) : (
                <span>未上传阻止文件，可上传包含域名或敏感端口的文本</span>
              )}
            </div>
            <RulesField
              label="Never send (自定义阻止规则)"
              value={profile.routeBlock}
              placeholder={"keyword:doubleclick\nport:25\nads.example.com"}
              onChange={(routeBlock) => set({ routeBlock })}
            />
          </div>
        </div>

        <TextField
          label="Rules file (外部规则文件路径)" mono value={profile.routesFile ?? ""}
          placeholder="可选本地绝对路径，如 /etc/aether/routes.conf"
          onChange={(value) => set({ routesFile: value || null })}
          help="指定本地文件，与上述上传列表及自定义规则合并生效。"
        />

        {/* Detailed Routing Strategy & Syntax Guide */}
        {showGuide && (
          <div className="rounded-lg border border-border/70 bg-muted/15 p-3.5 space-y-3.5 transition-all text-xs">
            <div className="space-y-1.5">
              <div className="font-semibold text-foreground flex items-center gap-1.5 text-[13px]">
                <Compass className="size-4 text-primary" />
                {t("分流路由策略机制 (Routing Policy & Order)")}
              </div>
              <p className="text-muted-foreground text-[11.5px] leading-relaxed">
                Aether 核心分流采用严格的三级流水线判断，规则命中后立即执行对应策略，不再继续向下匹配：
              </p>
              <div className="grid grid-cols-1 md:grid-cols-3 gap-2.5 pt-1">
                <div className="rounded-md border border-destructive/30 bg-destructive/5 p-2.5 space-y-1">
                  <div className="flex items-center gap-1.5 font-semibold text-destructive">
                    <ShieldAlert className="size-3.5" />
                    <span>1. 阻止拦截 [block]</span>
                  </div>
                  <p className="text-[11px] text-muted-foreground leading-snug">
                    <strong className="text-foreground">最高优先级。</strong>请求就地阻断并直接返回连接拒绝，绝不向外发出任何数据包。适用于广告、隐私追踪、遥测与敏感端口封禁。
                  </p>
                </div>
                <div className="rounded-md border border-emerald-500/30 bg-emerald-500/5 p-2.5 space-y-1">
                  <div className="flex items-center gap-1.5 font-semibold text-emerald-600 dark:text-emerald-400">
                    <Zap className="size-3.5" />
                    <span>2. 允许直连 [direct]</span>
                  </div>
                  <p className="text-[11px] text-muted-foreground leading-snug">
                    <strong className="text-foreground">次优先级。</strong>流量完全绕过隧道，直接经由本机物理网络接口发出。0 隧道带宽消耗、极低原生网络延迟，适用于内网局域网、国内网站。
                  </p>
                </div>
                <div className="rounded-md border border-primary/30 bg-primary/5 p-2.5 space-y-1">
                  <div className="flex items-center gap-1.5 font-semibold text-primary">
                    <Globe className="size-3.5" />
                    <span>3. 默认隧道代理</span>
                  </div>
                  <p className="text-[11px] text-muted-foreground leading-snug">
                    <strong className="text-foreground">默认回退。</strong>未被上述规则命中的所有外部网络流量，安全汇入已建立的加密隧道出站，受到保护与伪装。
                  </p>
                </div>
              </div>
            </div>

            <Separator className="opacity-40" />

            <div className="space-y-1.5">
              <div className="font-semibold text-foreground flex items-center gap-1.5 text-[12.5px]">
                <FileCode className="size-3.5 text-primary" />
                {t("规则匹配语法规则示例")}
              </div>
              <div className="overflow-x-auto rounded border border-border/60 bg-background/50">
                <table className="w-full text-left border-collapse text-[11px]">
                  <thead>
                    <tr className="border-b border-border/60 bg-muted/40 text-muted-foreground">
                      <th className="py-1.5 px-2 font-medium">匹配类型</th>
                      <th className="py-1.5 px-2 font-medium">语法格式</th>
                      <th className="py-1.5 px-2 font-medium">行为示例与说明</th>
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-border/40 font-mono">
                    <tr>
                      <td className="py-1.5 px-2 font-medium text-foreground">域名后缀</td>
                      <td className="py-1.5 px-2 font-mono text-primary">example.com</td>
                      <td className="py-1.5 px-2 text-muted-foreground">匹配自身及其所有子域名（如 a.example.com）</td>
                    </tr>
                    <tr>
                      <td className="py-1.5 px-2 font-medium text-foreground">精准域名</td>
                      <td className="py-1.5 px-2 font-mono text-primary">full:login.test.com</td>
                      <td className="py-1.5 px-2 text-muted-foreground">仅精准匹配单个完整域名，不含子域名</td>
                    </tr>
                    <tr>
                      <td className="py-1.5 px-2 font-medium text-foreground">关键字包含</td>
                      <td className="py-1.5 px-2 font-mono text-primary">keyword:google</td>
                      <td className="py-1.5 px-2 text-muted-foreground">只要域名中含有该子串即匹配</td>
                    </tr>
                    <tr>
                      <td className="py-1.5 px-2 font-medium text-foreground">IP / CIDR网段</td>
                      <td className="py-1.5 px-2 font-mono text-primary">192.168.1.0/24</td>
                      <td className="py-1.5 px-2 text-muted-foreground">支持 IPv4 与 IPv6 网段范围命中</td>
                    </tr>
                    <tr>
                      <td className="py-1.5 px-2 font-medium text-foreground">端口控制</td>
                      <td className="py-1.5 px-2 font-mono text-primary">port:25 或 port:1-1024</td>
                      <td className="py-1.5 px-2 text-muted-foreground">指定单个端口或连续端口范围</td>
                    </tr>
                    <tr>
                      <td className="py-1.5 px-2 font-medium text-foreground">局域网保留字</td>
                      <td className="py-1.5 px-2 font-mono text-primary">private</td>
                      <td className="py-1.5 px-2 text-muted-foreground">智能保留字：自动匹配局域网私网、回环、CGNAT 及 IPv6 link-local</td>
                    </tr>
                  </tbody>
                </table>
              </div>
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function LanSharing({
  profile,
  onChange,
  connected,
  onToast,
}: {
  profile: ConnectionProfile;
  onChange: (profile: ConnectionProfile) => void;
  connected: boolean;
  onToast: (title: string, message: string, error?: boolean) => void;
}) {
  const t = useT();
  const share = profile.lanShare;
  const [status, setStatus] = useState<LanStatus>({ running: false, address: null, open: false });
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    lanShareStatus().then(setStatus).catch(() => setStatus({ running: false, address: null, open: false }));
  }, [connected]);

  const apply = async (patch: Partial<LanSettings>) => {
    const next = { ...share, ...patch };
    onChange({ ...profile, lanShare: next });
    if (!connected && next.enabled) return;
    setBusy(true);
    try {
      setStatus(await setLanShare(next));
    } catch (error) {
      onChange({ ...profile, lanShare: { ...next, enabled: false } });
      setStatus({ running: false, address: null, open: false });
      onToast("Could not share", error instanceof Error ? error.message : String(error), true);
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card>
      <CardHeader className="pb-1">
        <CardTitle className="text-[15px]">{t("Share with other devices")}</CardTitle>
        <CardDescription>
          {t(
            "Opens a proxy on this machine that phones, televisions and anything else on the same network can point at. They go out through whatever is carrying traffic here — the second hop when one is running, the tunnel when it is not.",
          )}
        </CardDescription>
      </CardHeader>
      <CardContent className="pt-0">
        <Row
          first
          title="Share this connection on my network"
          help={
            connected
              ? t("The port is opened while connected and closed when you disconnect.")
              : t("Connect first — there is nothing to share until the tunnel is carrying traffic.")
          }
        >
          <Switch
            checked={share.enabled}
            disabled={busy || (!connected && !share.enabled)}
            onCheckedChange={(enabled) => void apply({ enabled })}
          />
        </Row>

        {share.enabled ? (
          <>
            <div className="grid grid-cols-3 gap-4 py-3">
              <TextField
                label="Port"
                mono
                value={String(share.port)}
                onChange={(value) => {
                  const port = Number(value.replace(/[^0-9]/g, ""));
                  onChange({
                    ...profile,
                    lanShare: { ...share, port: Number.isFinite(port) ? port : 0 },
                  });
                }}
                help="Typed into the other device."
              />
              <TextField
                label="Username"
                value={share.username}
                onChange={(username) => onChange({ ...profile, lanShare: { ...share, username } })}
                help="Optional."
              />
              <TextField
                label="Password"
                value={share.password}
                onChange={(password) => onChange({ ...profile, lanShare: { ...share, password } })}
                help="Optional."
              />
            </div>

            {!share.username.trim() || !share.password.trim() ? (
              <div className="mb-3 rounded-lg border border-amber-500/40 bg-amber-500/[0.08] p-3">
                <div className="text-[12.5px] font-semibold text-amber-500">
                  {t("No sign-in: anyone on this network can use your tunnel")}
                </div>
                <div className="mt-1 text-[12px] text-muted-foreground">
                  Every device that can reach this machine — including guests and anything else on a
                  shared or public network — can send traffic through your connection, and it will
                  leave from your exit address. Fill in both a username and a password to require a
                  sign-in.
                </div>
              </div>
            ) : null}

            <div className="flex items-center justify-between gap-4 border-t pt-3">
              <div className="min-w-0">
                <div className="text-[13px] font-medium">
                  {status.running ? t("Open") : t("Not open")}
                </div>
                <div className="truncate font-mono text-[11.5px] text-muted-foreground">
                  {status.running && status.address
                    ? `Point devices at ${status.address} — HTTP or SOCKS5, same port`
                    : t("Apply to open the port.")}
                </div>
              </div>
              <Button
                variant="outline"
                size="sm"
                disabled={busy || !connected}
                onClick={() => void apply({})}
              >
                {t("Apply")}
              </Button>
            </div>

            <p className="pt-2 text-[12px] text-muted-foreground">
              Windows asks to allow this the first time. Until you say yes, the port answers on this
              machine only.
            </p>
          </>
        ) : null}
      </CardContent>
    </Card>
  );
}

function systemProxyHelp(runtime: string, t: (s: string) => string): string {
  const os = runtime.split(" · ")[0]?.toLowerCase();
  if (os === "windows") return "Sets the WinINET proxy. Most apps follow it; some bring their own settings.";
  if (os === "macos") return "Sets the SOCKS proxy on every active network service.";
  if (os === "linux") return "Sets the GNOME proxy. Desktops that ignore gsettings are unaffected.";
  return t("Sets the operating system's proxy settings.");
}
