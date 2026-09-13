/**
 * The rolling window of round-trip samples behind the status chart.
 *
 * Samples are kept as `number | null` rather than dropped, because a probe that
 * did not come back is information: the gap in the line is where the route went
 * quiet. Collapsing failures out of the series would draw a healthy chart over
 * an unhealthy minute.
 */
export type Sample = number | null;

/** Sixteen samples at five seconds is the eighty-second window on the chart. */
export const WINDOW = 16;
export const SAMPLE_MS = 5_000;

export interface LatencySummary {
  min: number | null;
  avg: number | null;
  max: number | null;
  /** Share of samples in the window that never answered, 0 to 1. */
  loss: number;
  /** The most recent figure, or null when the last probe failed. */
  last: number | null;
}

export function append(history: Sample[], sample: Sample): Sample[] {
  return [...history, sample].slice(-WINDOW);
}

export function summarise(history: Sample[]): LatencySummary {
  const answered = history.filter((value): value is number => value != null);
  if (!history.length) return { min: null, avg: null, max: null, loss: 0, last: null };
  const last = history[history.length - 1] ?? null;
  if (!answered.length) return { min: null, avg: null, max: null, loss: 1, last };
  const total = answered.reduce((sum, value) => sum + value, 0);
  return {
    min: Math.min(...answered),
    avg: total / answered.length,
    max: Math.max(...answered),
    loss: (history.length - answered.length) / history.length,
    last,
  };
}

/**
 * The vertical range to draw, padded so a flat line does not sit on the floor
 * and a single spike does not flatten everything else.
 */
export function scaleOf(history: Sample[]): { lo: number; hi: number } {
  const answered = history.filter((value): value is number => value != null);
  if (!answered.length) return { lo: 0, hi: 100 };
  const min = Math.min(...answered);
  const max = Math.max(...answered);
  const pad = Math.max(8, (max - min) * 0.25);
  return { lo: Math.max(0, min - pad), hi: max + pad };
}
