import { useState, useEffect, useCallback } from "react";
import {
  Edit2,
  Lock,
  Play,
  Plus,
  RefreshCw,
  Server,
  Square,
  Trash2,
  Unlock,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import {
  listSocksInstances,
  createSocksInstance,
  updateSocksInstance,
  deleteSocksInstance,
  startSocksInstance,
  stopSocksInstance,
  testSocksConnectivity,
  setSocksAutostart,
  getAetherProfiles,
  type AetherProfileConfig,
  type SocksInstanceConfig,
  type SocksInstanceView,
} from "@/core/api";
import type { SocksUpstreamType } from "@/types";

interface SocksManagerProps {
  onToast: (title: string, message: string, error?: boolean) => void;
  onViewDashboard?: () => void;
}

export function SocksManager({ onToast, onViewDashboard }: SocksManagerProps) {
  const [instances, setInstances] = useState<SocksInstanceView[]>([]);
  const [loading, setLoading] = useState(true);
  const [actionLoading, setActionLoading] = useState<Record<string, boolean>>({});
  const [testingConn, setTestingConn] = useState<Record<string, boolean>>({});

  // Modal State
  const [modalOpen, setModalOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [formName, setFormName] = useState("");
  const [formPort, setFormPort] = useState<number>(1081);
  const [formHost, setFormHost] = useState("0.0.0.0");
  const [formEnableAuth, setFormEnableAuth] = useState(false);
  const [formUsername, setFormUsername] = useState("");
  const [formPassword, setFormPassword] = useState("");
  const [formAutostart, setFormAutostart] = useState(false);
  const [formUpstream, setFormUpstream] = useState<SocksUpstreamType>("warp");
  const [formCountry, setFormCountry] = useState("JP");
  const [formCustomAddress, setFormCustomAddress] = useState("127.0.0.1:1080");
  const [formProfileId, setFormProfileId] = useState("default");
  const [aetherProfiles, setAetherProfiles] = useState<AetherProfileConfig[]>([]);

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
    } catch (e) {
      console.error("Failed to load instances:", e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
    const timer = setInterval(() => void refresh(), 4000);
    return () => clearInterval(timer);
  }, [refresh]);

  const openCreateModal = () => {
    // Find next available port
    const usedPorts = new Set(instances.map((i) => i.listenPort));
    let nextPort = 1081;
    while (usedPorts.has(nextPort)) {
      nextPort++;
    }

    setEditingId(null);
    setFormName(`SOCKS5 出口 :${nextPort}`);
    setFormPort(nextPort);
    setFormHost("0.0.0.0");
    setFormEnableAuth(false);
    setFormUsername("");
    setFormPassword("");
    setFormAutostart(true);
    setFormUpstream("warp");
    setFormCountry("JP");
    setFormCustomAddress("127.0.0.1:1080");
    setFormProfileId(aetherProfiles[0]?.id || "default");
    setModalOpen(true);
  };

  const openEditModal = (inst: SocksInstanceView) => {
    setEditingId(inst.id);
    setFormName(inst.name);
    setFormPort(inst.listenPort);
    setFormHost(inst.listenHost || "0.0.0.0");
    const hasAuth = Boolean(inst.username && inst.password);
    setFormEnableAuth(hasAuth);
    setFormUsername(inst.username || "");
    setFormPassword(inst.password || "");
    setFormAutostart(inst.autostart);
    setFormUpstream(inst.upstreamType);
    setFormCountry(inst.upstreamConfig?.country || inst.upstreamConfig?.region || "JP");
    setFormCustomAddress(inst.upstreamConfig?.address || "127.0.0.1:1080");
    setFormProfileId(inst.upstreamConfig?.profileId || aetherProfiles[0]?.id || "default");
    setModalOpen(true);
  };

  const handleSaveModal = async () => {
    if (!formName.trim()) {
      onToast("表单错误", "请输入实例名称", true);
      return;
    }
    if (formPort <= 0 || formPort > 65535) {
      onToast("表单错误", "端口范围必须在 1 - 65535 之间", true);
      return;
    }
    if (formEnableAuth && (!formUsername.trim() || !formPassword.trim())) {
      onToast("表单错误", "启用鉴权时必须填写用户名和密码", true);
      return;
    }

    let upstreamConfig: Record<string, any> = {};
    if (formUpstream === "warp") {
      upstreamConfig = { profileId: formProfileId || "default" };
    } else if (formUpstream === "proton" || formUpstream === "windscribe") {
      upstreamConfig = { country: formCountry.trim().toUpperCase() };
    } else if (formUpstream === "psiphon") {
      upstreamConfig = { region: formCountry.trim().toUpperCase() };
    } else if (formUpstream === "custom") {
      upstreamConfig = { address: formCustomAddress.trim() };
    }

    const payload: SocksInstanceConfig = {
      id: editingId || `socks-${formPort}-${Date.now().toString(36)}`,
      name: formName.trim(),
      listenHost: formHost.trim() || "0.0.0.0",
      listenPort: formPort,
      username: formEnableAuth ? formUsername.trim() : null,
      password: formEnableAuth ? formPassword.trim() : null,
      autostart: formAutostart,
      upstreamType: formUpstream,
      upstreamConfig,
    };

    try {
      if (editingId) {
        await updateSocksInstance(payload);
        onToast("修改成功", `实例 [${payload.name}] 配置已更新`);
      } else {
        await createSocksInstance(payload);
        onToast("创建成功", `新增 SOCKS5 端口 :${payload.listenPort}`);
      }
      setModalOpen(false);
      await refresh();
    } catch (e) {
      onToast("保存失败", e instanceof Error ? e.message : String(e), true);
    }
  };

  const handleDelete = async (id: string, name: string) => {
    if (!window.confirm(`确定要删除 SOCKS5 端口实例 [${name}] 吗？`)) return;
    try {
      await deleteSocksInstance(id);
      onToast("已删除", `SOCKS5 实例 [${name}] 已移除`);
      await refresh();
    } catch (e) {
      onToast("删除失败", e instanceof Error ? e.message : String(e), true);
    }
  };

  const handleToggleRunning = async (id: string, isRunning: boolean) => {
    setActionLoading((prev) => ({ ...prev, [id]: true }));
    try {
      if (isRunning) {
        await stopSocksInstance(id);
        onToast("端口已关闭", `已停止实例 [${id}]`);
      } else {
        await startSocksInstance(id);
        onToast("端口已启动", `实例 [${id}] 正在监听`);
      }
      await refresh();
    } catch (e) {
      onToast("操作失败", e instanceof Error ? e.message : String(e), true);
    } finally {
      setActionLoading((prev) => ({ ...prev, [id]: false }));
    }
  };

  const handleTestConn = async (id: string) => {
    setTestingConn((prev) => ({ ...prev, [id]: true }));
    try {
      const res = await testSocksConnectivity(id);
      if (res.success) {
        onToast("真连通成功", `出口IP: ${res.ip} | 国家: ${res.country} | 延迟: ${res.latencyMs}ms`);
      } else {
        onToast("真连通失败", res.error ?? "连接超时或拒绝", true);
      }
      await refresh();
    } catch (e) {
      onToast("测试出错", e instanceof Error ? e.message : String(e), true);
    } finally {
      setTestingConn((prev) => ({ ...prev, [id]: false }));
    }
  };

  const handleToggleAutostart = async (id: string, current: boolean) => {
    try {
      await setSocksAutostart(id, !current);
      onToast("自启设置更新", `开机自动启动已${!current ? "开启" : "关闭"}`);
      await refresh();
    } catch (e) {
      onToast("设置失败", e instanceof Error ? e.message : String(e), true);
    }
  };

  const handleStartAll = async () => {
    onToast("正在批量启动", "启动所有配置为自启动的端口...");
    for (const inst of instances) {
      if (inst.autostart && !inst.status.isRunning) {
        try {
          await startSocksInstance(inst.id);
        } catch (e) {
          console.error("Failed to start", inst.id, e);
        }
      }
    }
    await refresh();
    onToast("批量启动完毕", "已尝试拉起全部自启动服务");
  };

  const handleStopAll = async () => {
    onToast("正在批量停止", "停止所有运行中的端口...");
    for (const inst of instances) {
      if (inst.status.isRunning) {
        try {
          await stopSocksInstance(inst.id);
        } catch (e) {
          console.error("Failed to stop", inst.id, e);
        }
      }
    }
    await refresh();
    onToast("全部已停止", "所有 SOCKS5 端口监听已释放");
  };

  return (
    <div className="flex h-full flex-col overflow-y-auto bg-background p-6">
      {/* ── Page Header ─────────────────────────────────────────────────── */}
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between border-b border-border/50 pb-5">
        <div>
          <h2 className="text-xl font-bold tracking-tight text-foreground flex items-center gap-2">
            <Server className="size-5 text-primary" />
            SOCKS5 实例管理中心
          </h2>
          <p className="text-xs text-muted-foreground mt-0.5">
            配置与监管 VPS 上监听的多个 SOCKS5 端口，支持独立鉴权、出口通道绑定、真连通与开机自启动管理
          </p>
        </div>

        <div className="flex items-center gap-2">
          {onViewDashboard && (
            <Button variant="ghost" size="sm" onClick={onViewDashboard} className="text-xs">
              运行概览
            </Button>
          )}
          <Button variant="outline" size="sm" onClick={handleStartAll} className="gap-1.5 text-xs">
            <Play className="size-3.5 fill-current text-emerald-500" />
            启动全部自启项
          </Button>
          <Button variant="outline" size="sm" onClick={handleStopAll} className="gap-1.5 text-xs">
            <Square className="size-3.5 fill-current text-destructive" />
            停止全部
          </Button>
          <Button size="sm" onClick={openCreateModal} className="gap-1.5 text-xs">
            <Plus className="size-3.5" />
            添加 SOCKS5 端口
          </Button>
        </div>
      </div>

      {/* ── Instances Table / Cards ──────────────────────────────────────── */}
      <div className="mt-6">
        {loading && instances.length === 0 ? (
          <div className="flex flex-col items-center justify-center rounded-xl border border-border/80 p-12 text-center text-muted-foreground">
            <RefreshCw className="size-6 animate-spin mb-2 text-primary" />
            <span className="text-xs">加载 SOCKS5 实例列表...</span>
          </div>
        ) : instances.length === 0 ? (
          <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-border/80 p-12 text-center">
            <Server className="size-10 text-muted-foreground/40 mb-3" />
            <h3 className="text-base font-semibold text-foreground">暂未配置任何 SOCKS5 端口</h3>
            <p className="text-xs text-muted-foreground mt-1 max-w-sm">
              点击下方按钮添加首个 SOCKS5 端口，支持绑定 Warp、Proton、Windscribe 等多地区节点
            </p>
            <Button onClick={openCreateModal} className="mt-4 gap-1.5 text-xs">
              <Plus className="size-3.5" />
              创建 SOCKS5 端口
            </Button>
          </div>
        ) : (
          <div className="rounded-xl border border-border/70 bg-card shadow-sm overflow-hidden">
            <div className="overflow-x-auto">
              <table className="w-full text-left text-xs">
                <thead className="border-b border-border/60 bg-muted/40 text-[11px] uppercase tracking-wider text-muted-foreground">
                  <tr>
                    <th className="px-4 py-3 font-semibold">状态</th>
                    <th className="px-4 py-3 font-semibold">实例名称</th>
                    <th className="px-4 py-3 font-semibold">监听端口</th>
                    <th className="px-4 py-3 font-semibold">上游出口</th>
                    <th className="px-4 py-3 font-semibold">鉴权模式</th>
                    <th className="px-4 py-3 font-semibold">开机自启</th>
                    <th className="px-4 py-3 font-semibold">真实公网出口 / RTT</th>
                    <th className="px-4 py-3 font-semibold text-end">操作</th>
                  </tr>
                </thead>
                <tbody className="divide-y divide-border/40">
                  {instances.map((inst) => {
                    const isRunning = inst.status.isRunning;
                    const hasAuth = Boolean(inst.username && inst.password);

                    return (
                      <tr key={inst.id} className="hover:bg-muted/20 transition-colors">
                        {/* Status */}
                        <td className="px-4 py-3.5 whitespace-nowrap">
                          <Badge variant={isRunning ? "ok" : "outline"} className="gap-1.5 text-[10.5px]">
                            <span
                              className={[
                                "size-1.5 rounded-full",
                                isRunning ? "bg-emerald-500 animate-pulse" : "bg-muted-foreground",
                              ].join(" ")}
                            />
                            {isRunning ? "监听中" : "已停止"}
                          </Badge>
                        </td>

                        {/* Name */}
                        <td className="px-4 py-3.5 whitespace-nowrap font-medium text-foreground">
                          {inst.name}
                        </td>

                        {/* Port */}
                        <td className="px-4 py-3.5 whitespace-nowrap font-mono text-foreground font-semibold">
                          <span className="rounded bg-muted/70 px-2 py-0.5 border border-border/50">
                            :{inst.listenPort}
                          </span>
                        </td>

                        {/* Upstream */}
                        <td className="px-4 py-3.5 whitespace-nowrap">
                          <span className="inline-flex items-center gap-1 rounded bg-accent/60 px-2 py-0.5 text-[11px] font-medium uppercase text-muted-foreground">
                            {inst.upstreamType}
                            {inst.upstreamType === "warp"
                              ? ` (${(() => {
                                  const p = aetherProfiles.find((x) => x.id === inst.upstreamConfig?.profileId);
                                  return p ? p.name : inst.upstreamConfig?.profileId || "默认方案";
                                })()})`
                              : inst.upstreamConfig?.country
                              ? ` (${inst.upstreamConfig.country})`
                              : ""}
                          </span>
                        </td>

                        {/* Auth */}
                        <td className="px-4 py-3.5 whitespace-nowrap">
                          {hasAuth ? (
                            <span className="inline-flex items-center gap-1 text-amber-500 font-medium">
                              <Lock className="size-3" />
                              {inst.username}
                            </span>
                          ) : (
                            <span className="inline-flex items-center gap-1 text-muted-foreground">
                              <Unlock className="size-3" /> 免密
                            </span>
                          )}
                        </td>

                        {/* Autostart */}
                        <td className="px-4 py-3.5 whitespace-nowrap">
                          <div className="flex items-center gap-2">
                            <Switch
                              checked={inst.autostart}
                              onCheckedChange={() => handleToggleAutostart(inst.id, inst.autostart)}
                            />
                            <span className="text-[11px] text-muted-foreground">
                              {inst.autostart ? "自启" : "手动"}
                            </span>
                          </div>
                        </td>

                        {/* Real Exit & Latency */}
                        <td className="px-4 py-3.5 whitespace-nowrap">
                          {inst.status.lastConnectivity?.success ? (
                            <div className="flex items-center gap-2 font-mono">
                              <span className="text-foreground font-bold">{inst.status.lastConnectivity.ip}</span>
                              <span className="rounded bg-muted/60 px-1.5 py-0.2 text-[10px] text-muted-foreground font-semibold">
                                {inst.status.lastConnectivity.country}
                              </span>
                              <span className="text-emerald-500 font-semibold">
                                {inst.status.lastConnectivity.latencyMs}ms
                              </span>
                            </div>
                          ) : (
                            <span className="text-muted-foreground/60 italic text-[11px]">未探测</span>
                          )}
                        </td>

                        {/* Actions */}
                        <td className="px-4 py-3.5 whitespace-nowrap text-end space-x-1.5">
                          {/* Toggle Start/Stop */}
                          <Button
                            size="sm"
                            variant={isRunning ? "outline" : "default"}
                            className={isRunning ? "h-7 text-xs border-destructive/30 text-destructive hover:bg-destructive/10" : "h-7 text-xs"}
                            disabled={actionLoading[inst.id]}
                            onClick={() => handleToggleRunning(inst.id, isRunning)}
                          >
                            {isRunning ? "停止" : "启动"}
                          </Button>

                          {/* Test connectivity */}
                          <Button
                            size="sm"
                            variant="ghost"
                            className="h-7 text-xs gap-1 text-emerald-500 hover:text-emerald-400 hover:bg-emerald-500/10"
                            disabled={!isRunning || testingConn[inst.id]}
                            onClick={() => handleTestConn(inst.id)}
                          >
                            <RefreshCw className={["size-3", testingConn[inst.id] ? "animate-spin" : ""].join(" ")} />
                            测通
                          </Button>

                          {/* Edit */}
                          <Button
                            size="sm"
                            variant="ghost"
                            className="h-7 px-2 text-muted-foreground hover:text-foreground"
                            onClick={() => openEditModal(inst)}
                          >
                            <Edit2 className="size-3.5" />
                          </Button>

                          {/* Delete */}
                          <Button
                            size="sm"
                            variant="ghost"
                            className="h-7 px-2 text-muted-foreground hover:text-destructive"
                            onClick={() => handleDelete(inst.id, inst.name)}
                          >
                            <Trash2 className="size-3.5" />
                          </Button>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>
          </div>
        )}
      </div>

      {/* ── Add / Edit Modal Dialog ──────────────────────────────────────── */}
      {modalOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4 backdrop-blur-sm">
          <div className="w-full max-w-lg rounded-xl border border-border bg-card p-6 shadow-2xl animate-in fade-in zoom-in-95 duration-150">
            <h3 className="text-base font-bold text-foreground">
              {editingId ? "编辑 SOCKS5 端口配置" : "添加新 SOCKS5 端口实例"}
            </h3>
            <p className="text-xs text-muted-foreground mt-0.5">
              配置 VPS 监听端口、认证账号与上游多地区出口绑定
            </p>

            <div className="mt-5 space-y-4 text-xs">
              {/* Name */}
              <div>
                <label className="font-semibold text-foreground mb-1 block">实例名称</label>
                <Input
                  value={formName}
                  onChange={(e) => setFormName(e.target.value)}
                  placeholder="例如: Proton 日本节点 / Warp 欧洲出口"
                />
              </div>

              {/* Listen Port & Host */}
              <div className="grid grid-cols-2 gap-3">
                <div>
                  <label className="font-semibold text-foreground mb-1 block">监听端口 (Port)</label>
                  <Input
                    type="number"
                    value={formPort}
                    onChange={(e) => setFormPort(parseInt(e.target.value) || 0)}
                    placeholder="1081"
                  />
                </div>
                <div>
                  <label className="font-semibold text-foreground mb-1 block">绑定地址 (Host)</label>
                  <Input
                    value={formHost}
                    onChange={(e) => setFormHost(e.target.value)}
                    placeholder="0.0.0.0 (对外开放)"
                  />
                </div>
              </div>

              {/* Upstream Carrier Selector */}
              <div>
                <label className="font-semibold text-foreground mb-1 block">绑定上游程序 / 出口类型</label>
                <div className="grid grid-cols-3 gap-2">
                  {(["warp", "proton", "windscribe", "psiphon", "tor", "custom"] as SocksUpstreamType[]).map((u) => (
                    <button
                      key={u}
                      type="button"
                      onClick={() => setFormUpstream(u)}
                      className={[
                        "rounded-lg border p-2 text-center capitalize transition-all font-semibold",
                        formUpstream === u
                          ? "border-primary bg-primary/10 text-primary ring-1 ring-primary/40"
                          : "border-border/60 bg-muted/30 text-muted-foreground hover:bg-muted",
                      ].join(" ")}
                    >
                      {u}
                    </button>
                  ))}
                </div>
              </div>

              {/* Upstream Config params */}
              {formUpstream === "warp" && (
                <div className="space-y-2">
                  <div className="flex items-center justify-between">
                    <label className="font-semibold text-foreground text-xs block">
                      选择 Aether (WARP) 方案 (Profile)
                    </label>
                    <span className="text-[11px] text-muted-foreground">
                      可在“底层设置”中调整方案参数
                    </span>
                  </div>
                  <select
                    value={formProfileId}
                    onChange={(e) => setFormProfileId(e.target.value)}
                    className="flex h-9 w-full rounded-md border border-input bg-background px-3 py-1 text-xs font-medium shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring"
                  >
                    {aetherProfiles.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name} ({p.protocol.toUpperCase()} · {p.noize === "off" ? "无混流" : `混流: ${p.noize}`} · {p.fragmentClientHello ? "分片开启" : "分片关闭"})
                      </option>
                    ))}
                    {aetherProfiles.length === 0 && (
                      <option value="default">默认直连方案 (Default)</option>
                    )}
                  </select>
                  {(() => {
                    const sel = aetherProfiles.find((p) => p.id === formProfileId);
                    if (!sel) return null;
                    return (
                      <div className="rounded-md bg-muted/40 border border-border/50 p-2 text-[11px] text-muted-foreground flex flex-wrap items-center gap-2">
                        <span className="font-semibold text-foreground">{sel.name}</span>
                        <span>·</span>
                        <span>协议: {sel.protocol.toUpperCase()} ({sel.masqueTransport})</span>
                        <span>·</span>
                        <span>混流: {sel.noize}</span>
                        <span>·</span>
                        <span>分片: {sel.fragmentClientHello ? `开启 (${sel.fragmentSize})` : "关闭"}</span>
                        <span>·</span>
                        <span>优选: {sel.scanMode}</span>
                      </div>
                    );
                  })()}
                </div>
              )}

              {(formUpstream === "proton" || formUpstream === "windscribe" || formUpstream === "psiphon") && (
                <div>
                  <label className="font-semibold text-foreground mb-1 block">目标出口国家 / 地区代码</label>
                  <Input
                    value={formCountry}
                    onChange={(e) => setFormCountry(e.target.value.toUpperCase())}
                    placeholder="例如: JP, US, NL, HK, SG"
                  />
                  <p className="text-[11px] text-muted-foreground mt-1">
                    两字母国家代码，例如 JP (日本), US (美国), HK (香港), NL (荷兰)
                  </p>
                </div>
              )}

              {formUpstream === "custom" && (
                <div>
                  <label className="font-semibold text-foreground mb-1 block">自定义 SOCKS5 转发目标</label>
                  <Input
                    value={formCustomAddress}
                    onChange={(e) => setFormCustomAddress(e.target.value)}
                    placeholder="127.0.0.1:1080"
                  />
                </div>
              )}

              {/* Authentication Toggle */}
              <div className="rounded-lg border border-border/70 bg-muted/20 p-3">
                <div className="flex items-center justify-between">
                  <div>
                    <span className="font-semibold text-foreground block">启用 SOCKS5 账号鉴权</span>
                    <span className="text-[11px] text-muted-foreground">客户端连接此端口时必须提供用户名与密码</span>
                  </div>
                  <Switch checked={formEnableAuth} onCheckedChange={setFormEnableAuth} />
                </div>

                {formEnableAuth && (
                  <div className="mt-3 grid grid-cols-2 gap-3 pt-3 border-t border-border/40">
                    <div>
                      <label className="font-semibold text-foreground mb-1 block">用户名</label>
                      <Input
                        value={formUsername}
                        onChange={(e) => setFormUsername(e.target.value)}
                        placeholder="socks_user"
                      />
                    </div>
                    <div>
                      <label className="font-semibold text-foreground mb-1 block">密码</label>
                      <Input
                        type="text"
                        value={formPassword}
                        onChange={(e) => setFormPassword(e.target.value)}
                        placeholder="socks_password"
                      />
                    </div>
                  </div>
                )}
              </div>

              {/* Autostart Toggle */}
              <div className="flex items-center justify-between rounded-lg border border-border/70 bg-muted/20 p-3">
                <div>
                  <span className="font-semibold text-foreground block">开机 / 服务启动时自动运行</span>
                  <span className="text-[11px] text-muted-foreground">当 VPS 后台服务拉起时自动监听此 SOCKS5 端口</span>
                </div>
                <Switch checked={formAutostart} onCheckedChange={setFormAutostart} />
              </div>
            </div>

            {/* Modal Actions */}
            <div className="mt-6 flex items-center justify-end gap-2.5 pt-3 border-t border-border/50">
              <Button variant="outline" size="sm" onClick={() => setModalOpen(false)}>
                取消
              </Button>
              <Button size="sm" onClick={handleSaveModal}>
                {editingId ? "保存更改" : "确认创建"}
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
