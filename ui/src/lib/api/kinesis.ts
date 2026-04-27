/**
 * Typed Kinesis Data Streams API client.
 */

import { ENDPOINT, amzDate, authHeader, loggedFetch } from "$lib/aws";

const SERVICE = "kinesis";
const TARGET_PREFIX = "Kinesis_20131202";

// ---------- Types ----------

export interface Stream {
  name: string;
  status: string;
  shardCount: number;
  retentionPeriodHours: number;
  arn: string;
  encryptionType: string;
}

export interface Shard {
  shardId: string;
  parentShardId?: string;
  startingHashKey: string;
  endingHashKey: string;
  startingSequenceNumber: string;
  endingSequenceNumber?: string;
}

export interface KinesisRecord {
  sequenceNumber: string;
  partitionKey: string;
  approximateArrivalTimestamp: number;
  data: string; // base64
}

export type ShardIteratorType =
  | "TRIM_HORIZON"
  | "LATEST"
  | "AT_SEQUENCE_NUMBER"
  | "AFTER_SEQUENCE_NUMBER"
  | "AT_TIMESTAMP";

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
      `Kinesis ${action} failed (HTTP ${res.status}): ${await res.text()}`,
    );
  }
  const text = await res.text();
  return (text ? JSON.parse(text) : {}) as T;
}

// ---------- Operations ----------

export async function listStreams(): Promise<string[]> {
  const data = await request<{ StreamNames?: string[] }>("ListStreams");
  return data.StreamNames ?? [];
}

export async function describeStream(
  name: string,
): Promise<{ stream: Stream; shards: Shard[] }> {
  const data = await request<{
    StreamDescription?: {
      StreamName: string;
      StreamARN: string;
      StreamStatus: string;
      RetentionPeriodHours: number;
      EncryptionType?: string;
      Shards: {
        ShardId: string;
        ParentShardId?: string;
        HashKeyRange: {
          StartingHashKey: string;
          EndingHashKey: string;
        };
        SequenceNumberRange: {
          StartingSequenceNumber: string;
          EndingSequenceNumber?: string;
        };
      }[];
    };
  }>("DescribeStream", { StreamName: name });
  const desc = data.StreamDescription;
  return {
    stream: {
      name: desc?.StreamName ?? name,
      status: desc?.StreamStatus ?? "",
      shardCount: desc?.Shards?.length ?? 0,
      retentionPeriodHours: desc?.RetentionPeriodHours ?? 24,
      arn: desc?.StreamARN ?? "",
      encryptionType: desc?.EncryptionType ?? "NONE",
    },
    shards: (desc?.Shards ?? []).map((s) => ({
      shardId: s.ShardId,
      parentShardId: s.ParentShardId,
      startingHashKey: s.HashKeyRange.StartingHashKey,
      endingHashKey: s.HashKeyRange.EndingHashKey,
      startingSequenceNumber: s.SequenceNumberRange.StartingSequenceNumber,
      endingSequenceNumber: s.SequenceNumberRange.EndingSequenceNumber,
    })),
  };
}

export async function createStream(
  name: string,
  shardCount: number,
): Promise<void> {
  await request("CreateStream", { StreamName: name, ShardCount: shardCount });
}

export async function deleteStream(name: string): Promise<void> {
  await request("DeleteStream", { StreamName: name });
}

function utf8ToBase64(value: string): string {
  return btoa(unescape(encodeURIComponent(value)));
}

export async function putRecord(
  streamName: string,
  data: string,
  partitionKey: string,
): Promise<{ shardId: string; sequenceNumber: string }> {
  const res = await request<{ ShardId: string; SequenceNumber: string }>(
    "PutRecord",
    {
      StreamName: streamName,
      Data: utf8ToBase64(data),
      PartitionKey: partitionKey,
    },
  );
  return { shardId: res.ShardId, sequenceNumber: res.SequenceNumber };
}

export interface PutRecordsEntry {
  data: string;
  partitionKey: string;
}

export async function putRecords(
  streamName: string,
  records: PutRecordsEntry[],
): Promise<{ failedRecordCount: number }> {
  const data = await request<{ FailedRecordCount?: number }>("PutRecords", {
    StreamName: streamName,
    Records: records.map((r) => ({
      Data: utf8ToBase64(r.data),
      PartitionKey: r.partitionKey,
    })),
  });
  return { failedRecordCount: data.FailedRecordCount ?? 0 };
}

export async function getShardIterator(
  streamName: string,
  shardId: string,
  type: ShardIteratorType = "TRIM_HORIZON",
  startingSequenceNumber?: string,
): Promise<string> {
  const params: Record<string, unknown> = {
    StreamName: streamName,
    ShardId: shardId,
    ShardIteratorType: type,
  };
  if (startingSequenceNumber)
    params["StartingSequenceNumber"] = startingSequenceNumber;
  const data = await request<{ ShardIterator?: string }>(
    "GetShardIterator",
    params,
  );
  return data.ShardIterator ?? "";
}

export async function getRecords(
  shardIterator: string,
  limit = 100,
): Promise<{
  records: KinesisRecord[];
  nextShardIterator?: string;
  millisBehindLatest?: number;
}> {
  const data = await request<{
    Records?: {
      SequenceNumber: string;
      PartitionKey: string;
      ApproximateArrivalTimestamp: number;
      Data: string;
    }[];
    NextShardIterator?: string;
    MillisBehindLatest?: number;
  }>("GetRecords", { ShardIterator: shardIterator, Limit: limit });
  return {
    records: (data.Records ?? []).map((r) => ({
      sequenceNumber: r.SequenceNumber,
      partitionKey: r.PartitionKey,
      approximateArrivalTimestamp: r.ApproximateArrivalTimestamp,
      data: r.Data,
    })),
    nextShardIterator: data.NextShardIterator,
    millisBehindLatest: data.MillisBehindLatest,
  };
}

/** Decode base64 record data into utf-8 string (best effort). */
export function decodeRecordData(b64: string): string {
  try {
    return decodeURIComponent(escape(atob(b64)));
  } catch {
    return b64;
  }
}
