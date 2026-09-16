import { useState, useMemo } from "react";
import { useT } from "@/core/useT";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import { REPORT_EVENT_LIMIT, buildReport, reportFilename } from "@/core/report";
import { saveReport } from "@/core/api";
import { Row, Seg, TextField } from "./panels";
import type { ConnectionProfile, CoreLogEvent, CoreProbe, CoreSnapshot } from "@/types";

export interface DiagnosticsProps {
  profile: ConnectionProfile;
  onChange: (profile: ConnectionProfile) => void;
  snapshot: CoreSnapshot;
  probe: CoreProbe;
  logs: CoreLogEvent[];
  runtime: string;
  appVersion: string;
  onToast: (title: string, message: string, error?: boolean) => void;
}

export function DiagnosticsView({
  snapshot,
  profile,
  onChange,
  probe,
  logs,
  runtime,
  appVersion,
  onToast,
}: DiagnosticsProps) {
  const t = useT();
  const set = (patch: Partial<ConnectionProfile>) => onChange({ ...profile, ...patch });
  const [includeSystem, setIncludeSystem] = useState(true);
  const [includeSettings, setIncludeSettings] = useState(true);
  const [includeEvents, setIncludeEvents] = useState(true);
  const [redact, setRedact] = useState(true);

  const report = useMemo(
    () =>
      buildReport({
        appVersion,
        engineVersion: probe.version,
        system: runtime,
        snapshot,
        profile,
        logs,
        options: { includeSystem, includeSettings, includeEvents, redact },
      }),
    [appVersion, probe.version, runtime, snapshot, profile, logs, includeSystem, includeSettings, includeEvents, redact],
  );

  return (
    <>
      <Card>
        <CardHeader className="pb-1"><CardTitle className="text-[15px]">{t("Core and profile")}</CardTitle></CardHeader>
        <CardContent className="flex flex-col gap-4 pt-2">
          <div className="grid grid-cols-2 gap-4">
            <TextField
              label="Profile name" value={profile.name}
              onChange={(name) => set({ name })}
              help="Shown in reports so you can tell saved setups apart."
            />
            <TextField
              label="Core executable" mono value={profile.corePath ?? ""}
              placeholder="Auto-detect"
              onChange={(value) => set({ corePath: value || null })}
              help={probe.path ?? probe.message}
            />
          </div>
          <Separator />
          <Row first title="Log detail" help="Connection state is read from info-level output, so info is the floor.">
            <Seg
              value={profile.logLevel}
              onChange={(logLevel) => set({ logLevel })}
              options={[["error", "error"], ["warn", "warn"], ["info", "info"], ["debug", "debug"], ["trace", "trace"]]}
            />
          </Row>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="pb-1">
          <CardTitle className="text-[15px]">{t("Report")}</CardTitle>
          <CardDescription>{t("Raise the log detail, reproduce the problem, then build this.")}</CardDescription>
        </CardHeader>
        <CardContent className="pt-0">
          <Row first title="App and engine version" help="Always included — a report without it cannot be read.">
            <Switch checked disabled />
          </Row>
          <Row title="Operating system" help={runtime}>
            <Switch checked={includeSystem} onCheckedChange={setIncludeSystem} />
          </Row>
          <Row title="Connection settings" help="No Zero Trust credentials and no pinned address — only whether one is set.">
            <Switch checked={includeSettings} onCheckedChange={setIncludeSettings} />
          </Row>
          <Row title={`${t("Recent events (up to")} ${REPORT_EVENT_LIMIT})`} help="What the core and the supervisor did.">
            <Switch checked={includeEvents} onCheckedChange={setIncludeEvents} />
          </Row>
          <Row title="Replace IP addresses" help="Swaps them for placeholders. Most problems can still be diagnosed.">
            <Switch checked={redact} onCheckedChange={setRedact} />
          </Row>
        </CardContent>
      </Card>

      <Card>
        <CardContent className="flex flex-col gap-3 p-4">
          <pre className="max-h-[240px] overflow-auto whitespace-pre-wrap break-words rounded-md bg-muted/50 p-3 font-mono text-[11.5px] leading-relaxed">
            {report}
          </pre>
          <div className="flex justify-end gap-2">
            <Button
              variant="outline"
              onClick={async () => {
                try {
                  await navigator.clipboard.writeText(report);
                  onToast("Copied", "The report is on the clipboard.");
                } catch (error) {
                  onToast("Copy failed", String(error), true);
                }
              }}
            >
              {t("Copy")}
            </Button>
            <Button
              onClick={async () => {
                try {
                  onToast("Report saved", await saveReport(report, reportFilename()));
                } catch (error) {
                  onToast("Save failed", error instanceof Error ? error.message : String(error), true);
                }
              }}
            >
              {t("Save report")}
            </Button>
          </div>
        </CardContent>
      </Card>
    </>
  );
}
