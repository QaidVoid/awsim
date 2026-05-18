/**
 * Typed EventBridge API client.
 *
 * Wraps the AWS JSON 1.1 Events API with strong types.
 */

import { ENDPOINT, amzDate, authHeader, loggedFetch } from "$lib/aws";

const SERVICE = "events";
const TARGET_PREFIX = "AWSEvents";

// ---------- Types ----------

export interface EventBus {
  name: string;
  arn: string;
  policy?: string;
}

export interface Rule {
  name: string;
  arn: string;
  state: "ENABLED" | "DISABLED";
  eventPattern?: string;
  description?: string;
  scheduleExpression?: string;
  eventBusName?: string;
}

export interface Archive {
  name: string;
  arn: string;
  eventSourceArn: string;
  state: string;
  retentionDays: number;
  sizeBytes: number;
  eventCount: number;
  creationTime?: number;
}

export interface PutEventEntry {
  source: string;
  detailType: string;
  detail: string;
  eventBusName?: string;
  resources?: string[];
}

// ---------- Internal request ----------

async function request<T>(
  action: string,
  params: Record<string, unknown> = {},
): Promise<T> {
  const res = await loggedFetch(SERVICE, action, "POST", ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.1",
      "X-Amz-Target": `${TARGET_PREFIX}.${action}`,
      Authorization: authHeader(SERVICE),
      "X-Amz-Date": amzDate(),
    },
    body: JSON.stringify(params),
  });
  if (!res.ok) {
    throw new Error(
      `EventBridge ${action} failed (HTTP ${res.status}): ${await res.text()}`,
    );
  }
  const text = await res.text();
  return (text ? JSON.parse(text) : {}) as T;
}

// ---------- Operations ----------

export async function listEventBuses(): Promise<EventBus[]> {
  const data = await request<{
    EventBuses?: { Name: string; Arn: string; Policy?: string }[];
  }>("ListEventBuses");
  return (data.EventBuses ?? []).map((b) => ({
    name: b.Name,
    arn: b.Arn,
    policy: b.Policy,
  }));
}

export async function listRules(busName?: string): Promise<Rule[]> {
  const params: Record<string, unknown> = {};
  if (busName) params["EventBusName"] = busName;
  const data = await request<{
    Rules?: {
      Name: string;
      Arn: string;
      State: "ENABLED" | "DISABLED";
      EventPattern?: string;
      Description?: string;
      ScheduleExpression?: string;
      EventBusName?: string;
    }[];
  }>("ListRules", params);
  return (data.Rules ?? []).map((r) => ({
    name: r.Name,
    arn: r.Arn,
    state: r.State,
    eventPattern: r.EventPattern,
    description: r.Description,
    scheduleExpression: r.ScheduleExpression,
    eventBusName: r.EventBusName,
  }));
}

export async function describeRule(
  name: string,
  busName?: string,
): Promise<Rule> {
  const params: Record<string, unknown> = { Name: name };
  if (busName) params["EventBusName"] = busName;
  const data = await request<{
    Name: string;
    Arn: string;
    State: "ENABLED" | "DISABLED";
    EventPattern?: string;
    Description?: string;
    ScheduleExpression?: string;
    EventBusName?: string;
  }>("DescribeRule", params);
  return {
    name: data.Name,
    arn: data.Arn,
    state: data.State,
    eventPattern: data.EventPattern,
    description: data.Description,
    scheduleExpression: data.ScheduleExpression,
    eventBusName: data.EventBusName,
  };
}

export interface PutRuleInput {
  name: string;
  busName?: string;
  eventPattern?: string;
  scheduleExpression?: string;
  description?: string;
  state?: "ENABLED" | "DISABLED";
}

export async function putRule(
  input: PutRuleInput,
): Promise<{ ruleArn: string }> {
  const params: Record<string, unknown> = {
    Name: input.name,
    State: input.state ?? "ENABLED",
  };
  if (input.busName) params["EventBusName"] = input.busName;
  if (input.eventPattern) params["EventPattern"] = input.eventPattern;
  if (input.scheduleExpression)
    params["ScheduleExpression"] = input.scheduleExpression;
  if (input.description) params["Description"] = input.description;
  const data = await request<{ RuleArn?: string }>("PutRule", params);
  return { ruleArn: data.RuleArn ?? "" };
}

export async function deleteRule(
  name: string,
  busName?: string,
): Promise<void> {
  const params: Record<string, unknown> = { Name: name, Force: true };
  if (busName) params["EventBusName"] = busName;
  await request("DeleteRule", params);
}

export async function putEvents(
  entries: PutEventEntry[],
): Promise<{ failedEntryCount: number }> {
  const data = await request<{ FailedEntryCount?: number }>("PutEvents", {
    Entries: entries.map((e) => ({
      Source: e.source,
      DetailType: e.detailType,
      Detail: e.detail,
      EventBusName: e.eventBusName,
      Resources: e.resources,
    })),
  });
  return { failedEntryCount: data.FailedEntryCount ?? 0 };
}

/**
 * TestEventPattern - does `event` (a JSON string in the canonical
 * EventBridge envelope shape) match `eventPattern` (a rule's pattern
 * JSON string)? Uses the same matcher PutEvents routes with.
 */
export async function testEventPattern(
  eventPattern: string,
  event: string,
): Promise<boolean> {
  const data = await request<{ Result?: boolean }>("TestEventPattern", {
    EventPattern: eventPattern,
    Event: event,
  });
  return !!data.Result;
}

export async function listArchives(
  eventSourceArn?: string,
): Promise<Archive[]> {
  const params: Record<string, unknown> = {};
  if (eventSourceArn) params["EventSourceArn"] = eventSourceArn;
  const data = await request<{
    Archives?: {
      ArchiveName: string;
      EventSourceArn: string;
      State: string;
      RetentionDays?: number;
      SizeBytes?: number;
      EventCount?: number;
      CreationTime?: number;
    }[];
  }>("ListArchives", params);
  return (data.Archives ?? []).map((a) => ({
    name: a.ArchiveName,
    arn: "",
    eventSourceArn: a.EventSourceArn,
    state: a.State,
    retentionDays: a.RetentionDays ?? 0,
    sizeBytes: a.SizeBytes ?? 0,
    eventCount: a.EventCount ?? 0,
    creationTime: a.CreationTime,
  }));
}

export async function describeArchive(name: string): Promise<Archive> {
  const data = await request<{
    ArchiveName: string;
    ArchiveArn: string;
    EventSourceArn: string;
    State: string;
    RetentionDays?: number;
    SizeBytes?: number;
    EventCount?: number;
    CreationTime?: number;
  }>("DescribeArchive", { ArchiveName: name });
  return {
    name: data.ArchiveName,
    arn: data.ArchiveArn,
    eventSourceArn: data.EventSourceArn,
    state: data.State,
    retentionDays: data.RetentionDays ?? 0,
    sizeBytes: data.SizeBytes ?? 0,
    eventCount: data.EventCount ?? 0,
    creationTime: data.CreationTime,
  };
}
