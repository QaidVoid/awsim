/**
 * Display-formatting helpers used across the dashboard.
 *
 * Keep these pure and DOM-free so they remain trivially unit-testable
 * and safe to call during SSR.
 */

/**
 * Convert a byte count into a human-readable string (KB / MB / GB / TB).
 * Returns `0 B` for null/undefined/NaN/negative inputs so callers can
 * pipe storage results in directly without guarding.
 */
export function bytesHuman(n: number | null | undefined): string {
  if (n === null || n === undefined || Number.isNaN(n) || n < 0) return "0 B";
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB", "PB"];
  let value = n / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  const rounded =
    value >= 100 ? Math.round(value) : Math.round(value * 10) / 10;
  return `${rounded} ${units[unit]}`;
}

/**
 * Format a unix-style timestamp (seconds, fractional ok) as a relative
 * string like `12s ago`, `3m ago`, `2h ago`, `5d ago`. Future timestamps
 * are clamped to `just now`.
 */
export function relativeTime(
  tsSeconds: number,
  now: number = Date.now() / 1000,
): string {
  const delta = Math.max(0, now - tsSeconds);
  if (delta < 1) return "just now";
  if (delta < 60) return `${Math.floor(delta)}s ago`;
  if (delta < 3600) return `${Math.floor(delta / 60)}m ago`;
  if (delta < 86400) return `${Math.floor(delta / 3600)}h ago`;
  return `${Math.floor(delta / 86400)}d ago`;
}

/**
 * Format an uptime in seconds as a compact `Xh Ym` / `Xm Ys` / `Xs`
 * string. Suitable for KPI cards where space is at a premium.
 */
export function durationHuman(secs: number | null | undefined): string {
  if (secs === null || secs === undefined || Number.isNaN(secs) || secs < 0)
    return "0s";
  const s = Math.floor(secs);
  const days = Math.floor(s / 86400);
  const hours = Math.floor((s % 86400) / 3600);
  const mins = Math.floor((s % 3600) / 60);
  const seconds = s % 60;
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${mins}m`;
  if (mins > 0) return `${mins}m ${seconds}s`;
  return `${seconds}s`;
}
