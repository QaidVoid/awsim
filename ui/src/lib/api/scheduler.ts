/**
 * Typed EventBridge Scheduler API client.
 *
 * Wraps AWSim's REST scheduler endpoints. The Scheduler API is REST/JSON
 * (not AWS-JSON), so we hit `/schedules` and `/schedule-groups` with HTTP verbs.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "scheduler";

// ---------- Types ----------

export type ScheduleState = "ENABLED" | "DISABLED";

export interface ScheduleSummary {
  name: string;
  arn: string;
  groupName: string;
  scheduleExpression: string;
  state: ScheduleState;
  creationDate?: number;
  lastModificationDate?: number;
  targetArn?: string;
}

export interface ScheduleTarget {
  arn: string;
  roleArn?: string;
  input?: string;
}

export interface FlexibleTimeWindow {
  mode: "OFF" | "FLEXIBLE";
  maximumWindowInMinutes?: number;
}

export interface Schedule {
  name: string;
  arn: string;
  groupName: string;
  scheduleExpression: string;
  scheduleExpressionTimezone?: string;
  state: ScheduleState;
  description?: string;
  startDate?: string;
  endDate?: string;
  creationDate?: number;
  lastModificationDate?: number;
  target: ScheduleTarget;
  flexibleTimeWindow: FlexibleTimeWindow;
}

export interface ScheduleGroup {
  name: string;
  arn: string;
  state: string;
  creationDate?: number;
  lastModificationDate?: number;
}

export interface CreateScheduleInput {
  name: string;
  groupName?: string;
  scheduleExpression: string;
  scheduleExpressionTimezone?: string;
  description?: string;
  state?: ScheduleState;
  target: ScheduleTarget;
  flexibleTimeWindow?: FlexibleTimeWindow;
}

// ---------- Internal request ----------

function headers(): Record<string, string> {
  return {
    "Content-Type": "application/json",
    Authorization: authHeader(SERVICE),
    "X-Amz-Date": amzDate(),
  };
}

async function request<T>(
  action: string,
  method: "GET" | "POST" | "PUT" | "DELETE",
  path: string,
  body?: Record<string, unknown>,
): Promise<T> {
  const opts: RequestInit = { method, headers: headers() };
  if (body !== undefined) opts.body = JSON.stringify(body);
  const res = await loggedFetch(
    SERVICE,
    action,
    method,
    `${ENDPOINT}${path}`,
    opts,
  );
  const text = await res.text();
  if (!res.ok) {
    let msg = text;
    try {
      const data = JSON.parse(text) as { message?: string; Message?: string };
      msg = data.message ?? data.Message ?? text;
    } catch {
      // not JSON, leave as-is
    }
    throw new Error(`Scheduler ${action} failed (HTTP ${res.status}): ${msg}`);
  }
  return (text ? JSON.parse(text) : {}) as T;
}

// ---------- Operations ----------

interface RawScheduleSummary {
  Name: string;
  Arn: string;
  GroupName: string;
  ScheduleExpression?: string;
  State?: ScheduleState;
  CreationDate?: number;
  LastModificationDate?: number;
  Target?: { Arn: string };
}

export async function listSchedules(
  groupName?: string,
): Promise<ScheduleSummary[]> {
  const qs = groupName ? `?groupName=${encodeURIComponent(groupName)}` : "";
  const data = await request<{ Schedules?: RawScheduleSummary[] }>(
    "ListSchedules",
    "GET",
    `/schedules${qs}`,
  );
  return (data.Schedules ?? []).map((s) => ({
    name: s.Name,
    arn: s.Arn,
    groupName: s.GroupName,
    scheduleExpression: s.ScheduleExpression ?? "",
    state: (s.State ?? "ENABLED") as ScheduleState,
    creationDate: s.CreationDate,
    lastModificationDate: s.LastModificationDate,
    targetArn: s.Target?.Arn,
  }));
}

interface RawSchedule {
  Name: string;
  Arn: string;
  GroupName: string;
  ScheduleExpression: string;
  ScheduleExpressionTimezone?: string;
  State: ScheduleState;
  Description?: string;
  StartDate?: string;
  EndDate?: string;
  CreationDate?: number;
  LastModificationDate?: number;
  Target: { Arn: string; RoleArn?: string; Input?: string };
  FlexibleTimeWindow: {
    Mode: "OFF" | "FLEXIBLE";
    MaximumWindowInMinutes?: number;
  };
}

export async function getSchedule(
  name: string,
  groupName = "default",
): Promise<Schedule> {
  const data = await request<RawSchedule>(
    "GetSchedule",
    "GET",
    `/schedules/${encodeURIComponent(name)}?groupName=${encodeURIComponent(groupName)}`,
  );
  return {
    name: data.Name,
    arn: data.Arn,
    groupName: data.GroupName,
    scheduleExpression: data.ScheduleExpression,
    scheduleExpressionTimezone: data.ScheduleExpressionTimezone,
    state: data.State,
    description: data.Description,
    startDate: data.StartDate,
    endDate: data.EndDate,
    creationDate: data.CreationDate,
    lastModificationDate: data.LastModificationDate,
    target: {
      arn: data.Target.Arn,
      roleArn: data.Target.RoleArn,
      input: data.Target.Input,
    },
    flexibleTimeWindow: {
      mode: data.FlexibleTimeWindow.Mode,
      maximumWindowInMinutes: data.FlexibleTimeWindow.MaximumWindowInMinutes,
    },
  };
}

export async function createSchedule(
  input: CreateScheduleInput,
): Promise<{ scheduleArn: string }> {
  const body: Record<string, unknown> = {
    ScheduleExpression: input.scheduleExpression,
    GroupName: input.groupName ?? "default",
    FlexibleTimeWindow: {
      Mode: input.flexibleTimeWindow?.mode ?? "OFF",
      ...(input.flexibleTimeWindow?.maximumWindowInMinutes !== undefined
        ? {
            MaximumWindowInMinutes:
              input.flexibleTimeWindow.maximumWindowInMinutes,
          }
        : {}),
    },
    State: input.state ?? "ENABLED",
    Target: {
      Arn: input.target.arn,
      ...(input.target.roleArn ? { RoleArn: input.target.roleArn } : {}),
      ...(input.target.input ? { Input: input.target.input } : {}),
    },
  };
  if (input.scheduleExpressionTimezone)
    body["ScheduleExpressionTimezone"] = input.scheduleExpressionTimezone;
  if (input.description) body["Description"] = input.description;
  const data = await request<{ ScheduleArn?: string }>(
    "CreateSchedule",
    "POST",
    `/schedules/${encodeURIComponent(input.name)}`,
    body,
  );
  return { scheduleArn: data.ScheduleArn ?? "" };
}

export async function deleteSchedule(
  name: string,
  groupName = "default",
): Promise<void> {
  await request<unknown>(
    "DeleteSchedule",
    "DELETE",
    `/schedules/${encodeURIComponent(name)}?groupName=${encodeURIComponent(groupName)}`,
  );
}

export async function listScheduleGroups(): Promise<ScheduleGroup[]> {
  const data = await request<{
    ScheduleGroups?: {
      Name: string;
      Arn: string;
      State: string;
      CreationDate?: number;
      LastModificationDate?: number;
    }[];
  }>("ListScheduleGroups", "GET", "/schedule-groups");
  return (data.ScheduleGroups ?? []).map((g) => ({
    name: g.Name,
    arn: g.Arn,
    state: g.State,
    creationDate: g.CreationDate,
    lastModificationDate: g.LastModificationDate,
  }));
}

export async function getScheduleGroup(name: string): Promise<ScheduleGroup> {
  const data = await request<{
    Name: string;
    Arn: string;
    State: string;
    CreationDate?: number;
    LastModificationDate?: number;
  }>("GetScheduleGroup", "GET", `/schedule-groups/${encodeURIComponent(name)}`);
  return {
    name: data.Name,
    arn: data.Arn,
    state: data.State,
    creationDate: data.CreationDate,
    lastModificationDate: data.LastModificationDate,
  };
}

export async function createScheduleGroup(name: string): Promise<void> {
  await request<unknown>(
    "CreateScheduleGroup",
    "POST",
    `/schedule-groups/${encodeURIComponent(name)}`,
    {},
  );
}

export async function deleteScheduleGroup(name: string): Promise<void> {
  await request<unknown>(
    "DeleteScheduleGroup",
    "DELETE",
    `/schedule-groups/${encodeURIComponent(name)}`,
  );
}

// ---------- Cron / rate preview helper ----------

/**
 * Tiny client-side estimator for `rate(N unit)` and `cron(...)` expressions.
 *
 * - `rate(N minutes|hours|days)` — yields the next N fire times by adding N
 *   units repeatedly to "now".
 * - `cron(min hour day-of-month month day-of-week year?)` — supports a tiny
 *   subset:
 *     - `*` (any)
 *     - integer (specific value)
 *     - `*\/N` (every N — written `*\/N` to escape the comment in source, but the
 *       expression itself is `*` + `/` + N)
 *   It walks forward minute-by-minute up to `lookaheadMinutes` and returns the
 *   first `count` matches. Anything fancier (lists, ranges, `?`, `L`, `W`,
 *   day-of-week names, year) returns null and the UI shows a "preview not
 *   available" hint.
 */
