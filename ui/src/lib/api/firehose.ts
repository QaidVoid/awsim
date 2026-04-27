/**
 * Typed Kinesis Firehose API client.
 */

import { ENDPOINT, amzDate, authHeader, loggedFetch } from "$lib/aws";

const SERVICE = "firehose";
const TARGET_PREFIX = "Firehose_20150804";

// ---------- Types ----------

export type DestinationType =
  | "S3"
  | "ExtendedS3"
  | "Redshift"
  | "ElasticsearchDestination"
  | "AmazonopensearchserviceDestination"
  | "SplunkDestination"
  | "HttpEndpointDestination"
  | "Unknown";

export interface DeliveryStreamSummary {
  name: string;
  arn: string;
  status: string;
  type: string;
  destinationType: DestinationType;
  destinationDetail: string;
  createTime?: number;
  lastUpdate?: number;
}

export interface DeliveryStreamDetail extends DeliveryStreamSummary {
  versionId: string;
  destinations: DestinationDescription[];
  raw: Record<string, unknown>;
}

export interface DestinationDescription {
  destinationId: string;
  type: DestinationType;
  bucketArn?: string;
  prefix?: string;
  errorOutputPrefix?: string;
  bufferingHints?: { sizeInMBs: number; intervalInSeconds: number };
  compressionFormat?: string;
  endpointUrl?: string;
}

export interface S3DestinationConfiguration {
  bucketArn: string;
  roleArn: string;
  prefix?: string;
  errorOutputPrefix?: string;
  bufferingHints?: { sizeInMBs: number; intervalInSeconds: number };
  compressionFormat?: "UNCOMPRESSED" | "GZIP" | "ZIP" | "Snappy";
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
      `Firehose ${action} failed (HTTP ${res.status}): ${await res.text()}`,
    );
  }
  const text = await res.text();
  return (text ? JSON.parse(text) : {}) as T;
}

function utf8ToBase64(value: string): string {
  return btoa(unescape(encodeURIComponent(value)));
}

function detectDestination(destinations: DescribeDestinationApi[]): {
  type: DestinationType;
  detail: string;
} {
  if (!destinations || destinations.length === 0) {
    return { type: "Unknown", detail: "—" };
  }
  const d = destinations[0];
  if (d.S3DestinationDescription || d.ExtendedS3DestinationDescription) {
    const arn =
      d.ExtendedS3DestinationDescription?.BucketARN ??
      d.S3DestinationDescription?.BucketARN ??
      "";
    return {
      type: d.ExtendedS3DestinationDescription ? "ExtendedS3" : "S3",
      detail: arn,
    };
  }
  if (d.RedshiftDestinationDescription) {
    return {
      type: "Redshift",
      detail: d.RedshiftDestinationDescription.ClusterJDBCURL ?? "—",
    };
  }
  if (d.HttpEndpointDestinationDescription) {
    return {
      type: "HttpEndpointDestination",
      detail:
        d.HttpEndpointDestinationDescription.EndpointConfiguration?.Url ?? "—",
    };
  }
  if (d.ElasticsearchDestinationDescription) {
    return {
      type: "ElasticsearchDestination",
      detail: d.ElasticsearchDestinationDescription.DomainARN ?? "—",
    };
  }
  if (d.SplunkDestinationDescription) {
    return {
      type: "SplunkDestination",
      detail: d.SplunkDestinationDescription.HECEndpoint ?? "—",
    };
  }
  return { type: "Unknown", detail: "—" };
}

interface DescribeDestinationApi {
  DestinationId: string;
  S3DestinationDescription?: {
    BucketARN?: string;
    Prefix?: string;
    ErrorOutputPrefix?: string;
    CompressionFormat?: string;
    BufferingHints?: { SizeInMBs: number; IntervalInSeconds: number };
  };
  ExtendedS3DestinationDescription?: {
    BucketARN?: string;
    Prefix?: string;
    ErrorOutputPrefix?: string;
    CompressionFormat?: string;
    BufferingHints?: { SizeInMBs: number; IntervalInSeconds: number };
  };
  RedshiftDestinationDescription?: { ClusterJDBCURL?: string };
  HttpEndpointDestinationDescription?: {
    EndpointConfiguration?: { Url?: string };
  };
  ElasticsearchDestinationDescription?: { DomainARN?: string };
  SplunkDestinationDescription?: { HECEndpoint?: string };
}

// ---------- Operations ----------

