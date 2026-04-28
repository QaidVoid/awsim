/**
 * CloudTrail API client.
 *
 * Wraps the AWSim CloudTrail JSON 1.1 protocol
 * (`X-Amz-Target: CloudTrail_20131101.<Action>`). Returns shapes already
 * normalised to camelCase + sane defaults.
 */

const ENDPOINT = "http://localhost:4566";
const FAKE_DATE = new Date().toISOString().slice(0, 10).replace(/-/g, "");

function authHeader(): string {
  return `AWS4-HMAC-SHA256 Credential=test/${FAKE_DATE}/us-east-1/cloudtrail/aws4_request, SignedHeaders=host;x-amz-date, Signature=fakesignature`;
}

function amzDate(): string {
  return new Date().toISOString().replace(/[:-]/g, "").slice(0, 15) + "Z";
}

async function ctRequest<T>(
  action: string,
  body: Record<string, unknown> = {},
): Promise<T> {
  const res = await fetch(`${ENDPOINT}/`, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.1",
      "X-Amz-Target": `CloudTrail_20131101.${action}`,
      Authorization: authHeader(),
      "X-Amz-Date": amzDate(),
    },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`HTTP ${res.status}: ${text || res.statusText}`);
  }
  const text = await res.text();
  return text ? (JSON.parse(text) as T) : ({} as T);
}

// -- Types --

export interface Trail {
  name: string;
  arn: string;
  s3BucketName: string;
  s3KeyPrefix?: string;
  homeRegion?: string;
  isMultiRegionTrail?: boolean;
  isOrganizationTrail?: boolean;
  hasCustomEventSelectors?: boolean;
  hasInsightSelectors?: boolean;
  isLogging?: boolean;
}

export interface TrailListEntry {
  name: string;
  arn: string;
  homeRegion?: string;
}

export interface TrailEvent {
  eventId: string;
  eventName: string;
  eventTime: number;
  eventSource: string;
  username?: string;
  resources: { type?: string; name?: string }[];
  region?: string;
  cloudTrailEvent?: string;
}

export interface EventSelector {
  readWriteType?: string;
  includeManagementEvents?: boolean;
  dataResources?: { type: string; values: string[] }[];
}

// -- Raw shapes --

interface RawTrail {
  Name?: string;
  TrailARN?: string;
  S3BucketName?: string;
  S3KeyPrefix?: string;
  HomeRegion?: string;
  IsMultiRegionTrail?: boolean;
  IsOrganizationTrail?: boolean;
  HasCustomEventSelectors?: boolean;
  HasInsightSelectors?: boolean;
}

interface RawEvent {
  EventId?: string;
  EventName?: string;
  EventTime?: number;
  EventSource?: string;
  Username?: string;
  Resources?: { ResourceType?: string; ResourceName?: string }[];
  AwsRegion?: string;
  CloudTrailEvent?: string;
}

// -- Operations --

export async function listTrails(): Promise<{ trails: TrailListEntry[] }> {
  const data = await ctRequest<{
    Trails?: { Name?: string; TrailARN?: string; HomeRegion?: string }[];
  }>("ListTrails", {});
  return {
    trails: (data.Trails ?? []).map((t) => ({
      name: t.Name ?? "",
      arn: t.TrailARN ?? "",
      homeRegion: t.HomeRegion,
    })),
  };
}

export async function describeTrails(): Promise<{ trails: Trail[] }> {
  const data = await ctRequest<{ trailList?: RawTrail[] }>(
    "DescribeTrails",
    {},
  );
  return {
    trails: (data.trailList ?? []).map((t) => ({
      name: t.Name ?? "",
      arn: t.TrailARN ?? "",
      s3BucketName: t.S3BucketName ?? "",
      s3KeyPrefix: t.S3KeyPrefix,
      homeRegion: t.HomeRegion,
      isMultiRegionTrail: t.IsMultiRegionTrail,
      isOrganizationTrail: t.IsOrganizationTrail,
      hasCustomEventSelectors: t.HasCustomEventSelectors,
      hasInsightSelectors: t.HasInsightSelectors,
    })),
  };
}

export async function getTrailStatus(
  name: string,
): Promise<{ isLogging: boolean; latestDeliveryTime?: number }> {
  const data = await ctRequest<{
    IsLogging?: boolean;
    LatestDeliveryTime?: number;
  }>("GetTrailStatus", { Name: name });
  return {
    isLogging: !!data.IsLogging,
    latestDeliveryTime: data.LatestDeliveryTime,
  };
}

export type LookupAttributeKey =
  | "EventId"
  | "EventName"
  | "Username"
  | "ResourceType"
  | "ResourceName"
  | "EventSource"
  | "ReadOnly"
  | "AccessKeyId";

export interface LookupOptions {
  attribute?: { key: LookupAttributeKey; value: string };
  startTimeSecs?: number;
  endTimeSecs?: number;
  maxResults?: number;
}

export async function lookupEvents(
  opts: LookupOptions = {},
): Promise<{ events: TrailEvent[] }> {
  const body: Record<string, unknown> = {
    MaxResults: opts.maxResults ?? 50,
  };
  if (opts.attribute) {
    body["LookupAttributes"] = [
      {
        AttributeKey: opts.attribute.key,
        AttributeValue: opts.attribute.value,
      },
    ];
  }
  if (opts.startTimeSecs) body["StartTime"] = opts.startTimeSecs;
  if (opts.endTimeSecs) body["EndTime"] = opts.endTimeSecs;

  const data = await ctRequest<{ Events?: RawEvent[] }>("LookupEvents", body);
  return {
    events: (data.Events ?? []).map((e) => ({
      eventId: e.EventId ?? "",
      eventName: e.EventName ?? "",
      eventTime:
        typeof e.EventTime === "number" ? Math.floor(e.EventTime * 1000) : 0,
      eventSource: e.EventSource ?? "",
      username: e.Username,
      resources: (e.Resources ?? []).map((r) => ({
        type: r.ResourceType,
        name: r.ResourceName,
      })),
      region: e.AwsRegion,
      cloudTrailEvent: e.CloudTrailEvent,
    })),
  };
}

export async function getEventSelectors(
  trailName: string,
): Promise<{ eventSelectors: EventSelector[] }> {
  const data = await ctRequest<{
    EventSelectors?: {
      ReadWriteType?: string;
      IncludeManagementEvents?: boolean;
      DataResources?: { Type?: string; Values?: string[] }[];
    }[];
  }>("GetEventSelectors", { TrailName: trailName });
  return {
    eventSelectors: (data.EventSelectors ?? []).map((s) => ({
      readWriteType: s.ReadWriteType,
      includeManagementEvents: s.IncludeManagementEvents,
      dataResources: (s.DataResources ?? []).map((r) => ({
        type: r.Type ?? "",
        values: r.Values ?? [],
      })),
    })),
  };
}

export async function startLogging(name: string): Promise<void> {
  await ctRequest("StartLogging", { Name: name });
}

export async function stopLogging(name: string): Promise<void> {
  await ctRequest("StopLogging", { Name: name });
}

export async function createTrail(
  name: string,
  s3BucketName: string,
): Promise<void> {
  await ctRequest("CreateTrail", { Name: name, S3BucketName: s3BucketName });
}

export async function deleteTrail(name: string): Promise<void> {
  await ctRequest("DeleteTrail", { Name: name });
}
