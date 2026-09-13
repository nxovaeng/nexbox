import { Power, Radar, ShieldAlert, ShieldCheck } from "lucide-react";

/**
 * What the orb shows. Derived from the core state, never set directly, so the
 * ring can never disagree with what the supervisor is actually doing.
 */
export type OrbState = "idle" | "working" | "live" | "failed";

/**
 * One control carrying the connection state in colour, motion and a ring.
 *
 * Geometry is shared with the Android client so the two feel like one product:
 * a 240 viewBox with the halo at 118, the track at 104, the dotted ring at 90
 * and the core at 75.
 *
 * While searching, the sweep is indeterminate on purpose — the supervisor
 * reports an attempt number and a message, not progress, so a filling bar
 * would be inventing a number.
 */
const SIGNAL: Record<OrbState, string> = {
  idle: "hsl(220 5% 52%)",
  working: "hsl(38 92% 50%)",
  live: "hsl(142 70% 45%)",
  failed: "hsl(0 72% 55%)",
};

const ICON = { idle: Power, working: Radar, live: ShieldCheck, failed: ShieldAlert };

interface ConnectOrbProps {
  state: OrbState;
  caption: string;
  disabled?: boolean;
  onClick: () => void;
}

export function ConnectOrb({ state, caption, disabled, onClick }: ConnectOrbProps) {
  const colour = SIGNAL[state];
  const Icon = ICON[state];
  const moving = state === "live" || state === "working";

  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      aria-label={caption}
      className="group relative grid size-[296px] place-items-center rounded-full transition-transform
        focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-4
        focus-visible:ring-offset-background enabled:active:scale-[0.97] disabled:cursor-not-allowed
        disabled:opacity-60"
    >
      <svg viewBox="0 0 240 240" className="absolute inset-0 size-full" aria-hidden>
        <defs>
          <radialGradient id="orb-bloom">
            <stop offset="0" stopColor={colour} stopOpacity="0.55" />
            <stop offset="1" stopColor={colour} stopOpacity="0" />
          </radialGradient>
          <linearGradient id="orb-sweep" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor={colour} stopOpacity="0" />
            <stop offset="1" stopColor={colour} stopOpacity="1" />
          </linearGradient>
          <radialGradient id="orb-core">
            <stop offset="0" stopColor={colour} stopOpacity="0.17" />
            <stop offset="1" stopColor="hsl(var(--card))" stopOpacity="1" />
          </radialGradient>
        </defs>

        <circle
          className="orb-bloom"
          cx="120"
          cy="120"
          r="118"
          fill="url(#orb-bloom)"
          opacity={state === "live" ? 0.5 : state === "working" ? 0.34 : 0.16}
        />

        {moving
          ? [0, -1.13, -2.26].map((delay) => (
              <circle
                key={delay}
                className="orb-ripple"
                cx="120"
                cy="120"
                r="75"
                fill="none"
                strokeWidth="1.2"
                stroke={colour}
                style={{ animationDelay: `${delay}s` }}
              />
            ))
          : null}

        <circle cx="120" cy="120" r="118" fill="none" stroke={colour} strokeOpacity="0.16" />
        <circle cx="120" cy="120" r="104" fill="none" stroke="hsl(var(--border-strong))" strokeWidth="2.5" />

        {state === "live" || state === "idle" ? (
          <circle
            className={state === "live" ? "orb-orbit" : undefined}
            cx="120"
            cy="120"
            r="90"
            fill="none"
            stroke={colour}
            strokeOpacity={state === "live" ? 0.34 : 0.17}
            strokeWidth="1.5"
            strokeLinecap="round"
            strokeDasharray="1 9"
          />
        ) : null}

        {state === "working" ? (
          <g className="orb-sweep">
            <circle
              cx="120"
              cy="120"
              r="118"
              fill="none"
              stroke="url(#orb-sweep)"
              strokeWidth="2.8"
              strokeLinecap="round"
              strokeDasharray="215 527"
            />
            <path d="M117 106 L133.5 113.5 L117 121 Z" fill={colour} />
          </g>
        ) : null}

        {state === "live" || state === "failed" ? (
          <circle cx="120" cy="120" r="104" fill="none" stroke={colour} strokeWidth="3.5" />
        ) : null}

        <circle cx="120" cy="120" r="75" fill="url(#orb-core)" stroke={colour} strokeOpacity="0.34" />
      </svg>

      <span className="relative flex size-[186px] flex-col items-center justify-center gap-2.5 rounded-full">
        <Icon className="size-11" strokeWidth={1.5} style={{ color: colour }} />
        <span
          className="text-[10.5px] font-bold uppercase tracking-[0.13em]"
          style={{ color: state === "idle" ? "hsl(var(--muted-foreground))" : colour }}
        >
          {caption}
        </span>
      </span>
    </button>
  );
}
