import { useT } from "@/core/useT";
import { Save } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import { Row, TextField } from "./panels";
import type { ConnectionProfile } from "@/types";

export interface IdentitySettingsProps {
  profile: ConnectionProfile;
  onChange: (profile: ConnectionProfile) => void;
  onSave: () => void;
}

export function IdentitySettings({ profile, onChange, onSave }: IdentitySettingsProps) {
  const t = useT();
  const set = (patch: Partial<ConnectionProfile>) => onChange({ ...profile, ...patch });
  return (
    <div className="flex flex-col gap-3">
      <Card>
        <CardHeader className="pb-1">
          <CardTitle className="text-[15px]">Cloudflare Zero Trust</CardTitle>
          <CardDescription>{t("Leave empty to stay on a personal WARP identity.")}</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4 pt-2">
          <div className="grid grid-cols-2 gap-4">
            <TextField
              label="Team"
              value={profile.team ?? ""}
              placeholder="team name"
              onChange={(value) => set({ team: value || null })}
            />
            <TextField
              label="Email"
              value={profile.accessEmail ?? ""}
              placeholder="you@example.com"
              onChange={(value) => set({ accessEmail: value || null })}
            />
            <TextField
              label="Access client ID"
              mono
              value={profile.accessClientId ?? ""}
              onChange={(value) => set({ accessClientId: value || null })}
            />
            <TextField
              label="Access client secret"
              type="password"
              value={profile.accessClientSecret ?? ""}
              onChange={(value) => set({ accessClientSecret: value || null })}
            />
            <TextField
              label="Existing token"
              type="password"
              value={profile.accessToken ?? ""}
              onChange={(value) => set({ accessToken: value || null })}
              help="Skips sign-in when you already hold one."
            />
          </div>
          <p className="text-[13px] text-muted-foreground">
            {t(
              "The client secret and the token are held in memory and passed to the core through its environment. Neither is written to the profile on disk, and neither appears in a diagnostics report. The team, client ID and email are saved with the profile on this device.",
            )}
          </p>
          <Separator />
          <Row first title="Send web traffic to Gateway" help="Applies the enrolled organisation's policy. Adds a hop, and permits its logging.">
            <Switch checked={profile.gateway} onCheckedChange={(gateway) => set({ gateway })} />
          </Row>
        </CardContent>
      </Card>

      <div className="flex justify-end pt-1">
        <Button onClick={onSave} className="gap-2 shadow-sm">
          <Save className="size-4" />
          {t("保存身份设置")}
        </Button>
      </div>
    </div>
  );
}
