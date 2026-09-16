import { useState, useMemo, useRef, useEffect } from "react";
import { useT } from "@/core/useT";
import { Copy, Check, Trash2, Search, ArrowDown, Terminal } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import type { CoreLogEvent } from "@/types";

const LEVEL_COLORS: Record<string, { badge: string; text: string }> = {
  error: { badge: "bg-red-500/15 text-red-500 border-red-500/30", text: "text-red-400" },
  warn: { badge: "bg-amber-500/15 text-amber-500 border-amber-500/30", text: "text-amber-400" },
  info: { badge: "bg-blue-500/15 text-blue-500 border-blue-500/30", text: "text-blue-300" },
  debug: { badge: "bg-zinc-500/15 text-zinc-400 border-zinc-500/30", text: "text-zinc-400" },
  trace: { badge: "bg-zinc-600/15 text-zinc-500 border-zinc-600/30", text: "text-zinc-500" },
};

interface LiveLogsProps {
  logs: CoreLogEvent[];
}

export function LiveLogs({ logs }: LiveLogsProps) {
  const t = useT();
  const [levelFilter, setLevelFilter] = useState<string>("all");
  const [streamFilter, setStreamFilter] = useState<string>("all");
  const [search, setSearch] = useState<string>("");
  const [autoScroll, setAutoScroll] = useState<boolean>(true);
  const [copied, setCopied] = useState<boolean>(false);
  const [clearedAt, setClearedAt] = useState<number>(0);

  const containerRef = useRef<HTMLDivElement | null>(null);

  // Available streams in logs
  const streams = useMemo(() => {
    const set = new Set<string>();
    for (const l of logs) {
      if (l.stream) set.add(l.stream);
    }
    return Array.from(set);
  }, [logs]);

  // Filter logs
  const filtered = useMemo(() => {
    return logs.filter((entry) => {
      if (clearedAt > 0 && entry.timestamp <= clearedAt) return false;
      if (levelFilter !== "all" && entry.level.toLowerCase() !== levelFilter) return false;
      if (streamFilter !== "all" && entry.stream !== streamFilter) return false;
      if (search.trim()) {
        const q = search.toLowerCase();
        return (
          entry.message.toLowerCase().includes(q) ||
          entry.stream.toLowerCase().includes(q) ||
          entry.level.toLowerCase().includes(q)
        );
      }
      return true;
    });
  }, [logs, levelFilter, streamFilter, search, clearedAt]);

  // Auto-scroll to bottom when new logs arrive
  useEffect(() => {
    if (autoScroll && containerRef.current) {
      containerRef.current.scrollTop = containerRef.current.scrollHeight;
    }
  }, [filtered.length, autoScroll]);

  const handleCopy = async () => {
    try {
      const text = filtered
        .map(
          (e) =>
            `[${new Date(e.timestamp).toLocaleTimeString()}] [${e.stream}/${e.level.toUpperCase()}] ${e.message}`,
        )
        .join("\n");
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch (err) {
      console.error("Failed to copy logs", err);
    }
  };

  const handleClear = () => {
    if (logs.length > 0) {
      setClearedAt(logs[logs.length - 1].timestamp);
    }
  };

  return (
    <Card className="flex flex-col h-[calc(100vh-140px)] shadow-sm border-border/80">
      <CardHeader className="pb-3 border-b border-border/40">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex items-center gap-2">
            <Terminal className="size-4 text-primary" />
            <CardTitle className="text-base font-semibold">{t("实时内核运行日志")}</CardTitle>
            <Badge variant="outline" className="font-mono text-xs ml-1 bg-muted/40">
              {filtered.length} 条事件
            </Badge>
          </div>

          {/* Action buttons */}
          <div className="flex items-center gap-2">
            <Button
              variant={autoScroll ? "default" : "outline"}
              size="sm"
              className="h-7 text-xs gap-1"
              onClick={() => setAutoScroll(!autoScroll)}
            >
              <ArrowDown className={`size-3 ${autoScroll ? "animate-bounce" : ""}`} />
              {autoScroll ? t("自动滚屏: 开") : t("自动滚屏: 关")}
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="h-7 text-xs gap-1"
              onClick={handleCopy}
              disabled={filtered.length === 0}
            >
              {copied ? <Check className="size-3 text-emerald-500" /> : <Copy className="size-3" />}
              {copied ? t("已复制") : t("复制日志")}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="h-7 text-xs gap-1 text-muted-foreground hover:text-destructive"
              onClick={handleClear}
              disabled={filtered.length === 0}
            >
              <Trash2 className="size-3" />
              {t("清屏")}
            </Button>
          </div>
        </div>

        <CardDescription className="text-xs text-muted-foreground mt-1">
          {t("实时捕获 CoreSupervisor 内核守护进程、Aether / Proton / Psiphon 子进程的控制台流与错误事件。")}
        </CardDescription>

        {/* Filters Toolbar */}
        <div className="flex flex-wrap items-center gap-2 pt-2 text-xs">
          {/* Search box */}
          <div className="relative flex-1 min-w-[180px] max-w-sm">
            <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 size-3.5 text-muted-foreground" />
            <Input
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              placeholder={t("按内容、级别或来源搜索...")}
              className="h-7 pl-8 text-xs bg-muted/30"
            />
          </div>

          {/* Level filter */}
          <div className="flex items-center gap-1 bg-muted/40 p-0.5 rounded-md border border-border/50">
            {["all", "error", "warn", "info", "debug"].map((lvl) => (
              <button
                key={lvl}
                type="button"
                onClick={() => setLevelFilter(lvl)}
                className={[
                  "px-2 py-0.5 rounded text-[11px] font-medium transition-colors uppercase",
                  levelFilter === lvl
                    ? "bg-background text-foreground shadow-xs"
                    : "text-muted-foreground hover:text-foreground",
                ].join(" ")}
              >
                {lvl}
              </button>
            ))}
          </div>

          {/* Stream filter */}
          {streams.length > 0 && (
            <select
              value={streamFilter}
              onChange={(e) => setStreamFilter(e.target.value)}
              className="bg-muted/40 text-foreground border border-border/50 rounded-md px-2 py-1 text-[11px] focus:outline-none focus:ring-1 focus:ring-primary"
            >
              <option value="all">全部来源 ({streams.length})</option>
              {streams.map((s) => (
                <option key={s} value={s}>
                  {s}
                </option>
              ))}
            </select>
          )}
        </div>
      </CardHeader>

      <CardContent className="flex-1 p-0 min-h-0 relative">
        <div
          ref={containerRef}
          className="absolute inset-0 overflow-y-auto bg-zinc-950 p-3 font-mono text-[11.5px] leading-relaxed text-zinc-300 selection:bg-primary/30"
        >
          {filtered.length === 0 ? (
            <div className="h-full flex flex-col items-center justify-center text-zinc-500 py-12">
              <Terminal className="size-8 stroke-[1.2] opacity-40 mb-2" />
              <p className="text-xs">{t("暂无符合过滤条件的运行日志")}</p>
              <p className="text-[11px] text-zinc-600 mt-0.5">
                {t("当内核启动、连接、握手或报错时，日志将实时推送到此。")}
              </p>
            </div>
          ) : (
            <div className="space-y-0.5">
              {filtered.map((entry, index) => {
                const conf = LEVEL_COLORS[entry.level.toLowerCase()] || {
                  badge: "bg-zinc-600/20 text-zinc-400 border-zinc-600/30",
                  text: "text-zinc-300",
                };
                return (
                  <div
                    key={`${entry.timestamp}-${index}`}
                    className="flex items-start gap-2.5 py-0.5 px-1.5 rounded hover:bg-zinc-900/60 transition-colors group"
                  >
                    <span className="shrink-0 text-zinc-500 tabular select-none text-[11px] pt-0.5">
                      {new Date(entry.timestamp).toLocaleTimeString()}
                    </span>

                    <span className="shrink-0 inline-block px-1.5 py-0.2 text-[10px] font-semibold uppercase rounded border border-border/30 bg-zinc-900 text-zinc-400 select-none">
                      {entry.stream}
                    </span>

                    <span
                      className={`shrink-0 inline-block px-1.5 py-0.2 text-[10px] font-bold uppercase rounded border ${conf.badge} select-none`}
                    >
                      {entry.level}
                    </span>

                    <span className={`break-all flex-1 ${conf.text}`}>
                      {entry.message}
                    </span>
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  );
}
