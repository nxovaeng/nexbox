import { useState, useEffect } from "react";
import {
  Layers,
  CheckCircle2,
  Copy,
  Trash2,
  Edit2,
  Plus,
  RotateCw,
  Settings2,
} from "lucide-react";
import { useT } from "@/core/useT";
import {
  activeProfileId,
  createProfile,
  deleteProfile,
  duplicateProfile,
  listProfiles,
  renameProfile,
  switchProfile,
  type ProfileSummary,
} from "@/core/api";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";

// ── Profile Management (Full Card Panel) ──────────────────────────────────────

export interface ProfileManagementProps {
  onSelectProfile?: (id: string) => void;
  disabled?: boolean;
  onToast?: (title: string, message: string, error?: boolean) => void;
}

export function ProfileManagement({
  onSelectProfile,
  disabled,
  onToast,
}: ProfileManagementProps) {
  const t = useT();
  const [profiles, setProfiles] = useState<ProfileSummary[]>([]);
  const [activeId, setActiveId] = useState<string>("default");
  const [loading, setLoading] = useState(false);

  // Creation state
  const [isCreating, setIsCreating] = useState(false);
  const [newProfileName, setNewProfileName] = useState("");

  // Rename modal / inline state
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");

  const load = async () => {
    setLoading(true);
    try {
      const [all, active] = await Promise.all([listProfiles(), activeProfileId()]);
      setProfiles(all);
      setActiveId(active.id);
    } catch (e) {
      console.error("Failed to load profiles", e);
      onToast?.(t("Error"), t("Failed to load profiles"), true);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void load();
    const onSwitched = () => void load();
    window.addEventListener("ProfileSwitched", onSwitched);
    return () => window.removeEventListener("ProfileSwitched", onSwitched);
  }, []);

  const handleSwitch = async (id: string) => {
    if (disabled || id === activeId) return;
    try {
      await switchProfile(id);
      setActiveId(id);
      onToast?.(t("Profile Switched"), `${t("Active profile:")} ${id}`);
    } catch (e) {
      onToast?.(
        t("Error"),
        e instanceof Error ? e.message : t("Failed to switch profile"),
        true
      );
    }
  };

  const handleCreate = async () => {
    const name = newProfileName.trim();
    if (!name) return;
    try {
      const created = await createProfile(name);
      await switchProfile(created.id);
      setNewProfileName("");
      setIsCreating(false);
      await load();
      onToast?.(t("Created"), `${t("Profile created:")} ${created.name}`);
    } catch (e) {
      onToast?.(
        t("Error"),
        e instanceof Error ? e.message : t("Failed to create profile"),
        true
      );
    }
  };

  const handleDuplicate = async (profile: ProfileSummary) => {
    const defaultCopyName = `${profile.name} (Copy)`;
    const name = window.prompt(t("Enter name for duplicated profile:"), defaultCopyName);
    if (!name || !name.trim()) return;
    try {
      const copy = await duplicateProfile(profile.id, name.trim());
      await load();
      onToast?.(t("Duplicated"), `${t("Created copy:")} ${copy.name}`);
    } catch (e) {
      onToast?.(
        t("Error"),
        e instanceof Error ? e.message : t("Failed to duplicate profile"),
        true
      );
    }
  };

  const startRename = (profile: ProfileSummary) => {
    setEditingId(profile.id);
    setEditingName(profile.name);
  };

  const saveRename = async (id: string) => {
    const name = editingName.trim();
    if (!name) return;
    try {
      await renameProfile(id, name);
      setEditingId(null);
      await load();
      onToast?.(t("Renamed"), `${t("Profile renamed to:")} ${name}`);
    } catch (e) {
      onToast?.(
        t("Error"),
        e instanceof Error ? e.message : t("Failed to rename profile"),
        true
      );
    }
  };

  const handleDelete = async (profile: ProfileSummary) => {
    if (profile.id === "default") {
      alert(t("The default profile cannot be deleted."));
      return;
    }
    if (
      !window.confirm(
        `${t("Are you sure you want to delete profile")} "${profile.name}"?`
      )
    ) {
      return;
    }
    try {
      await deleteProfile(profile.id);
      await load();
      onToast?.(t("Deleted"), `${t("Deleted profile:")} ${profile.name}`);
    } catch (e) {
      onToast?.(
        t("Error"),
        e instanceof Error ? e.message : t("Failed to delete profile"),
        true
      );
    }
  };

  return (
    <Card className="w-full">
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between flex-wrap gap-2">
          <div>
            <CardTitle className="text-base flex items-center gap-2">
              <Layers className="size-4 text-primary" />
              {t("Profile Configurations")}
            </CardTitle>
            <CardDescription className="mt-1">
              {t(
                "Manage multiple connection profiles and routing schemes. Easily switch, duplicate, and customize parameters for different networks."
              )}
            </CardDescription>
          </div>

          <div className="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => void load()}
              disabled={loading}
              className="h-8 gap-1.5 text-xs"
            >
              <RotateCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} />
              {t("Refresh")}
            </Button>

            {!isCreating && (
              <Button
                size="sm"
                onClick={() => setIsCreating(true)}
                disabled={disabled}
                className="h-8 gap-1.5 text-xs font-medium"
              >
                <Plus className="size-3.5" />
                {t("New Profile")}
              </Button>
            )}
          </div>
        </div>
      </CardHeader>

      <CardContent className="flex flex-col gap-4">
        {/* Creation inline form */}
        {isCreating && (
          <div className="flex items-center gap-2 rounded-lg border border-primary/40 bg-primary/[0.04] p-3 transition-all">
            <Layers className="size-4 text-primary shrink-0" />
            <Input
              placeholder={t("Profile name (e.g. Mobile Hotspot, Office Gateway)…")}
              value={newProfileName}
              onChange={(e) => setNewProfileName(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") void handleCreate();
                if (e.key === "Escape") setIsCreating(false);
              }}
              autoFocus
              className="h-8 text-sm"
            />
            <Button size="sm" onClick={() => void handleCreate()} className="h-8 text-xs shrink-0">
              {t("Create")}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => setIsCreating(false)}
              className="h-8 text-xs shrink-0"
            >
              {t("Cancel")}
            </Button>
          </div>
        )}

        {/* Profile Card List */}
        <div className="grid grid-cols-1 gap-3.5">
          {profiles.map((p) => {
            const isActive = p.id === activeId;
            const isEditing = editingId === p.id;

            const protocolLabel =
              p.protocol === "masque"
                ? `MASQUE (${p.masqueTransport?.toUpperCase() ?? "AUTO"})`
                : p.protocol === "wg"
                ? "WireGuard"
                : p.protocol === "awg"
                ? "AmneziaWG"
                : (p.protocol ?? "MASQUE");

            const endpointLabel =
              p.endpointMode === "custom-only"
                ? `${t("Custom only")}: ${p.peer ?? "none"}`
                : p.endpointMode === "custom-first"
                ? `${t("Custom first")}: ${p.peer ?? "none"}`
                : t("Auto gateway sweep");

            const dnsLabel = p.dns?.length
              ? p.dns.join(", ")
              : "1.1.1.1, 1.0.0.1";

            const socksLabel = p.socksAddress ?? "127.0.0.1:1080";

            return (
              <div
                key={p.id}
                className={`relative flex flex-col gap-3 rounded-lg border p-4 transition-all ${
                  isActive
                    ? "border-primary/60 bg-primary/[0.03] shadow-sm ring-1 ring-primary/20"
                    : "border-border bg-card hover:border-border/80 hover:shadow-sm"
                }`}
              >
                {/* Header Row */}
                <div className="flex items-start justify-between gap-3 flex-wrap">
                  <div className="flex items-center gap-2.5">
                    <div
                      className={`rounded-md p-2 transition-colors ${
                        isActive
                          ? "bg-primary text-primary-foreground"
                          : "bg-muted text-muted-foreground"
                      }`}
                    >
                      <Layers className="size-4" />
                    </div>

                    {isEditing ? (
                      <div className="flex items-center gap-1.5">
                        <Input
                          value={editingName}
                          onChange={(e) => setEditingName(e.target.value)}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") void saveRename(p.id);
                            if (e.key === "Escape") setEditingId(null);
                          }}
                          autoFocus
                          className="h-7 w-48 text-sm"
                        />
                        <Button
                          size="sm"
                          className="h-7 px-2 text-xs"
                          onClick={() => void saveRename(p.id)}
                        >
                          {t("Save")}
                        </Button>
                        <Button
                          size="sm"
                          variant="ghost"
                          className="h-7 px-2 text-xs"
                          onClick={() => setEditingId(null)}
                        >
                          {t("Cancel")}
                        </Button>
                      </div>
                    ) : (
                      <div className="flex items-center gap-2">
                        <span className="font-semibold text-sm tracking-tight text-foreground">
                          {p.name}
                        </span>
                        {p.id === "default" && (
                          <span className="text-[11px] font-mono text-muted-foreground">
                            (default)
                          </span>
                        )}
                        {isActive && (
                          <Badge variant="ok" className="h-5 gap-1 text-[11px]">
                            <CheckCircle2 className="size-3" />
                            {t("Active")}
                          </Badge>
                        )}
                      </div>
                    )}
                  </div>

                  {/* Actions */}
                  <div className="flex items-center gap-1.5 self-start">
                    {!isActive && (
                      <Button
                        size="sm"
                        variant="default"
                        disabled={disabled}
                        onClick={() => void handleSwitch(p.id)}
                        className="h-7 px-2.5 text-xs font-medium"
                      >
                        {t("Activate")}
                      </Button>
                    )}

                    <Button
                      size="sm"
                      variant="outline"
                      disabled={disabled}
                      onClick={() => {
                        if (!isActive) void handleSwitch(p.id);
                        onSelectProfile?.(p.id);
                      }}
                      className="h-7 px-2 text-xs"
                      title={t("Configure and edit routes")}
                    >
                      <Settings2 className="size-3.5 mr-1" />
                      {t("Edit Routes")}
                    </Button>

                    <Button
                      size="sm"
                      variant="ghost"
                      disabled={disabled}
                      onClick={() => startRename(p)}
                      className="h-7 w-7 p-0 text-muted-foreground hover:text-foreground"
                      title={t("Rename")}
                    >
                      <Edit2 className="size-3.5" />
                    </Button>

                    <Button
                      size="sm"
                      variant="ghost"
                      disabled={disabled}
                      onClick={() => void handleDuplicate(p)}
                      className="h-7 w-7 p-0 text-muted-foreground hover:text-foreground"
                      title={t("Duplicate Scheme")}
                    >
                      <Copy className="size-3.5" />
                    </Button>

                    {p.id !== "default" && (
                      <Button
                        size="sm"
                        variant="ghost"
                        disabled={disabled}
                        onClick={() => void handleDelete(p)}
                        className="h-7 w-7 p-0 text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
                        title={t("Delete Profile")}
                      >
                        <Trash2 className="size-3.5" />
                      </Button>
                    )}
                  </div>
                </div>

                {/* Summary Metadata Grid */}
                <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-2 pt-1">
                  <div className="flex flex-col gap-0.5 rounded-md bg-muted/40 p-2 text-xs">
                    <span className="text-[11px] font-medium text-muted-foreground uppercase tracking-wider">
                      {t("Protocol")}
                    </span>
                    <span className="font-semibold text-foreground/90">
                      {protocolLabel}
                    </span>
                  </div>

                  <div className="flex flex-col gap-0.5 rounded-md bg-muted/40 p-2 text-xs">
                    <span className="text-[11px] font-medium text-muted-foreground uppercase tracking-wider">
                      {t("Endpoint Mode")}
                    </span>
                    <span className="font-medium text-foreground/90 truncate" title={endpointLabel}>
                      {endpointLabel}
                    </span>
                  </div>

                  <div className="flex flex-col gap-0.5 rounded-md bg-muted/40 p-2 text-xs">
                    <span className="text-[11px] font-medium text-muted-foreground uppercase tracking-wider">
                      {t("DNS Servers")}
                    </span>
                    <span className="font-mono text-[11.5px] text-foreground/90 truncate" title={dnsLabel}>
                      {dnsLabel}
                    </span>
                  </div>

                  <div className="flex flex-col gap-0.5 rounded-md bg-muted/40 p-2 text-xs">
                    <span className="text-[11px] font-medium text-muted-foreground uppercase tracking-wider">
                      {t("SOCKS Listener")}
                    </span>
                    <span className="font-mono text-[11.5px] text-foreground/90 truncate">
                      {socksLabel}
                    </span>
                  </div>
                </div>

                {p.upstreamProxy && (
                  <div className="text-[11.5px] text-muted-foreground font-mono bg-muted/20 px-2 py-1 rounded">
                    🔗 {t("Upstream Proxy")}: {p.upstreamProxy}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      </CardContent>
    </Card>
  );
}

// ── Profile Selector Dropdown (Quick Bar Widget) ──────────────────────────────

export function ProfileSelector({
  disabled,
  onManageClick,
}: {
  disabled?: boolean;
  onManageClick?: () => void;
}) {
  const t = useT();
  const [profiles, setProfiles] = useState<ProfileSummary[]>([]);
  const [activeId, setActiveId] = useState<string>("default");

  const loadProfiles = async () => {
    try {
      const [profilesData, activeData] = await Promise.all([
        listProfiles(),
        activeProfileId(),
      ]);
      setProfiles(profilesData);
      setActiveId(activeData.id);
    } catch (e) {
      console.error("Failed to load profiles", e);
    }
  };

  useEffect(() => {
    void loadProfiles();
    const onSwitched = () => void loadProfiles();
    window.addEventListener("ProfileSwitched", onSwitched);
    return () => window.removeEventListener("ProfileSwitched", onSwitched);
  }, []);

  const handleSwitch = async (id: string) => {
    if (id === activeId) return;
    if (id === "manage") {
      onManageClick?.();
      return;
    }
    try {
      await switchProfile(id);
      setActiveId(id);
    } catch (e) {
      console.error("Failed to switch profile", e);
    }
  };

  return (
    <div className="flex items-center gap-1.5">
      <select
        className="flex h-8 w-[160px] rounded-md border border-input bg-background px-2.5 py-1 text-xs font-medium shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
        value={activeId}
        disabled={disabled}
        onChange={(e) => void handleSwitch(e.target.value)}
      >
        {profiles.map((p) => (
          <option key={p.id} value={p.id}>
            {p.name}
          </option>
        ))}
        <option value="manage" className="font-semibold text-primary">
          ⚙️ {t("Manage Profiles…")}
        </option>
      </select>
    </div>
  );
}
