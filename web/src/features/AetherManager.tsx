import { useEffect, useState, useCallback } from "react";
import {
  ShieldCheck, Globe, Zap, Play, Square, Settings2, Plus,
  Copy, Trash2, CheckCircle2, AlertCircle, Loader2, Sparkles, Check,
  Radio, Lock, Layers, EyeOff, Activity
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import {
  getAetherProfiles,
  saveAetherProfile,
  duplicateAetherProfile,
  deleteAetherProfile,
  setAetherActiveProfile,
  getCoreStatus,
  probeCore,
  startCore,
  stopCore,
  type AetherProfileConfig,
} from "@/core/api";
import type { CoreProbe, CoreSnapshot } from "@/types";

interface AetherManagerProps {
  onToast?: (title: string, message: string, error?: boolean) => void;
}

export function AetherManager({ onToast }: AetherManagerProps) {

  const [profiles, setProfiles] = useState<AetherProfileConfig[]>([]);
  const [selectedId, setSelectedId] = useState<string>("default");
  const [current, setCurrent] = useState<AetherProfileConfig | null>(null);
  const [loading, setLoading] = useState(true);
  const [actionLoading, setActionLoading] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [successMsg, setSuccessMsg] = useState<string | null>(null);

  // Probe and Snapshot for live test status
  const [probe, setProbe] = useState<CoreProbe | null>(null);
  const [snapshot, setSnapshot] = useState<CoreSnapshot | null>(null);

  // New profile modal state
  const [isCreating, setIsCreating] = useState(false);
  const [newProfileName, setNewProfileName] = useState("");

  const showSuccess = (msg: string) => {
    setSuccessMsg(msg);
    setTimeout(() => setSuccessMsg(null), 4000);
  };

  const loadProfiles = useCallback(async (preferId?: string) => {
    try {
      const list = await getAetherProfiles();
      setProfiles(list);
      const active = list.find((p) => p.isActive) || list[0];
      const targetId = preferId || (list.some((p) => p.id === selectedId) ? selectedId : active?.id || "default");
      setSelectedId(targetId);
      const sel = list.find((p) => p.id === targetId) || active || list[0] || null;
      if (sel) {
        setCurrent({ ...sel });
      }
    } catch (e) {
      console.error("Failed to load aether profiles:", e);
      setError(e instanceof Error ? e.message : "无法加载 Aether 方案列表");
    } finally {
      setLoading(false);
    }
  }, [selectedId]);

  const loadStatus = useCallback(async () => {
    try {
      const [stat, p] = await Promise.allSettled([
        getCoreStatus(),
        probeCore(current ? ({ ...current, carriers: { first: "aether", second: null }, chain: { enabled: false } } as any) : ({} as any)),
      ]);
      if (stat.status === "fulfilled") setSnapshot(stat.value);
      if (p.status === "fulfilled") setProbe(p.value);
    } catch (e) {
      // Ignore background probe failure
    }
  }, [current]);

  useEffect(() => {
    void loadProfiles();
  }, [loadProfiles]);

  useEffect(() => {
    void loadStatus();
    const timer = setInterval(() => void loadStatus(), 4000);
    return () => clearInterval(timer);
  }, [loadStatus]);

  const handleSelectProfile = (id: string) => {
    setSelectedId(id);
    const found = profiles.find((p) => p.id === id);
    if (found) {
      setCurrent({ ...found });
    }
  };

  // 1-Click Scenario Preset appliers
  const applyPreset = (presetType: "overseas" | "stealth" | "gfw" | "wg") => {
    if (!current) return;
    if (presetType === "overseas") {
      setCurrent({
        ...current,
        protocol: "masque",
        masqueTransport: "h3",
        noize: "off",
        fragmentClientHello: false,
        scanMode: "turbo",
        quickReconnect: true,
        keepaliveSecs: 25,
        description: "关闭混流与分片，QUIC 传输，适合无封锁海外服务器直连，低开销低延迟",
      });
      showSuccess("已应用场景预设：⚡ 海外 VPS 极速直连");
    } else if (presetType === "stealth") {
      setCurrent({
        ...current,
        protocol: "masque",
        masqueTransport: "h2",
        noize: "firewall",
        fragmentClientHello: true,
        fragmentSize: "16-32",
        fragmentDelay: "2-10",
        scanMode: "balanced",
        quickReconnect: true,
        keepaliveSecs: 25,
        description: "开启 TLS ClientHello 分片 (16-32B)、HTTP/2 传输与 Firewall 混淆流，穿透受控网络",
      });
      showSuccess("已应用场景预设：🛡️ 受管控 / 抗封锁 H2 混流");
    } else if (presetType === "gfw") {
      setCurrent({
        ...current,
        protocol: "masque",
        masqueTransport: "h2",
        noize: "gfw",
        fragmentClientHello: true,
        fragmentSize: "8-16",
        fragmentDelay: "5-15",
        scanMode: "stealth",
        quickReconnect: true,
        keepaliveSecs: 25,
        description: "极细粒度 TLS 分片 (8-16B) 与 GFW 深度混淆流，对抗严苛审查",
      });
      showSuccess("已应用场景预设：🚀 GFW 深度穿透 (高强度对抗)");
    } else if (presetType === "wg") {
      setCurrent({
        ...current,
        protocol: "wg",
        masqueTransport: "h2",
        noize: "off",
        fragmentClientHello: false,
        scanMode: "balanced",
        keepaliveSecs: 25,
        description: "经典 WireGuard UDP 隧道协议，直连 Cloudflare Warp 端点",
      });
      showSuccess("已应用场景预设：🔒 WireGuard 原生直连");
    }
  };

  const handleSave = async () => {
    if (!current) return;
    setActionLoading("save");
    setError(null);
    try {
      const saved = await saveAetherProfile(current);
      showSuccess(`方案 [${saved.name}] 配置已成功保存！`);
      await loadProfiles(saved.id);
      onToast?.("保存成功", `Aether 方案 [${saved.name}] 已写入配置文件`);
    } catch (e: any) {
      setError(e.message || "保存失败");
    } finally {
      setActionLoading(null);
    }
  };

  const handleSetActive = async () => {
    if (!current) return;
    setActionLoading("set_active");
    setError(null);
    try {
      await setAetherActiveProfile(current.id);
      showSuccess(`已将 [${current.name}] 设为系统默认激活方案！`);
      await loadProfiles(current.id);
      onToast?.("方案切换", `当前活动方案已切换至: ${current.name}`);
    } catch (e: any) {
      setError(e.message || "切换方案失败");
    } finally {
      setActionLoading(null);
    }
  };

  const handleDuplicate = async () => {
    if (!current) return;
    const name = window.prompt("请输入克隆方案名称:", `${current.name} (Copy)`);
    if (!name || !name.trim()) return;
    setActionLoading("duplicate");
    try {
      const dup = await duplicateAetherProfile(current.id, name.trim());
      showSuccess(`已成功克隆方案: ${dup.name}`);
      await loadProfiles(dup.id);
      onToast?.("克隆成功", `已创建方案副本: ${dup.name}`);
    } catch (e: any) {
      setError(e.message || "克隆失败");
    } finally {
      setActionLoading(null);
    }
  };

  const handleDelete = async () => {
    if (!current) return;
    if (profiles.length <= 1) {
      alert("无法删除最后一个方案！");
      return;
    }
    if (!window.confirm(`确定要删除 Aether 方案 [${current.name}] 吗？`)) return;
    setActionLoading("delete");
    try {
      await deleteAetherProfile(current.id);
      showSuccess(`已删除方案 [${current.name}]`);
      await loadProfiles();
      onToast?.("已删除", `Aether 方案 [${current.name}] 已移除`);
    } catch (e: any) {
      setError(e.message || "删除失败");
    } finally {
      setActionLoading(null);
    }
  };

  const handleCreateNew = async () => {
    const name = newProfileName.trim();
    if (!name) return;
    setActionLoading("create");
    try {
      const rawId = name.toLowerCase().replace(/[^a-z0-9]/g, "-").replace(/^-+|-+$/g, "") || `profile-${Date.now()}`;
      const newProf: AetherProfileConfig = {
        ...current!,
        id: rawId,
        name,
        description: "自定义 Aether 连接参数方案",
        isActive: false,
      };
      const saved = await saveAetherProfile(newProf);
      setNewProfileName("");
      setIsCreating(false);
      showSuccess(`已成功新建方案: ${saved.name}`);
      await loadProfiles(saved.id);
      onToast?.("创建成功", `已新增 Aether 方案: ${saved.name}`);
    } catch (e: any) {
      setError(e.message || "创建失败");
    } finally {
      setActionLoading(null);
    }
  };

  const handleStartTest = async () => {
    if (!current) return;
    setActionLoading("test_start");
    setError(null);
    try {
      // Start core with this profile's configuration
      const snap = await startCore({
        ...current,
        carriers: { first: "aether", second: null },
        chain: { enabled: false, throughTunnel: false, sources: [], manual: "", node: null },
      } as any);
      setSnapshot(snap);
      showSuccess(`Aether 已启动测试！SOCKS 监听于: ${snap.socksAddress}`);
      onToast?.("Aether 测试启动", `已成功拉起内核，监听地址: ${snap.socksAddress}`);
    } catch (e: any) {
      setError(e.message || "启动测试失败");
    } finally {
      setActionLoading(null);
    }
  };

  const handleStopTest = async () => {
    setActionLoading("test_stop");
    try {
      await stopCore();
      showSuccess("Aether 测试进程已停止");
      onToast?.("已停止", "Aether 引擎测试已停止");
    } catch (e: any) {
      setError(e.message || "停止失败");
    } finally {
      setActionLoading(null);
    }
  };

  if (loading && !current) {
    return (
      <div className="p-12 text-center text-sm text-muted-foreground flex items-center justify-center gap-2">
        <Loader2 className="w-4 h-4 animate-spin text-primary" />
        正在加载 Aether 连接参数与方案配置...
      </div>
    );
  }

  const isConnected = snapshot?.state === "connected";
  const isCoreRunning = snapshot?.state === "connected" || snapshot?.state === "connecting";

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

      {/* ── 顶部引擎状态总览面板 (参考 ProtonManager 风格) ───────────────── */}
      <Card className="border-border/60 shadow-sm bg-gradient-to-r from-card to-card/60">
        <CardContent className="p-4 grid grid-cols-2 sm:grid-cols-4 gap-4">
          <div className="flex flex-col gap-1">
            <span className="text-xs text-muted-foreground flex items-center gap-1.5">
              <ShieldCheck className="w-3.5 h-3.5 text-blue-500" />
              Aether 内核状态
            </span>
            <div className="flex items-center gap-2 mt-0.5">
              {probe?.available ? (
                <Badge variant="outline" className="bg-emerald-500/10 text-emerald-500 border-emerald-500/20 font-mono text-xs">
                  <Check className="w-3 h-3 mr-1" /> 已就绪 ({probe.version || "Aether"})
                </Badge>
              ) : (
                <Badge variant="destructive" className="text-xs">未检测到核心</Badge>
              )}
            </div>
          </div>

          <div className="flex flex-col gap-1">
            <span className="text-xs text-muted-foreground flex items-center gap-1.5">
              <Activity className="w-3.5 h-3.5 text-purple-500" />
              核心运行状态
            </span>
            <div className="flex items-center gap-2 mt-0.5">
              <Badge
                variant="outline"
                className={
                  isConnected
                    ? "bg-emerald-500/10 text-emerald-500 border-emerald-500/20"
                    : isCoreRunning
                    ? "bg-amber-500/10 text-amber-500 border-amber-500/20"
                    : "bg-muted text-muted-foreground"
                }
              >
                {snapshot?.state || "idle"}
              </Badge>
              {snapshot?.pid && <span className="font-mono text-xs text-muted-foreground">PID: {snapshot.pid}</span>}
            </div>
          </div>

          <div className="flex flex-col gap-1">
            <span className="text-xs text-muted-foreground flex items-center gap-1.5">
              <Globe className="w-3.5 h-3.5 text-emerald-500" />
              出口边缘节点
            </span>
            <span className="font-mono text-xs font-semibold text-foreground/90 truncate mt-1" title={snapshot?.endpoint ?? "未连接"}>
              {snapshot?.endpoint ?? "—"}
            </span>
          </div>

          <div className="flex flex-col gap-1">
            <span className="text-xs text-muted-foreground flex items-center gap-1.5">
              <Zap className="w-3.5 h-3.5 text-amber-500" />
              实测延迟
            </span>
            <span className="font-mono text-xs font-semibold text-foreground/90 mt-1">
              {snapshot?.latencyMs ? `${snapshot.latencyMs.toFixed(1)} ms` : "—"}
            </span>
          </div>
        </CardContent>

        <div className="border-t border-border/40 px-4 py-2.5 bg-muted/20 flex flex-wrap items-center justify-between gap-3">
          <span className="text-xs text-muted-foreground">
            Aether 为 Cloudflare WARP 核心，单 VPS 支持通过不同参数方案启动监听端口
          </span>
          <div className="flex items-center gap-2">
            {isCoreRunning ? (
              <Button
                variant="destructive"
                size="sm"
                className="h-7 text-xs gap-1.5"
                disabled={actionLoading === "test_stop"}
                onClick={handleStopTest}
              >
                <Square className="w-3 h-3" />
                停止测试
              </Button>
            ) : (
              <Button
                variant="outline"
                size="sm"
                className="h-7 text-xs gap-1.5 border-primary/30 text-primary hover:bg-primary/10"
                disabled={actionLoading === "test_start" || !probe?.available}
                onClick={handleStartTest}
              >
                <Play className="w-3 h-3" />
                测试启动当前方案
              </Button>
            )}
          </div>
        </div>
      </Card>

      {/* ── 方案选择与管理工具栏 (Profile Selector & Management) ─────────── */}
      <Card className="border-border/60 shadow-sm">
        <CardHeader className="pb-3">
          <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3">
            <div>
              <CardTitle className="text-[16px] flex items-center gap-2">
                <Layers className="w-4 h-4 text-primary" />
                Aether 参数方案管理 (Profiles)
              </CardTitle>
              <CardDescription className="text-xs mt-0.5">
                基于 <code className="text-[11px] font-mono bg-muted px-1.5 py-0.5 rounded">config/aether_profiles.json</code> 统一存储，可在 SOCKS5 管理中为每个出口端口绑定特定方案
              </CardDescription>
            </div>

            <div className="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                className="h-8 text-xs gap-1"
                onClick={() => setIsCreating(true)}
              >
                <Plus className="w-3.5 h-3.5" />
                新建方案
              </Button>
              <Button
                variant="outline"
                size="sm"
                className="h-8 text-xs gap-1"
                onClick={handleDuplicate}
                disabled={!current}
              >
                <Copy className="w-3.5 h-3.5" />
                克隆
              </Button>
              {profiles.length > 1 && (
                <Button
                  variant="outline"
                  size="sm"
                  className="h-8 text-xs gap-1 text-destructive hover:bg-destructive/10 hover:text-destructive"
                  onClick={handleDelete}
                  disabled={!current}
                >
                  <Trash2 className="w-3.5 h-3.5" />
                  删除
                </Button>
              )}
            </div>
          </div>
        </CardHeader>

        <CardContent className="space-y-3">
          {/* Profile Switcher Row */}
          <div className="flex flex-wrap items-center gap-2">
            {profiles.map((p) => {
              const isSel = p.id === selectedId;
              return (
                <button
                  key={p.id}
                  type="button"
                  onClick={() => handleSelectProfile(p.id)}
                  className={[
                    "flex items-center gap-2 px-3 py-1.5 rounded-lg border text-xs transition-all",
                    isSel
                      ? "border-primary bg-primary/10 text-primary font-semibold ring-1 ring-primary/30"
                      : "border-border/60 bg-card hover:bg-accent text-muted-foreground",
                  ].join(" ")}
                >
                  <span>{p.name}</span>
                  {p.isActive && (
                    <span className="text-[10px] bg-primary text-primary-foreground px-1.5 py-0.2 rounded font-medium">
                      默认激活
                    </span>
                  )}
                </button>
              );
            })}
          </div>

          {current && (
            <div className="flex flex-wrap items-center justify-between gap-2 pt-2 border-t border-border/40 text-xs text-muted-foreground">
              <div className="flex items-center gap-2">
                <span>方案标识 (ID): <code className="font-mono text-foreground font-semibold">{current.id}</code></span>
                <span>·</span>
                <span>协议: <span className="uppercase text-foreground font-medium">{current.protocol}</span> ({current.masqueTransport})</span>
                <span>·</span>
                <span>混流: <span className="capitalize text-foreground font-medium">{current.noize}</span></span>
                <span>·</span>
                <span>分片: <span className="text-foreground font-medium">{current.fragmentClientHello ? `开启 (${current.fragmentSize})` : "关闭"}</span></span>
              </div>
              {!current.isActive && (
                <Button
                  variant="ghost"
                  size="sm"
                  className="h-7 text-xs text-primary hover:bg-primary/10 gap-1 p-0 px-2"
                  onClick={handleSetActive}
                  disabled={actionLoading === "set_active"}
                >
                  <Check className="w-3 h-3" />
                  设为系统默认方案
                </Button>
              )}
            </div>
          )}
        </CardContent>
      </Card>

      {/* ── 快速场景预设卡片 (Scenario Presets - 1-Click Setup) ─────────── */}
      <Card className="border-border/60 shadow-sm">
        <CardHeader className="pb-3">
          <CardTitle className="text-[15px] flex items-center gap-2">
            <Sparkles className="w-4 h-4 text-amber-500" />
            一键场景参数预设
          </CardTitle>
          <CardDescription className="text-xs">
            根据 VPS 实际部署网络环境快速套用最佳连接参数组合
          </CardDescription>
        </CardHeader>

        <CardContent className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-3">
          {/* Preset 1: 海外直连 */}
          <div
            onClick={() => applyPreset("overseas")}
            className="group cursor-pointer rounded-lg border border-border/70 p-3 hover:border-primary/50 hover:bg-primary/5 transition-all flex flex-col justify-between"
          >
            <div>
              <div className="flex items-center justify-between mb-1.5">
                <span className="font-semibold text-xs text-foreground flex items-center gap-1.5">
                  <Zap className="w-3.5 h-3.5 text-emerald-500" />
                  海外 VPS 极速直连
                </span>
                <Badge variant="outline" className="text-[10px] bg-emerald-500/10 text-emerald-600 border-emerald-500/20">低延迟</Badge>
              </div>
              <p className="text-[11px] text-muted-foreground leading-relaxed">
                关闭所有混流与分片，采用 MASQUE H3 (QUIC) 直连。最小化 CPU 与网络开销，延迟最低、吞吐最大。
              </p>
            </div>
            <div className="mt-3 pt-2 border-t border-border/40 text-[10px] text-muted-foreground flex justify-between">
              <span>MASQUE H3 · Noize: Off</span>
              <span className="text-primary font-medium group-hover:underline">点击应用 →</span>
            </div>
          </div>

          {/* Preset 2: 受管控抗封锁 */}
          <div
            onClick={() => applyPreset("stealth")}
            className="group cursor-pointer rounded-lg border border-border/70 p-3 hover:border-primary/50 hover:bg-primary/5 transition-all flex flex-col justify-between"
          >
            <div>
              <div className="flex items-center justify-between mb-1.5">
                <span className="font-semibold text-xs text-foreground flex items-center gap-1.5">
                  <ShieldCheck className="w-3.5 h-3.5 text-blue-500" />
                  受管控 / 抗封锁 H2
                </span>
                <Badge variant="outline" className="text-[10px] bg-blue-500/10 text-blue-600 border-blue-500/20">推荐穿透</Badge>
              </div>
              <p className="text-[11px] text-muted-foreground leading-relaxed">
                开启 MASQUE H2 (TLS) 传输，启用 TLS ClientHello 分片 (16-32B) 与 Firewall 混流，抗深度探测。
              </p>
            </div>
            <div className="mt-3 pt-2 border-t border-border/40 text-[10px] text-muted-foreground flex justify-between">
              <span>H2 + 分片 + Firewall</span>
              <span className="text-primary font-medium group-hover:underline">点击应用 →</span>
            </div>
          </div>

          {/* Preset 3: GFW 深度穿透 */}
          <div
            onClick={() => applyPreset("gfw")}
            className="group cursor-pointer rounded-lg border border-border/70 p-3 hover:border-primary/50 hover:bg-primary/5 transition-all flex flex-col justify-between"
          >
            <div>
              <div className="flex items-center justify-between mb-1.5">
                <span className="font-semibold text-xs text-foreground flex items-center gap-1.5">
                  <Radio className="w-3.5 h-3.5 text-purple-500" />
                  GFW 深度穿透
                </span>
                <Badge variant="outline" className="text-[10px] bg-purple-500/10 text-purple-600 border-purple-500/20">强力混淆</Badge>
              </div>
              <p className="text-[11px] text-muted-foreground leading-relaxed">
                微粒度 TLS 分片 (8-16B, 5-15ms 延迟) 配合 GFW 级混淆流与 Stealth 扫描，用于严苛封锁环境。
              </p>
            </div>
            <div className="mt-3 pt-2 border-t border-border/40 text-[10px] text-muted-foreground flex justify-between">
              <span>H2 + 细分片 + GFW 流</span>
              <span className="text-primary font-medium group-hover:underline">点击应用 →</span>
            </div>
          </div>

          {/* Preset 4: WireGuard 原生 */}
          <div
            onClick={() => applyPreset("wg")}
            className="group cursor-pointer rounded-lg border border-border/70 p-3 hover:border-primary/50 hover:bg-primary/5 transition-all flex flex-col justify-between"
          >
            <div>
              <div className="flex items-center justify-between mb-1.5">
                <span className="font-semibold text-xs text-foreground flex items-center gap-1.5">
                  <Lock className="w-3.5 h-3.5 text-amber-500" />
                  WireGuard 原生直连
                </span>
                <Badge variant="outline" className="text-[10px] bg-amber-500/10 text-amber-600 border-amber-500/20">标准 WG</Badge>
              </div>
              <p className="text-[11px] text-muted-foreground leading-relaxed">
                标准 WireGuard UDP 隧道协议，直连 Cloudflare WARP Anycast 节点，适合支持原生 UDP 转发的优质网络。
              </p>
            </div>
            <div className="mt-3 pt-2 border-t border-border/40 text-[10px] text-muted-foreground flex justify-between">
              <span>WireGuard · UDP 原生</span>
              <span className="text-primary font-medium group-hover:underline">点击应用 →</span>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* ── 详细参数调整表单 (Aether Detailed Connection Parameters) ─────── */}
      {current && (
        <div className="space-y-4">
          {/* Card 1: 核心协议与传输 */}
          <Card className="border-border/60 shadow-sm">
            <CardHeader className="pb-3">
              <CardTitle className="text-[15px] flex items-center gap-2">
                <Settings2 className="w-4 h-4 text-primary" />
                核心协议与传输方式
              </CardTitle>
              <CardDescription className="text-xs">
                配置 Aether 连接 Cloudflare 边缘网络的隧道协议与底层封装
              </CardDescription>
            </CardHeader>

            <CardContent className="space-y-4">
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                <div>
                  <label className="text-xs font-semibold text-foreground mb-1 block">方案名称</label>
                  <Input
                    value={current.name}
                    onChange={(e) => setCurrent({ ...current, name: e.target.value })}
                    placeholder="例如: 海外 VPS 极速直连"
                    className="h-8 text-xs"
                  />
                </div>
                <div>
                  <label className="text-xs font-semibold text-foreground mb-1 block">方案简要描述</label>
                  <Input
                    value={current.description || ""}
                    onChange={(e) => setCurrent({ ...current, description: e.target.value })}
                    placeholder="说明此方案的适用网络场景"
                    className="h-8 text-xs"
                  />
                </div>
              </div>

              <div className="grid grid-cols-1 sm:grid-cols-3 gap-4 pt-2 border-t border-border/40">
                <div>
                  <label className="text-xs font-semibold text-foreground mb-1 block">核心协议模式</label>
                  <select
                    value={current.protocol}
                    onChange={(e) => setCurrent({ ...current, protocol: e.target.value as any })}
                    className="flex h-8 w-full rounded-md border border-input bg-background px-2.5 py-1 text-xs font-medium shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <option value="masque">MASQUE (HTTP 代理隧道 - 推荐)</option>
                    <option value="wg">WireGuard (标准 UDP 协议)</option>
                    <option value="gool">Gool (极简代理协议)</option>
                  </select>
                </div>

                {current.protocol === "masque" && (
                  <div>
                    <label className="text-xs font-semibold text-foreground mb-1 block">MASQUE 传输层协议</label>
                    <select
                      value={current.masqueTransport}
                      onChange={(e) => setCurrent({ ...current, masqueTransport: e.target.value as any })}
                      className="flex h-8 w-full rounded-md border border-input bg-background px-2.5 py-1 text-xs font-medium shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                    >
                      <option value="h2">HTTP/2 over TLS (高抗封锁/支持分片)</option>
                      <option value="h3">HTTP/3 over QUIC (海外直连/低延迟)</option>
                    </select>
                  </div>
                )}

                <div>
                  <label className="text-xs font-semibold text-foreground mb-1 block">IP 协议族支持</label>
                  <select
                    value={current.ipFamily}
                    onChange={(e) => setCurrent({ ...current, ipFamily: e.target.value as any })}
                    className="flex h-8 w-full rounded-md border border-input bg-background px-2.5 py-1 text-xs font-medium shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <option value="both">IPv4 + IPv6 双栈 (推荐)</option>
                    <option value="v4">仅 IPv4</option>
                    <option value="v6">仅 IPv6</option>
                  </select>
                </div>
              </div>

              <div>
                <label className="text-xs font-semibold text-foreground mb-1 block">本地默认 SOCKS5 监听地址</label>
                <Input
                  value={current.socksAddress}
                  onChange={(e) => setCurrent({ ...current, socksAddress: e.target.value })}
                  placeholder="127.0.0.1:1819"
                  className="h-8 text-xs font-mono max-w-sm"
                />
                <p className="text-[11px] text-muted-foreground mt-1">
                  Aether 启动时本地混杂端口，SOCKS5 管理面板中的各个独立端口将通过桥接安全转发至此
                </p>
              </div>
            </CardContent>
          </Card>

          {/* Card 2: 抗审查与混淆混流 */}
          <Card className="border-border/60 shadow-sm">
            <CardHeader className="pb-3">
              <CardTitle className="text-[15px] flex items-center gap-2">
                <EyeOff className="w-4 h-4 text-purple-500" />
                抗审查、混淆与流量伪装
              </CardTitle>
              <CardDescription className="text-xs">
                针对审查环境开启 Noize 混流及 TLS ClientHello 分片，彻底打破固定流量指纹特征
              </CardDescription>
            </CardHeader>

            <CardContent className="space-y-4">
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                <div>
                  <label className="text-xs font-semibold text-foreground mb-1 block">Noize 混流干扰强度</label>
                  <select
                    value={current.noize}
                    onChange={(e) => setCurrent({ ...current, noize: e.target.value as any })}
                    className="flex h-8 w-full rounded-md border border-input bg-background px-2.5 py-1 text-xs font-medium shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <option value="off">关闭 (海外原生环境推荐，零干扰)</option>
                    <option value="light">轻度干扰 (Light)</option>
                    <option value="balanced">平衡模式 (Balanced)</option>
                    <option value="firewall">防火墙穿透 (Firewall - 推荐受限网络)</option>
                    <option value="gfw">深度对抗 (GFW 深度模式)</option>
                    <option value="aggressive">激进对抗 (Aggressive)</option>
                  </select>
                </div>

                <div className="flex flex-col justify-end">
                  <div className="flex items-center justify-between rounded-lg border border-border/70 bg-muted/20 p-2.5">
                    <div>
                      <span className="text-xs font-semibold text-foreground block">TLS ClientHello 报文分片</span>
                      <span className="text-[11px] text-muted-foreground">将 TLS 握手特征报文拆分成多个微小数据包</span>
                    </div>
                    <Switch
                      checked={current.fragmentClientHello}
                      onCheckedChange={(val) => setCurrent({ ...current, fragmentClientHello: val })}
                      disabled={current.protocol !== "masque" || current.masqueTransport !== "h2"}
                    />
                  </div>
                </div>
              </div>

              {current.fragmentClientHello && (
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 p-3 bg-muted/30 rounded-lg border border-border/50">
                  <div>
                    <label className="text-xs font-semibold text-foreground mb-1 block">分片数据包大小 (Fragment Size)</label>
                    <Input
                      value={current.fragmentSize}
                      onChange={(e) => setCurrent({ ...current, fragmentSize: e.target.value })}
                      placeholder="16-32"
                      className="h-8 text-xs font-mono"
                    />
                    <p className="text-[11px] text-muted-foreground mt-1">每个分片包的随机字节区间，例如 16-32 或 8-16</p>
                  </div>
                  <div>
                    <label className="text-xs font-semibold text-foreground mb-1 block">分片间隔延迟毫秒 (Fragment Delay)</label>
                    <Input
                      value={current.fragmentDelay}
                      onChange={(e) => setCurrent({ ...current, fragmentDelay: e.target.value })}
                      placeholder="2-10"
                      className="h-8 text-xs font-mono"
                    />
                    <p className="text-[11px] text-muted-foreground mt-1">分片包之间随机延时，例如 2-10 或 5-15</p>
                  </div>
                </div>
              )}
            </CardContent>
          </Card>

          {/* Card 3: 节点扫描与端点锁定 */}
          <Card className="border-border/60 shadow-sm">
            <CardHeader className="pb-3">
              <CardTitle className="text-[15px] flex items-center gap-2">
                <Globe className="w-4 h-4 text-emerald-500" />
                节点扫描优选与端点锁定
              </CardTitle>
              <CardDescription className="text-xs">
                控制 Aether 启动时对 Cloudflare 边缘节点的搜索激进程度或指定直连端点
              </CardDescription>
            </CardHeader>

            <CardContent className="space-y-4">
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                <div>
                  <label className="text-xs font-semibold text-foreground mb-1 block">优选扫描模式 (Scan Mode)</label>
                  <select
                    value={current.scanMode}
                    onChange={(e) => setCurrent({ ...current, scanMode: e.target.value as any })}
                    className="flex h-8 w-full rounded-md border border-input bg-background px-2.5 py-1 text-xs font-medium shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <option value="turbo">极速模式 (Turbo - 海外主机秒连)</option>
                    <option value="balanced">平衡模式 (Balanced - 兼顾速度与质量)</option>
                    <option value="thorough">详尽模式 (Thorough - 扫描更多优选)</option>
                    <option value="stealth">隐蔽模式 (Stealth - 慢速低探测)</option>
                    <option value="ironclad">强固模式 (Ironclad - 最低丢包优先)</option>
                  </select>
                </div>

                <div>
                  <label className="text-xs font-semibold text-foreground mb-1 block">端点选择策略 (Endpoint Mode)</label>
                  <select
                    value={current.endpointMode}
                    onChange={(e) => setCurrent({ ...current, endpointMode: e.target.value as any })}
                    className="flex h-8 w-full rounded-md border border-input bg-background px-2.5 py-1 text-xs font-medium shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <option value="automatic">自动扫描 (Automatic - 自动挑选最佳边缘)</option>
                    <option value="custom-first">优先使用指定端点 (Custom-First)</option>
                    <option value="custom-only">强制仅使用指定端点 (Custom-Only)</option>
                  </select>
                </div>
              </div>

              {current.endpointMode !== "automatic" && (
                <div>
                  <label className="text-xs font-semibold text-foreground mb-1 block">指定 Cloudflare Warp 端点地址 (IP:Port)</label>
                  <Input
                    value={current.peer || ""}
                    onChange={(e) => setCurrent({ ...current, peer: e.target.value })}
                    placeholder="例如: 162.159.192.1:2408"
                    className="h-8 text-xs font-mono max-w-sm"
                  />
                </div>
              )}
            </CardContent>
          </Card>

          {/* Card 4: DNS 与网络保活 */}
          <Card className="border-border/60 shadow-sm">
            <CardHeader className="pb-3">
              <CardTitle className="text-[15px] flex items-center gap-2">
                <Zap className="w-4 h-4 text-amber-500" />
                DNS 设置与网络保活
              </CardTitle>
              <CardDescription className="text-xs">
                保持长时间无流量下的连接心跳及 DNS 解析地址
              </CardDescription>
            </CardHeader>

            <CardContent className="space-y-4">
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                <div>
                  <label className="text-xs font-semibold text-foreground mb-1 block">DNS 服务器 (逗号分隔)</label>
                  <Input
                    value={current.dns.join(", ")}
                    onChange={(e) =>
                      setCurrent({
                        ...current,
                        dns: e.target.value.split(",").map((s) => s.trim()).filter(Boolean),
                      })
                    }
                    placeholder="1.1.1.1, 1.0.0.1"
                    className="h-8 text-xs font-mono"
                  />
                </div>

                <div>
                  <label className="text-xs font-semibold text-foreground mb-1 block">Keepalive 心跳保活 (秒)</label>
                  <Input
                    type="number"
                    value={current.keepaliveSecs}
                    onChange={(e) => setCurrent({ ...current, keepaliveSecs: parseInt(e.target.value, 10) || 25 })}
                    className="h-8 text-xs font-mono"
                  />
                </div>
              </div>

              <div className="grid grid-cols-1 sm:grid-cols-3 gap-3 pt-2 border-t border-border/40">
                <div className="flex items-center justify-between rounded-lg border border-border/70 bg-muted/20 p-2.5">
                  <div>
                    <span className="text-xs font-semibold text-foreground block">快速重连 (Quick Reconnect)</span>
                    <span className="text-[10px] text-muted-foreground">网络波动时瞬间恢复会话</span>
                  </div>
                  <Switch
                    checked={current.quickReconnect}
                    onCheckedChange={(val) => setCurrent({ ...current, quickReconnect: val })}
                  />
                </div>

                <div className="flex items-center justify-between rounded-lg border border-border/70 bg-muted/20 p-2.5">
                  <div>
                    <span className="text-xs font-semibold text-foreground block">数据包有效性校验</span>
                    <span className="text-[10px] text-muted-foreground">丢弃异常畸变数据包</span>
                  </div>
                  <Switch
                    checked={current.dataCheck}
                    onCheckedChange={(val) => setCurrent({ ...current, dataCheck: val })}
                  />
                </div>

                <div>
                  <label className="text-xs font-semibold text-foreground mb-1 block">日志输出级别</label>
                  <select
                    value={current.logLevel}
                    onChange={(e) => setCurrent({ ...current, logLevel: e.target.value })}
                    className="flex h-8 w-full rounded-md border border-input bg-background px-2 py-1 text-xs font-medium shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    <option value="info">Info (正常信息)</option>
                    <option value="warn">Warn (仅警告)</option>
                    <option value="error">Error (仅错误)</option>
                    <option value="debug">Debug (详细排查)</option>
                    <option value="trace">Trace (全量跟踪)</option>
                  </select>
                </div>
              </div>
            </CardContent>
          </Card>

          {/* 底部保存操作按钮栏 */}
          <div className="flex items-center justify-end gap-3 pt-4">
            <Button
              variant="outline"
              size="sm"
              onClick={() => handleSelectProfile(current.id)}
              disabled={actionLoading === "save"}
            >
              放弃更改
            </Button>
            <Button
              size="sm"
              onClick={handleSave}
              disabled={actionLoading === "save"}
              className="gap-1.5 px-5"
            >
              {actionLoading === "save" ? (
                <>
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                  正在保存方案...
                </>
              ) : (
                <>
                  <Check className="w-3.5 h-3.5" />
                  保存当前方案设置
                </>
              )}
            </Button>
          </div>
        </div>
      )}

      {/* ── 新建方案对话框 (Modal) ───────────────────────────────────────── */}
      {isCreating && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-background/80 backdrop-blur-sm p-4">
          <div className="w-full max-w-md rounded-xl border border-border/80 bg-card p-6 shadow-2xl">
            <h3 className="text-base font-semibold text-foreground mb-1">新建 Aether 连接方案</h3>
            <p className="text-xs text-muted-foreground mb-4">
              创建一个全新的方案以独立调整传输协议、混淆及分片等配置
            </p>
            <div className="space-y-3">
              <div>
                <label className="text-xs font-semibold text-foreground mb-1 block">方案名称</label>
                <Input
                  value={newProfileName}
                  onChange={(e) => setNewProfileName(e.target.value)}
                  placeholder="例如: 欧洲节点低延迟方案"
                  className="h-9 text-xs"
                  autoFocus
                />
              </div>
            </div>
            <div className="mt-6 flex items-center justify-end gap-2 pt-3 border-t border-border/50">
              <Button variant="outline" size="sm" onClick={() => setIsCreating(false)}>
                取消
              </Button>
              <Button size="sm" onClick={handleCreateNew} disabled={!newProfileName.trim()}>
                确认创建
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
