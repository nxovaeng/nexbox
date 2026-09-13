import { useEffect, useState, useRef } from "react";
import {
  CheckCircle2,
  XCircle,
  AlertCircle,
  Download,
  Upload,
  RefreshCw,
  HardDrive,
  ExternalLink,
  Shield,
  Layers,
  Globe,
  Radio,
  FileCode,
} from "lucide-react";
import { useT } from "@/core/useT";
import {
  type CoreItem,
  getCoreInventory,
  downloadCore,
  uploadCore,
} from "@/core/api";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";

type ActionState = "idle" | "working" | "done" | "error";

interface CoreMeta {
  title: string;
  role: string;
  source: string;
  sourceUrl?: string;
  canDownload: boolean;
  note?: string;
  icon: typeof Shield;
}

const CORE_METAS: Record<string, CoreMeta> = {
  aether: {
    title: "Aether 核心",
    role: "主 VPN 引擎，提供 MASQUE (H2/H3)、WireGuard 与 AmneziaWG 协议支持",
    source: "GitHub: CluvexStudio/Aether (最新发行版)",
    sourceUrl: "https://github.com/CluvexStudio/Aether/releases",
    canDownload: true,
    note: "受控主引擎，负责建立本地 SOCKS5 监听并管理出口端点连接。",
    icon: Shield,
  },
  mihomo: {
    title: "Mihomo 链式代理核心",
    role: "主要提供链式代理（VPN in VPN / 多跳代理），实现双重 VPN 嵌套与分流规则",
    source: "GitHub: MetaCubeX/mihomo (v1.19.30)",
    sourceUrl: "https://github.com/MetaCubeX/mihomo/releases/tag/v1.19.30",
    canDownload: true,
    note: "在本程序中主要作为链式多跳代理引擎（VPN in VPN），可将 Aether、Tor、Psiphon 等网络载体与外部出口节点串联嵌套，实现双重甚至多重加密隧道与智能分流。",
    icon: Layers,
  },
  tor: {
    title: "Tor 载体",
    role: "Tor 洋葱路由，集成 Lyrebird 可插拔传输（obfs4、meek、snowflake）",
    source: "Tor Project: Expert Bundle (15.0.22)",
    sourceUrl: "https://dist.torproject.org/torbrowser/15.0.22/",
    canDownload: true,
    note: "通过 Tor 网络网桥与中继建立高匿名抗封锁隧道。",
    icon: Globe,
  },
  psiphon: {
    title: "Psiphon 载体",
    role: "多协议反审查穿透隧道核心",
    source: "GitHub: Psiphon-Labs/psiphon-tunnel-core",
    sourceUrl: "https://github.com/Psiphon-Labs/psiphon-tunnel-core",
    canDownload: false,
    note: "官方未提供编译好的 CLI 独立二进制，可自行编译后上传部署。",
    icon: Radio,
  },
  warpscout: {
    title: "WarpScout 扫描器",
    role: "专用的 WARP / MASQUE 优选对端发现与延迟测速扫描器",
    source: "GitHub: vernette/warpscout (最新发行版)",
    sourceUrl: "https://github.com/vernette/warpscout/releases",
    canDownload: true,
    note: "高速扫描探测 Cloudflare 网关 IP，自动优选低延迟、零丢包端点。",
    icon: RefreshCw,
  },
  wireproxy: {
    title: "Wireproxy 核心",
    role: "用户态 WireGuard 代理客户端，生成本地 SOCKS5/HTTP 监听端点",
    source: "GitHub: windtf/wireproxy (v1.1.3)",
    sourceUrl: "https://github.com/windtf/wireproxy/releases",
    canDownload: true,
    note: "轻量级 WireGuard 客户端，无需 TUN 设备即可在用户空间提供代理服务。",
    icon: FileCode,
  },
};

