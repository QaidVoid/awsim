/**
 * Shared types for the Request Log page.
 */

export type LogTab = "all" | "errors" | "slow";

export type ColumnKey =
  | "time"
  | "method"
  | "service"
  | "operation"
  | "path"
  | "region"
  | "status"
  | "duration";

export const COLUMN_LABELS: Record<ColumnKey, string> = {
  time: "Time",
  method: "Method",
  service: "Service",
  operation: "Operation",
  path: "Path",
  region: "Region",
  status: "Status",
  duration: "Duration",
};

export const ALL_COLUMNS: ColumnKey[] = [
  "time",
  "method",
  "service",
  "operation",
  "path",
  "region",
  "status",
  "duration",
];

export const DEFAULT_COLUMNS: Record<ColumnKey, boolean> = {
  time: true,
  method: true,
  service: true,
  operation: true,
  path: false,
  region: false,
  status: true,
  duration: true,
};