export function previewNextFireTimes(
  expression: string,
  count = 5,
  from: Date = new Date(),
): Date[] | null {
  const trimmed = expression.trim();
  // rate(N unit)
  const rateMatch =
    /^rate\(\s*(\d+)\s+(minute|minutes|hour|hours|day|days)\s*\)$/i.exec(
      trimmed,
    );
  if (rateMatch) {
    const n = parseInt(rateMatch[1], 10);
    const unit = rateMatch[2].toLowerCase();
    const ms = unit.startsWith("minute")
      ? n * 60_000
      : unit.startsWith("hour")
        ? n * 3_600_000
        : n * 86_400_000;
    if (n <= 0 || ms <= 0) return null;
    const out: Date[] = [];
    let t = from.getTime() + ms;
    for (let i = 0; i < count; i++) {
      out.push(new Date(t));
      t += ms;
    }
    return out;
  }
  // cron(min hour dom month dow [year])
  const cronMatch = /^cron\(\s*(.+?)\s*\)$/i.exec(trimmed);
  if (!cronMatch) return null;
  const parts = cronMatch[1].split(/\s+/);
  if (parts.length < 5 || parts.length > 6) return null;
  const [minF, hourF, domF, monthF, dowF] = parts;
  // Refuse anything we don't understand.
  for (const f of [minF, hourF, domF, monthF, dowF]) {
    if (!isSimpleCronField(f)) return null;
  }
  const matchers = {
    min: cronMatcher(minF, 0, 59),
    hour: cronMatcher(hourF, 0, 23),
    dom: cronMatcher(domF, 1, 31),
    month: cronMatcher(monthF, 1, 12),
    dow: cronMatcher(dowF, 0, 6),
  };
  if (
    !matchers.min ||
    !matchers.hour ||
    !matchers.dom ||
    !matchers.month ||
    !matchers.dow
  ) {
    return null;
  }
  const out: Date[] = [];
  // Walk minute by minute up to ~366 days ahead.
  const lookaheadMs = 366 * 86_400_000;
  const start = new Date(from.getTime() + 60_000 - (from.getTime() % 60_000));
  const limit = start.getTime() + lookaheadMs;
  for (let t = start.getTime(); t < limit && out.length < count; t += 60_000) {
    const d = new Date(t);
    if (
      matchers.min(d.getUTCMinutes()) &&
      matchers.hour(d.getUTCHours()) &&
      matchers.dom(d.getUTCDate()) &&
      matchers.month(d.getUTCMonth() + 1) &&
      matchers.dow(d.getUTCDay())
    ) {
      out.push(d);
    }
  }
  return out.length > 0 ? out : null;
}

function isSimpleCronField(f: string): boolean {
  if (f === "*" || f === "?") return true;
  if (/^\d+$/.test(f)) return true;
  if (/^\*\/\d+$/.test(f)) return true;
  return false;
}

function cronMatcher(
  field: string,
  min: number,
  max: number,
): ((v: number) => boolean) | null {
  if (field === "*" || field === "?") return () => true;
  if (/^\d+$/.test(field)) {
    const n = parseInt(field, 10);
    if (n < min || n > max) return null;
    return (v) => v === n;
  }
  const stepMatch = /^\*\/(\d+)$/.exec(field);
  if (stepMatch) {
    const step = parseInt(stepMatch[1], 10);
    if (step <= 0) return null;
    return (v) => (v - min) % step === 0;
  }
  return null;
}