export function CoreManager() {
  const t = useT();
  const [inventory, setInventory] = useState<CoreItem[]>([]);
  const [loading, setLoading] = useState(false);
  const [fetchError, setFetchError] = useState<string | null>(null);
  const [itemAction, setItemAction] = useState<
    Record<string, { state: ActionState; msg?: string; progressText?: string }>
  >({});

  const fileInputRefs = useRef<Record<string, HTMLInputElement | null>>({});

  const refresh = async () => {
    setLoading(true);
    setFetchError(null);
    try {
      const data = await getCoreInventory();
      setInventory(data);
    } catch (e: unknown) {
      setFetchError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void refresh();
  }, []);

  const setAction = (
    name: string,
    state: ActionState,
    msg?: string,
    progressText?: string
  ) => {
    setItemAction((prev) => ({
      ...prev,
      [name]: { state, msg, progressText },
    }));
  };

  const handleDownload = async (coreName: string) => {
    setAction(coreName, "working", undefined, t("Downloading from upstream…"));
    try {
      await downloadCore(coreName);
      setAction(coreName, "done", t("Installed successfully"));
      await refresh();
      window.setTimeout(() => {
        setItemAction((prev) => {
          const next = { ...prev };
          delete next[coreName];
          return next;
        });
      }, 4000);
    } catch (e: unknown) {
      setAction(
        coreName,
        "error",
        e instanceof Error ? e.message : String(e)
      );
    }
  };

  const handleUploadClick = (coreName: string) => {
    const input = fileInputRefs.current[coreName];
    if (input) {
      input.value = "";
      input.click();
    }
  };

  const handleFileSelected = async (
    coreName: string,
    e: React.ChangeEvent<HTMLInputElement>
  ) => {
    const file = e.target.files?.[0];
    if (!file) return;

    setAction(
      coreName,
      "working",
      undefined,
      `${t("Uploading")} ${file.name} (${(file.size / 1024 / 1024).toFixed(1)} MB)…`
    );

    try {
      const res = await uploadCore(coreName, file);
      if (res.success) {
        setAction(
          coreName,
          "done",
          t("Binary uploaded and permissions set successfully")
        );
        await refresh();
        window.setTimeout(() => {
          setItemAction((prev) => {
            const next = { ...prev };
            delete next[coreName];
            return next;
          });
        }, 4000);
      } else {
        throw new Error("Upload failed on server");
      }
    } catch (err: unknown) {
      setAction(
        coreName,
        "error",
        err instanceof Error ? err.message : String(err)
      );
    }
  };

  return (
    <div className="flex flex-col gap-6">
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between">
            <div>
              <CardTitle className="text-base flex items-center gap-2">
                <HardDrive className="size-4 text-primary" />
                {t("Core Programs & Carriers")}
              </CardTitle>
              <CardDescription className="mt-1">
                {t(
                  "Manage, auto-download or upload external binaries required for NextVPN's multi-protocol tunneling and routing."
                )}
              </CardDescription>
            </div>
            <Button
              variant="outline"
              size="sm"
              onClick={() => void refresh()}
              disabled={loading}
              className="gap-1.5 text-xs"
            >
              <RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} />
              {t("Refresh")}
            </Button>
          </div>
        </CardHeader>
        <CardContent className="flex flex-col gap-3.5">
          {fetchError && (
            <div className="flex items-center gap-2 rounded-md border border-destructive/40 bg-destructive/[0.08] px-3.5 py-2.5 text-[13px] text-destructive">
              <XCircle className="size-4 shrink-0" />
              <span>{fetchError}</span>
            </div>
          )}

          {inventory.map((core) => {
            const meta = CORE_METAS[core.name] ?? {
              title: `${core.name.toUpperCase()} Core`,
              role: "Unified proxy / tunnel binary",
              source: "Upstream Binary",
              canDownload: false,
              icon: HardDrive,
            };
            const Icon = meta.icon;
            const action = itemAction[core.name] ?? { state: "idle" };

            return (
              <div
                key={core.name}
                className="group relative flex flex-col gap-3 rounded-lg border bg-card p-4 transition-all hover:border-border/80 hover:shadow-sm"
              >
                {/* Hidden file upload input */}
                <input
                  type="file"
                  className="hidden"
                  ref={(el) => {
                    fileInputRefs.current[core.name] = el;
                  }}
                  onChange={(e) => void handleFileSelected(core.name, e)}
                />

                <div className="flex flex-wrap items-start justify-between gap-2">
                  <div className="flex items-start gap-3">
                    <div className="rounded-md border bg-muted/40 p-2 text-foreground/80 mt-0.5">
                      <Icon className="size-4.5" />
                    </div>
                    <div className="flex flex-col gap-0.5">
                      <div className="flex items-center gap-2 flex-wrap">
                        <span className="font-semibold text-sm">
                          {t(meta.title)}
                        </span>
                        {core.installed ? (
                          <Badge variant="ok" className="text-[11px] h-5">
                            <CheckCircle2 className="size-3" />
                            {t("Installed")}
                          </Badge>
                        ) : (
                          <Badge variant="warn" className="text-[11px] h-5">
                            <AlertCircle className="size-3" />
                            {t("Missing")}
                          </Badge>
                        )}
                        {core.version && (
                          <span className="font-mono text-[11.5px] text-muted-foreground">
                            {core.version}
                          </span>
                        )}
                      </div>
                      <p className="text-xs text-muted-foreground mt-0.5">
                        {t(meta.role)}
                      </p>
                    </div>
                  </div>

                  {/* Actions */}
                  <div className="flex items-center gap-2 self-start">
                    {meta.canDownload && (
                      <Button
                        size="sm"
                        variant={core.installed ? "outline" : "default"}
                        disabled={action.state === "working"}
                        onClick={() => void handleDownload(core.name)}
                        className="h-8 gap-1.5 text-xs font-medium"
                      >
                        <Download className="size-3.5" />
                        {core.installed ? t("Re-download") : t("Auto Download")}
                      </Button>
                    )}

                    <Button
                      size="sm"
                      variant="outline"
                      disabled={action.state === "working"}
                      onClick={() => handleUploadClick(core.name)}
                      className="h-8 gap-1.5 text-xs font-medium"
                    >
                      <Upload className="size-3.5" />
                      {t("Upload Binary")}
                    </Button>
                  </div>
                </div>

                {/* Path & Details */}
                <div className="flex flex-col gap-1 rounded-md bg-muted/30 p-2.5 text-xs text-muted-foreground">
                  <div className="flex items-center justify-between flex-wrap gap-1">
                    <span className="font-medium text-foreground/70">
                      {t("Source")}:{" "}
                      <span className="font-normal">{meta.source}</span>
                    </span>
                    {meta.sourceUrl && (
                      <a
                        href={meta.sourceUrl}
                        target="_blank"
                        rel="noreferrer"
                        className="inline-flex items-center gap-1 text-[11px] text-primary hover:underline"
                      >
                        {t("Upstream Link")}
                        <ExternalLink className="size-3" />
                      </a>
                    )}
                  </div>

                  {core.installed && core.path && (
                    <div className="break-all font-mono text-[11px]">
                      <span className="font-medium text-foreground/70">
                        {t("Binary Path")}:{" "}
                      </span>
                      {core.path}
                    </div>
                  )}

                  {meta.note && !core.installed && (
                    <div className="text-[11.5px] text-muted-foreground/90 italic pt-0.5">
                      💡 {t(meta.note)}
                    </div>
                  )}
                </div>

                {/* In-progress status */}
                {action.state === "working" && (
                  <div className="flex flex-col gap-1.5 pt-1">
                    <Progress className="h-1.5 w-full" />
                    <span className="text-[11.5px] text-muted-foreground animate-pulse">
                      {action.progressText ?? t("Processing…")}
                    </span>
                  </div>
                )}

                {/* Done status */}
                {action.state === "done" && action.msg && (
                  <div className="flex items-center gap-2 rounded-md border border-primary/30 bg-primary/10 px-3 py-1.5 text-xs text-primary">
                    <CheckCircle2 className="size-3.5 shrink-0" />
                    <span>{action.msg}</span>
                  </div>
                )}

                {/* Error status */}
                {action.state === "error" && action.msg && (
                  <div className="flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
                    <XCircle className="size-4 shrink-0 mt-0.5" />
                    <span className="break-words">{action.msg}</span>
                  </div>
                )}
              </div>
            );
          })}

          {inventory.length === 0 && !fetchError && (
            <div className="flex flex-col items-center justify-center py-8 text-center text-muted-foreground">
              <RefreshCw className="size-5 animate-spin mb-2" />
              <p className="text-xs">{t("Scanning binary inventory…")}</p>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
