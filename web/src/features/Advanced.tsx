import { useEffect, useState } from "react";
import { useT } from "@/core/useT";
import {
  Layers,
  ShieldCheck,
  Globe,
  Radio,
  Wifi,
  Zap,
  Terminal,
  FileText,
  Scale,
  type LucideIcon,
} from "lucide-react";
import { AetherManager } from "./AetherManager";
import { ProtonManager } from "./ProtonManager";
import { WindscribeManager } from "./WindscribeManager";
import { PsiphonManager } from "./PsiphonManager";
import { TrafficSettings } from "./TrafficSettings";
import { IdentitySettings } from "./IdentitySettings";
import { CoreManager } from "./CoreManager";
import { LiveLogs } from "./LiveLogs";
import { DiagnosticsView } from "./DiagnosticsView";
import { LicencesView } from "./LicencesView";
import type { SectionId } from "./settingsIndex";
import type { ConnectionProfile, CoreLogEvent, CoreProbe, CoreSnapshot } from "@/types";

const SECTIONS: Array<{ group: string; items: Array<{ id: SectionId; label: string; icon: LucideIcon }> }> = [
  {
    group: "Outbounds",
    items: [
      { id: "aether", label: "Aether (WARP) 方案管理", icon: Layers },
      { id: "proton", label: "Proton 配置管理", icon: ShieldCheck },
      { id: "windscribe", label: "Windscribe 节点", icon: Globe },
      { id: "psiphon", label: "Psiphon 配置管理", icon: Radio },
    ],
  },
  {
    group: "System",
    items: [
      { id: "traffic", label: "Traffic & DNS", icon: Wifi },
      { id: "identity", label: "Cloudflare Identity", icon: ShieldCheck },
      { id: "core", label: "Core Management", icon: Zap },
    ],
  },
  {
    group: "Support",
    items: [
      { id: "logs", label: "实时运行日志", icon: Terminal },
      { id: "diagnostics", label: "Diagnostics & Logs", icon: FileText },
      { id: "licences", label: "Licences & notices", icon: Scale },
    ],
  },
];

const BLURB: Record<SectionId, string> = {
  aether: "管理 Aether (Cloudflare WARP) 连接方案、场景预设、分片与 Noize 混流参数。",
  proton: "管理 Proton 节点凭据、证书及 WireProxy 独立运行启动参数。",
  windscribe: "管理 Windscribe 账号凭据、纯净 HTTPS 代理节点及独立 SOCKS5 转发。",
  psiphon: "管理 Psiphon 引导节点列表、公钥签名及独立运行启动参数。",
  traffic: "设置系统全局代理、分流绕行与路由拦截规则、局域网共享。",
  identity: "Cloudflare Zero Trust 企业租户认证与凭据配置。",
  core: "检测并管理后台运行所需的各类底层二进制内核文件。",
  logs: "实时捕获内核守护进程、子进程控制台流与错误日志。",
  diagnostics: "导出诊断报告包、查看脱敏配置与排错信息。",
  licences: "NextVPN 遵守的开源授权协议与第三方开源声明。",
};

export interface AdvancedProps {
  profile: ConnectionProfile;
  onChange: (profile: ConnectionProfile) => void;
  snapshot: CoreSnapshot;
  probe: CoreProbe;
  logs: CoreLogEvent[];
  runtime: string;
  appVersion: string;
  onSave: () => void;
  onToast: (title: string, message: string, error?: boolean) => void;
  jumpTo?: { section: SectionId; at: number } | null;
}

export function Advanced(props: AdvancedProps) {
  const t = useT();
  const [section, setSection] = useState<SectionId>("aether");

  const { jumpTo } = props;
  useEffect(() => {
    if (jumpTo) setSection(jumpTo.section);
  }, [jumpTo]);

  const heading = SECTIONS.flatMap((group) => group.items).find((item) => item.id === section);

  return (
    <div className="grid h-full grid-cols-[196px_minmax(0,1fr)] overflow-hidden">
      <nav className="flex flex-col gap-0.5 overflow-y-auto border-r bg-card p-2" aria-label={t("Settings sections")}>
        {SECTIONS.map((group) => (
          <div key={group.group} className="flex flex-col gap-0.5">
            <span className="px-2.5 pb-1 pt-3.5 text-[10.5px] font-semibold uppercase tracking-wider text-muted-foreground">
              {t(group.group)}
            </span>
            {group.items.map(({ id, label, icon: Icon }) => (
              <button
                key={id}
                type="button"
                aria-current={section === id}
                onClick={() => setSection(id)}
                className={[
                  "flex items-center gap-2.5 rounded-md px-2.5 py-1.5 text-start text-[13.5px] transition-colors",
                  "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
                  section === id
                    ? "bg-primary/10 font-semibold text-primary"
                    : "text-muted-foreground hover:bg-accent hover:text-foreground",
                ].join(" ")}
              >
                <Icon className="size-[15px]" />
                {t(label)}
              </button>
            ))}
          </div>
        ))}
      </nav>

      <div className="flex flex-col gap-4 overflow-y-auto p-6">
        <div className="flex items-start justify-between gap-4">
          <div className="flex flex-col gap-1">
            <h2 className="text-[19px] font-semibold tracking-tight">{heading ? t(heading.label) : null}</h2>
            <p className="text-sm text-muted-foreground">{t(BLURB[section])}</p>
          </div>
        </div>

        {section === "aether" && <AetherManager onToast={props.onToast} />}
        {section === "proton" && <ProtonManager />}
        {section === "windscribe" && <WindscribeManager />}
        {section === "psiphon" && <PsiphonManager />}
        {section === "traffic" && <TrafficSettings {...props} />}
        {section === "identity" && (
          <IdentitySettings
            profile={props.profile}
            onChange={props.onChange}
            onSave={props.onSave}
          />
        )}
        {section === "core" && <CoreManager />}
        {section === "logs" && <LiveLogs logs={props.logs} />}
        {section === "diagnostics" && <DiagnosticsView {...props} />}
        {section === "licences" && <LicencesView />}
      </div>
    </div>
  );
}
