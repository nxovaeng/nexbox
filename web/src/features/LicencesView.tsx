import { useT } from "@/core/useT";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { Row } from "./panels";
import notices from "../../THIRD_PARTY_NOTICES.md?raw";

export function LicencesView() {
  const t = useT();
  return (
    <div className="space-y-3.5">
      <Card>
        <CardHeader>
          <CardTitle>{t("What this is built on")}</CardTitle>
          <CardDescription>
            Each component below keeps its own licence. NextVPN's source is derived from WhiteAesther (AGPL-3.0) and
            the full licence texts are installed next to the application under{" "}
            <code className="font-mono text-[12px]">licences/</code>.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-2.5">
          <Row first title="NextVPN" help="This application. Derived from github.com/WhiteDNS/WhiteAesther">
            <span className="font-mono text-[12.5px] text-muted-foreground">AGPL-3.0</span>
          </Row>
          <Separator />
          <Row title="Aether" help="The connection engine, shipped as a binary and run by this app. Aether 1.8.0">
            <span className="font-mono text-[12.5px] text-muted-foreground">AGPL-3.0</span>
          </Row>
          <Separator />
          <Row
            title="mihomo"
            help="The second hop behind Exit chain, run as a separate program. Source at github.com/MetaCubeX/mihomo at tag v1.19.30"
          >
            <span className="font-mono text-[12.5px] text-muted-foreground">GPL-3.0</span>
          </Row>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>{t("Full notices")}</CardTitle>
          <CardDescription>
            The same text that ships with the binary, including trademark terms and where each
            component&apos;s corresponding source lives.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <pre className="max-h-[420px] overflow-auto whitespace-pre-wrap break-words rounded-md border bg-muted/40 p-3.5 font-mono text-[11.5px] leading-relaxed text-muted-foreground">
            {notices}
          </pre>
        </CardContent>
      </Card>
    </div>
  );
}
