import { useMemo } from "react";
import { type Sample, WINDOW, scaleOf } from "./latency";

/**
 * The round-trip chart.
 *
 * Drawn with `preserveAspectRatio="none"` so the curve fills whatever width the
 * card has, with a non-scaling stroke so that stretching does not thicken the
 * line. Runs of failed probes break the path rather than joining across the gap.
 */
export function Sparkline({ history, height = 132 }: { history: Sample[]; height?: number }) {
  const width = 430;
  const { lo, hi } = useMemo(() => scaleOf(history), [history]);

  const segments = useMemo(() => {
    // Right-aligned, so a window that is still filling grows from the right and
    // the newest sample is always against the "now" edge.
    const offset = WINDOW - history.length;
    const step = width / (WINDOW - 1);
    const runs: Array<Array<[number, number]>> = [];
    let run: Array<[number, number]> = [];
    history.forEach((sample, index) => {
      if (sample == null) {
        if (run.length) runs.push(run);
        run = [];
        return;
      }
      const x = (offset + index) * step;
      const y = height - ((sample - lo) / (hi - lo || 1)) * (height - 16) - 8;
      run.push([x, y]);
    });
    if (run.length) runs.push(run);
    return runs;
  }, [history, lo, hi, height]);

  const path = (points: Array<[number, number]>) => {
    if (points.length === 1) {
      // A lone sample has no curve; a short flat tick still reads as a value.
      const [x, y] = points[0];
      return `M${(x - 3).toFixed(1)},${y.toFixed(1)} L${(x + 3).toFixed(1)},${y.toFixed(1)}`;
    }
    return points.reduce((d, [x, y], index) => {
      if (index === 0) return `M${x.toFixed(1)},${y.toFixed(1)}`;
      const [px, py] = points[index - 1];
      const half = (x - px) / 2;
      return `${d} C${(px + half).toFixed(1)},${py.toFixed(1)} ${(x - half).toFixed(1)},${y.toFixed(1)} ${x.toFixed(1)},${y.toFixed(1)}`;
    }, "");
  };

  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      style={{ height }}
      className="w-full"
      role="img"
      aria-label="Round-trip through the tunnel over the last eighty seconds"
    >
      <defs>
        <linearGradient id="spark-fill" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="hsl(var(--primary))" stopOpacity="0.3" />
          <stop offset="1" stopColor="hsl(var(--primary))" stopOpacity="0" />
        </linearGradient>
      </defs>
      {[0.25, 0.5, 0.75].map((fraction) => (
        <line
          key={fraction}
          x1="0"
          y1={height * fraction}
          x2={width}
          y2={height * fraction}
          stroke="hsl(var(--border))"
          vectorEffect="non-scaling-stroke"
        />
      ))}
      {segments.map((points, index) => (
        <g key={index}>
          {points.length > 1 ? (
            <path
              d={`${path(points)} L${points[points.length - 1][0].toFixed(1)},${height} L${points[0][0].toFixed(1)},${height} Z`}
              fill="url(#spark-fill)"
            />
          ) : null}
          <path
            d={path(points)}
            fill="none"
            stroke="hsl(var(--primary))"
            strokeWidth="2"
            strokeLinecap="round"
            vectorEffect="non-scaling-stroke"
          />
        </g>
      ))}
    </svg>
  );
}
