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
  continuationToken?: string,
): Promise<ListObjectsResult> {
  const params = new URLSearchParams({ "list-type": "2" });
  if (prefix) params.set("prefix", prefix);
  if (delimiter !== undefined) params.set("delimiter", delimiter);
  if (continuationToken) params.set("continuation-token", continuationToken);

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

export interface CorsRule {
  AllowedHeaders?: string[];
  AllowedMethods: string[];
  AllowedOrigins: string[];
  ExposeHeaders?: string[];
  MaxAgeSeconds?: number;
}

export async function getBucketCors(
  bucket: string,
): Promise<CorsRule[]> {
  const res = await loggedFetch(
    "s3",
    "GetBucketCors",
    "GET",
    `${ENDPOINT}/${encodeURIComponent(bucket)}?cors`,
    { headers: s3Headers() },
  );
  if (res.status === 404) return [];
  if (!res.ok)
    throw new Error(
      `GetBucketCors failed: HTTP ${res.status}: ${await res.text()}`,
    );
  const text = await res.text();
  try {
    const parser = new DOMParser();
    const doc = parser.parseFromString(text, "text/xml");
    const rules: CorsRule[] = [];
    doc.querySelectorAll("CORSRule").forEach((el) => {
      const rule: CorsRule = {
        AllowedMethods: [],
        AllowedOrigins: [],
      };
      el.querySelectorAll("AllowedMethod").forEach((m) => rule.AllowedMethods.push(m.textContent ?? ""));
      el.querySelectorAll("AllowedOrigin").forEach((o) => rule.AllowedOrigins.push(o.textContent ?? ""));
      el.querySelectorAll("AllowedHeader").forEach((h) => {
        if (!rule.AllowedHeaders) rule.AllowedHeaders = [];
        rule.AllowedHeaders.push(h.textContent ?? "");
      });
      el.querySelectorAll("ExposeHeader").forEach((h) => {
        if (!rule.ExposeHeaders) rule.ExposeHeaders = [];
        rule.ExposeHeaders.push(h.textContent ?? "");
      });
      const maxAge = el.querySelector("MaxAgeSeconds")?.textContent;
      if (maxAge) rule.MaxAgeSeconds = parseInt(maxAge, 10);
      rules.push(rule);
    });
    return rules;
  } catch {
    return [];
  }
}

export async function putBucketCors(
  bucket: string,
  rules: CorsRule[],
): Promise<void> {
  let xml = '<CORSConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/">';
  for (const rule of rules) {
    xml += "<CORSRule>";
    for (const m of rule.AllowedMethods) xml += `<AllowedMethod>${m}</AllowedMethod>`;
    for (const o of rule.AllowedOrigins) xml += `<AllowedOrigin>${o}</AllowedOrigin>`;
    for (const h of rule.AllowedHeaders ?? []) xml += `<AllowedHeader>${h}</AllowedHeader>`;
    for (const h of rule.ExposeHeaders ?? []) xml += `<ExposeHeader>${h}</ExposeHeader>`;
    if (rule.MaxAgeSeconds != null) xml += `<MaxAgeSeconds>${rule.MaxAgeSeconds}</MaxAgeSeconds>`;
    xml += "</CORSRule>";
  }
  xml += "</CORSConfiguration>";
  const headers = s3Headers();
  headers["Content-Type"] = "application/xml";
  const res = await loggedFetch(
    "s3",
    "PutBucketCors",
    "PUT",
    `${ENDPOINT}/${encodeURIComponent(bucket)}?cors`,
    { method: "PUT", headers, body: xml },
  );
  if (!res.ok)
    throw new Error(
      `PutBucketCors failed: HTTP ${res.status}: ${await res.text()}`,
    );
}

export async function deleteBucketCors(bucket: string): Promise<void> {
  const res = await loggedFetch(
    "s3",
    "DeleteBucketCors",
    "DELETE",
    `${ENDPOINT}/${encodeURIComponent(bucket)}?cors`,
    { method: "DELETE", headers: s3Headers() },
  );
  if (!res.ok && res.status !== 404)
    throw new Error(
      `DeleteBucketCors failed: HTTP ${res.status}: ${await res.text()}`,
    );
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

// ── Bucket configuration APIs ──

export interface VersioningConfig {
  status: "Enabled" | "Suspended" | "";
}

export async function getBucketVersioning(
  bucket: string,
): Promise<VersioningConfig> {
  const res = await loggedFetch(
    "s3",
    "GetBucketVersioning",
    "GET",
    `${ENDPOINT}/${encodeURIComponent(bucket)}?versioning`,
    { headers: s3Headers() },
  );
  if (!res.ok) return { status: "" };
  const text = await res.text();
  const enabled = /<Status>Enabled<\/Status>/.test(text);
  const suspended = /<Status>Suspended<\/Status>/.test(text);
  return { status: enabled ? "Enabled" : suspended ? "Suspended" : "" };
}

export async function putBucketVersioning(
  bucket: string,
  status: "Enabled" | "Suspended",
): Promise<void> {
  const xml = `<VersioningConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Status>${status}</Status></VersioningConfiguration>`;
  const headers = s3Headers();
  headers["Content-Type"] = "application/xml";
  const res = await loggedFetch(
    "s3",
    "PutBucketVersioning",
    "PUT",
    `${ENDPOINT}/${encodeURIComponent(bucket)}?versioning`,
    { method: "PUT", headers, body: xml },
  );
  if (!res.ok)
    throw new Error(
      `PutBucketVersioning failed: HTTP ${res.status}: ${await res.text()}`,
    );
}

export interface EncryptionConfig {
  enabled: boolean;
  algorithm: string;
}

export async function getBucketEncryption(
  bucket: string,
): Promise<EncryptionConfig> {
  const res = await loggedFetch(
    "s3",
    "GetBucketEncryption",
    "GET",
    `${ENDPOINT}/${encodeURIComponent(bucket)}?encryption`,
    { headers: s3Headers() },
  );
  if (res.status === 404 || res.status === 204) return { enabled: false, algorithm: "" };
  if (!res.ok) return { enabled: false, algorithm: "" };
  const text = await res.text();
  const algo = /<Algorithm>([^<]+)<\/Algorithm>/.exec(text)?.[1] ?? "";
  return { enabled: true, algorithm: algo };
}

export async function putBucketEncryption(
  bucket: string,
  algorithm = "AES256",
): Promise<void> {
  const xml = `<ServerSideEncryptionConfiguration xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Rule><ApplyServerSideEncryptionByDefault><Algorithm>${algorithm}</Algorithm></ApplyServerSideEncryptionByDefault></Rule></ServerSideEncryptionConfiguration>`;
  const headers = s3Headers();
  headers["Content-Type"] = "application/xml";
  const res = await loggedFetch(
    "s3",
    "PutBucketEncryption",
    "PUT",
    `${ENDPOINT}/${encodeURIComponent(bucket)}?encryption`,
    { method: "PUT", headers, body: xml },
  );
  if (!res.ok)
    throw new Error(
      `PutBucketEncryption failed: HTTP ${res.status}: ${await res.text()}`,
    );
}

export async function deleteBucketEncryption(bucket: string): Promise<void> {
  const res = await loggedFetch(
    "s3",
    "DeleteBucketEncryption",
    "DELETE",
    `${ENDPOINT}/${encodeURIComponent(bucket)}?encryption`,
    { method: "DELETE", headers: s3Headers() },
  );
  if (!res.ok && res.status !== 404)
    throw new Error(
      `DeleteBucketEncryption failed: HTTP ${res.status}: ${await res.text()}`,
    );
}

export interface BucketTag {
  key: string;
  value: string;
}

export async function getBucketTagging(bucket: string): Promise<BucketTag[]> {
  const res = await loggedFetch(
    "s3",
    "GetBucketTagging",
    "GET",
    `${ENDPOINT}/${encodeURIComponent(bucket)}?tagging`,
    { headers: s3Headers() },
  );
  if (res.status === 404) return [];
  if (!res.ok) return [];
  const text = await res.text();
  const tags: BucketTag[] = [];
  const regex = /<Tag>\s*<Key>([^<]+)<\/Key>\s*<Value>([^<]*)<\/Value>\s*<\/Tag>/g;
  let m: RegExpExecArray | null;
  while ((m = regex.exec(text)) !== null) {
    tags.push({ key: m[1], value: m[2] });
  }
  return tags;
}

export async function putBucketTagging(
  bucket: string,
  tags: BucketTag[],
): Promise<void> {
  let xml = '<Tagging xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><TagSet>';
  for (const t of tags) {
    xml += `<Tag><Key>${t.key}</Key><Value>${t.value}</Value></Tag>`;
  }
  xml += "</TagSet></Tagging>";
  const headers = s3Headers();
  headers["Content-Type"] = "application/xml";
  const res = await loggedFetch(
    "s3",
    "PutBucketTagging",
    "PUT",
    `${ENDPOINT}/${encodeURIComponent(bucket)}?tagging`,
    { method: "PUT", headers, body: xml },
  );
  if (!res.ok)
    throw new Error(
      `PutBucketTagging failed: HTTP ${res.status}: ${await res.text()}`,
    );
}