export async function listDeliveryStreams(): Promise<string[]> {
  const data = await request<{ DeliveryStreamNames?: string[] }>(
    "ListDeliveryStreams",
    {},
  );
  return data.DeliveryStreamNames ?? [];
}

export async function describeDeliveryStream(
  name: string,
): Promise<DeliveryStreamDetail> {
  const data = await request<{
    DeliveryStreamDescription?: {
      DeliveryStreamName: string;
      DeliveryStreamARN: string;
      DeliveryStreamStatus: string;
      DeliveryStreamType: string;
      VersionId: string;
      CreateTimestamp?: number;
      LastUpdateTimestamp?: number;
      Destinations?: DescribeDestinationApi[];
    };
  }>("DescribeDeliveryStream", { DeliveryStreamName: name });
  const desc = data.DeliveryStreamDescription;
  const destinations = desc?.Destinations ?? [];
  const det = detectDestination(destinations);
  return {
    name: desc?.DeliveryStreamName ?? name,
    arn: desc?.DeliveryStreamARN ?? "",
    status: desc?.DeliveryStreamStatus ?? "UNKNOWN",
    type: desc?.DeliveryStreamType ?? "DirectPut",
    destinationType: det.type,
    destinationDetail: det.detail,
    createTime: desc?.CreateTimestamp,
    lastUpdate: desc?.LastUpdateTimestamp,
    versionId: desc?.VersionId ?? "",
    destinations: destinations.map((d) => {
      const ext = d.ExtendedS3DestinationDescription;
      const s3 = d.S3DestinationDescription;
      const target = ext ?? s3;
      return {
        destinationId: d.DestinationId,
        type: ext ? "ExtendedS3" : s3 ? "S3" : "Unknown",
        bucketArn: target?.BucketARN,
        prefix: target?.Prefix,
        errorOutputPrefix: target?.ErrorOutputPrefix,
        compressionFormat: target?.CompressionFormat,
        bufferingHints: target?.BufferingHints
          ? {
              sizeInMBs: target.BufferingHints.SizeInMBs,
              intervalInSeconds: target.BufferingHints.IntervalInSeconds,
            }
          : undefined,
        endpointUrl:
          d.HttpEndpointDestinationDescription?.EndpointConfiguration?.Url,
      } satisfies DestinationDescription;
    }),
    raw: (desc as unknown as Record<string, unknown>) ?? {},
  };
}

export async function deleteDeliveryStream(name: string): Promise<void> {
  await request("DeleteDeliveryStream", { DeliveryStreamName: name });
}

export interface CreateDeliveryStreamInput {
  name: string;
  s3Destination: S3DestinationConfiguration;
}

export async function createDeliveryStream(
  input: CreateDeliveryStreamInput,
): Promise<{ arn: string }> {
  const data = await request<{ DeliveryStreamARN?: string }>(
    "CreateDeliveryStream",
    {
      DeliveryStreamName: input.name,
      DeliveryStreamType: "DirectPut",
      S3DestinationConfiguration: {
        BucketARN: input.s3Destination.bucketArn,
        RoleARN: input.s3Destination.roleArn,
        Prefix: input.s3Destination.prefix,
        ErrorOutputPrefix: input.s3Destination.errorOutputPrefix,
        BufferingHints: input.s3Destination.bufferingHints
          ? {
              SizeInMBs: input.s3Destination.bufferingHints.sizeInMBs,
              IntervalInSeconds:
                input.s3Destination.bufferingHints.intervalInSeconds,
            }
          : undefined,
        CompressionFormat:
          input.s3Destination.compressionFormat ?? "UNCOMPRESSED",
      },
    },
  );
  return { arn: data.DeliveryStreamARN ?? "" };
}

export async function putRecord(
  name: string,
  data: string,
): Promise<{ recordId: string }> {
  const res = await request<{ RecordId?: string }>("PutRecord", {
    DeliveryStreamName: name,
    Record: { Data: utf8ToBase64(data) },
  });
  return { recordId: res.RecordId ?? "" };
}

export async function putRecordBatch(
  name: string,
  records: string[],
): Promise<{ failedPutCount: number }> {
  const data = await request<{ FailedPutCount?: number }>("PutRecordBatch", {
    DeliveryStreamName: name,
    Records: records.map((r) => ({ Data: utf8ToBase64(r) })),
  });
  return { failedPutCount: data.FailedPutCount ?? 0 };
}
