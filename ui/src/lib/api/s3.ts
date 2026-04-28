/**
 * Typed S3 API client.
 *
 * Wraps AWSim's S3 REST API with strong TypeScript types so
 * callers in components never have to think about XML or fetch headers.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

export interface Bucket {
  name: string;
  creationDate: string;
  region?: string;
}

export interface S3Object {
  key: string;
  size: number;
  lastModified: string;
  storageClass: string;
  etag: string;
}

export interface S3CommonPrefix {
  prefix: string;
}

export interface ListObjectsResult {
  objects: S3Object[];
  commonPrefixes: S3CommonPrefix[];
  isTruncated: boolean;
  nextContinuationToken?: string;
}

export interface BucketPolicy {
  policy: string;
}

export interface ObjectMetadata {
  contentType?: string;
  contentLength?: number;
  lastModified?: string;
  etag?: string;
  versionId?: string;
  metadata: Record<string, string>;
}

function s3Headers(): Record<string, string> {
  return {
    Authorization: authHeader("s3"),
    "X-Amz-Date": amzDate(),
  };
}

function encodeKey(key: string): string {
  return key.split("/").map(encodeURIComponent).join("/");
}

function xmlText(xml: string, tag: string): string | undefined {
  const m = new RegExp(`<${tag}>([\\s\\S]*?)<\\/${tag}>`).exec(xml);
  return m ? m[1] : undefined;
}

export async function listBuckets(): Promise<Bucket[]> {
  const res = await loggedFetch("s3", "ListBuckets", "GET", `${ENDPOINT}/`, {
    headers: s3Headers(),
  });
  if (!res.ok) throw new Error(`ListBuckets failed: HTTP ${res.status}`);
  const xml = await res.text();
  const buckets: Bucket[] = [];
  const regex =
    /<Bucket>\s*<Name>([^<]+)<\/Name>\s*<CreationDate>([^<]+)<\/CreationDate>\s*<\/Bucket>/g;
  let match: RegExpExecArray | null;
  while ((match = regex.exec(xml)) !== null) {
    buckets.push({ name: match[1], creationDate: match[2] });
  }
  return buckets;
}

export async function createBucket(name: string): Promise<void> {
  const res = await loggedFetch(
    "s3",
    "CreateBucket",
    "PUT",
    `${ENDPOINT}/${encodeURIComponent(name)}`,
    {
      method: "PUT",
      headers: s3Headers(),
    },
  );
  if (!res.ok)
    throw new Error(
      `CreateBucket failed: HTTP ${res.status}: ${await res.text()}`,
    );
}

export async function deleteBucket(name: string): Promise<void> {
  const res = await loggedFetch(
    "s3",
    "DeleteBucket",
    "DELETE",
    `${ENDPOINT}/${encodeURIComponent(name)}`,
    {
      method: "DELETE",
      headers: s3Headers(),
    },
  );
  if (!res.ok)
    throw new Error(
      `DeleteBucket failed: HTTP ${res.status}: ${await res.text()}`,
    );
}

export async function listObjects(
  bucket: string,
  prefix = "",
  delimiter = "/",
): Promise<ListObjectsResult> {
  const params = new URLSearchParams({ "list-type": "2" });
  if (prefix) params.set("prefix", prefix);
  if (delimiter !== undefined) params.set("delimiter", delimiter);

  const res = await loggedFetch(
    "s3",
    "ListObjectsV2",
    "GET",
    `${ENDPOINT}/${encodeURIComponent(bucket)}?${params.toString()}`,
    { headers: s3Headers() },
  );
  if (!res.ok)
    throw new Error(
      `ListObjectsV2 failed: HTTP ${res.status}: ${await res.text()}`,
    );
  const xml = await res.text();

  const objects: S3Object[] = [];
  const contentRegex = /<Contents>([\s\S]*?)<\/Contents>/g;
  let match: RegExpExecArray | null;
  while ((match = contentRegex.exec(xml)) !== null) {
    const block = match[1];
    objects.push({
      key: xmlText(block, "Key") ?? "",
      size: parseInt(xmlText(block, "Size") ?? "0", 10),
      lastModified: xmlText(block, "LastModified") ?? "",
      storageClass: xmlText(block, "StorageClass") ?? "STANDARD",
      etag: (xmlText(block, "ETag") ?? "").replace(/&quot;/g, '"'),
    });
  }

  const commonPrefixes: S3CommonPrefix[] = [];
  const prefixRegex =
    /<CommonPrefixes>\s*<Prefix>([^<]+)<\/Prefix>\s*<\/CommonPrefixes>/g;
  while ((match = prefixRegex.exec(xml)) !== null) {
    commonPrefixes.push({ prefix: match[1] });
  }

  const isTruncated = /<IsTruncated>true<\/IsTruncated>/i.test(xml);
  const tokenMatch =
    /<NextContinuationToken>([^<]+)<\/NextContinuationToken>/.exec(xml);

  return {
    objects,
    commonPrefixes,
    isTruncated,
    nextContinuationToken: tokenMatch ? tokenMatch[1] : undefined,
  };
}

export async function putObject(
  bucket: string,
  key: string,
  body: Blob | ArrayBuffer | string,
  contentType?: string,
): Promise<void> {
  const headers = s3Headers();
  if (contentType) headers["Content-Type"] = contentType;
  const res = await loggedFetch(
    "s3",
    "PutObject",
    "PUT",
    `${ENDPOINT}/${encodeURIComponent(bucket)}/${encodeKey(key)}`,
    {
      method: "PUT",
      headers,
      body,
    },
  );
  if (!res.ok)
    throw new Error(
      `PutObject failed: HTTP ${res.status}: ${await res.text()}`,
    );
}

export async function deleteObject(bucket: string, key: string): Promise<void> {
  const res = await loggedFetch(
    "s3",
    "DeleteObject",
    "DELETE",
    `${ENDPOINT}/${encodeURIComponent(bucket)}/${encodeKey(key)}`,
    {
      method: "DELETE",
      headers: s3Headers(),
    },
  );
  if (!res.ok)
    throw new Error(
      `DeleteObject failed: HTTP ${res.status}: ${await res.text()}`,
    );
}

export async function headObject(
  bucket: string,
  key: string,
): Promise<ObjectMetadata> {
  const res = await loggedFetch(
    "s3",
    "HeadObject",
    "HEAD",
    `${ENDPOINT}/${encodeURIComponent(bucket)}/${encodeKey(key)}`,
    {
      method: "HEAD",
      headers: s3Headers(),
    },
  );
  if (!res.ok) throw new Error(`HeadObject failed: HTTP ${res.status}`);

  const metadata: Record<string, string> = {};
  res.headers.forEach((value, name) => {
    if (name.toLowerCase().startsWith("x-amz-meta-")) {
      metadata[name.slice("x-amz-meta-".length)] = value;
    }
  });

  const contentLengthHeader = res.headers.get("content-length");
  return {
    contentType: res.headers.get("content-type") ?? undefined,
    contentLength: contentLengthHeader
      ? parseInt(contentLengthHeader, 10)
      : undefined,
    lastModified: res.headers.get("last-modified") ?? undefined,
    etag: res.headers.get("etag") ?? undefined,
    versionId: res.headers.get("x-amz-version-id") ?? undefined,
    metadata,
  };
}

export async function getObjectBlob(
  bucket: string,
  key: string,
): Promise<Blob> {
  const res = await loggedFetch(
    "s3",
    "GetObject",
    "GET",
    `${ENDPOINT}/${encodeURIComponent(bucket)}/${encodeKey(key)}`,
    { headers: s3Headers() },
  );
  if (!res.ok)
    throw new Error(
      `GetObject failed: HTTP ${res.status}: ${await res.text()}`,
    );
  return res.blob();
}

export async function getObjectText(
  bucket: string,
  key: string,
): Promise<string> {
  const res = await loggedFetch(
    "s3",
    "GetObject",
    "GET",
    `${ENDPOINT}/${encodeURIComponent(bucket)}/${encodeKey(key)}`,
    { headers: s3Headers() },
  );
  if (!res.ok)
    throw new Error(
      `GetObject failed: HTTP ${res.status}: ${await res.text()}`,
    );
  return res.text();
}

export async function getBucketPolicy(bucket: string): Promise<BucketPolicy> {
  const res = await loggedFetch(
    "s3",
    "GetBucketPolicy",
    "GET",
    `${ENDPOINT}/${encodeURIComponent(bucket)}?policy`,
    { headers: s3Headers() },
  );
  if (res.status === 404) return { policy: "" };
  if (!res.ok)
    throw new Error(
      `GetBucketPolicy failed: HTTP ${res.status}: ${await res.text()}`,
    );
  const text = await res.text();
  // GetBucketPolicy returns the policy JSON directly in the response body.
  return { policy: text };
}

export async function putBucketPolicy(
  bucket: string,
  policy: string,
): Promise<void> {
  const headers = s3Headers();
  headers["Content-Type"] = "application/json";
  const res = await loggedFetch(
    "s3",
    "PutBucketPolicy",
    "PUT",
    `${ENDPOINT}/${encodeURIComponent(bucket)}?policy`,
    {
      method: "PUT",
      headers,
      body: policy,
    },
  );
  if (!res.ok)
    throw new Error(
      `PutBucketPolicy failed: HTTP ${res.status}: ${await res.text()}`,
    );
}

export async function deleteBucketPolicy(bucket: string): Promise<void> {
  const res = await loggedFetch(
    "s3",
    "DeleteBucketPolicy",
    "DELETE",
    `${ENDPOINT}/${encodeURIComponent(bucket)}?policy`,
    {
      method: "DELETE",
      headers: s3Headers(),
    },
  );
  if (!res.ok && res.status !== 404)
    throw new Error(
      `DeleteBucketPolicy failed: HTTP ${res.status}: ${await res.text()}`,
    );
}

export function objectUrl(bucket: string, key: string): string {
  return `${ENDPOINT}/${encodeURIComponent(bucket)}/${encodeKey(key)}`;
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const k = 1024;
  const i = Math.min(
    units.length - 1,
    Math.floor(Math.log(bytes) / Math.log(k)),
  );
  const value = bytes / Math.pow(k, i);
  const rounded =
    value >= 100 ? Math.round(value) : Math.round(value * 10) / 10;
  return `${rounded} ${units[i]}`;
}

export function formatTimestamp(iso: string): string {
  if (!iso) return "—";
  try {
    return new Date(iso).toLocaleString();
  } catch {
    return iso;
  }
}
