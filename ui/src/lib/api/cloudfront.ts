/**
 * CloudFront API client.
 *
 * Wraps the LocalStack-compatible CloudFront REST/XML API
 * (`/2020-05-31/...`) with strongly typed camel-cased shapes for
 * distributions, origin access identities, cache and origin-request
 * policies, key groups, public keys, and functions.
 */

import { ENDPOINT, amzDate, authHeader, loggedFetch } from "$lib/aws";

const SERVICE = "cloudfront";
const BASE = `${ENDPOINT}/2020-05-31`;

// ---------- Types ----------

export interface Distribution {
  id: string;
  arn: string;
  status: string;
  domainName: string;
  comment: string;
  enabled: boolean;
  lastModifiedTime: string;
  priceClass: string;
  httpVersion: string;
  originDomainName: string;
}

export interface OriginAccessIdentity {
  id: string;
  s3CanonicalUserId: string;
  comment: string;
}

export interface CachePolicy {
  id: string;
  name: string;
  comment: string;
  type: string;
  defaultTtl: number;
  maxTtl: number;
  minTtl: number;
  lastModifiedTime: string;
}

export interface OriginRequestPolicy {
  id: string;
  name: string;
  comment: string;
  type: string;
  lastModifiedTime: string;
}

export interface KeyGroup {
  id: string;
  name: string;
  comment: string;
  publicKeyIds: string[];
  lastModifiedTime: string;
}

export interface PublicKey {
  id: string;
  name: string;
  comment: string;
  encodedKey: string;
  createdTime: string;
}

export interface CloudFrontFunction {
  name: string;
  status: string;
  comment: string;
  runtime: string;
  stage: string;
  lastModifiedTime: string;
}

// ---------- Internals ----------

function headers(): Record<string, string> {
  return {
    Authorization: authHeader(SERVICE),
    "X-Amz-Date": amzDate(),
    "Content-Type": "application/xml",
  };
}

async function request(
  method: string,
  path: string,
  action: string,
  body?: string,
): Promise<string> {
  const res = await loggedFetch(SERVICE, action, method, `${BASE}${path}`, {
    method,
    headers: headers(),
    body,
  });
  const text = await res.text();
  if (!res.ok) {
    throw new Error(
      `CloudFront ${action} failed (HTTP ${res.status}): ${text}`,
    );
  }
  return text;
}

function tagValue(xml: string, tag: string): string {
  const match = xml.match(new RegExp(`<${tag}>([\\s\\S]*?)</${tag}>`));
  return match ? match[1].trim() : "";
}

function tagBlocks(xml: string, tag: string): string[] {
  const out: string[] = [];
  const regex = new RegExp(`<${tag}>([\\s\\S]*?)</${tag}>`, "g");
  let m;
  while ((m = regex.exec(xml)) !== null) out.push(m[1]);
  return out;
}

function escapeXml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

// ---------- Distributions ----------

export async function listDistributions(): Promise<Distribution[]> {
  const xml = await request("GET", "/distribution", "ListDistributions");
  return tagBlocks(xml, "DistributionSummary").map((block) => {
    // The first Origin's DomainName as a hint
    const originBlock = block.match(/<Origin>([\s\S]*?)<\/Origin>/);
    return {
      id: tagValue(block, "Id"),
      arn: tagValue(block, "ARN"),
      status: tagValue(block, "Status"),
      domainName: tagValue(block, "DomainName"),
      comment: tagValue(block, "Comment"),
      enabled: tagValue(block, "Enabled") === "true",
      lastModifiedTime: tagValue(block, "LastModifiedTime"),
      priceClass: tagValue(block, "PriceClass"),
      httpVersion: tagValue(block, "HttpVersion"),
      originDomainName: originBlock
        ? tagValue(originBlock[1], "DomainName")
        : "",
    };
  });
}

export interface DistributionDetail extends Distribution {
  callerReference: string;
  defaultRootObject: string;
}

export async function getDistribution(id: string): Promise<DistributionDetail> {
  const xml = await request("GET", `/distribution/${id}`, "GetDistribution");
  const summary = xml.match(/<Distribution>([\s\S]*?)<\/Distribution>/);
  const block = summary ? summary[1] : xml;
  const originBlock = xml.match(/<Origin>([\s\S]*?)<\/Origin>/);
  return {
    id: tagValue(block, "Id"),
    arn: tagValue(block, "ARN"),
    status: tagValue(block, "Status"),
    domainName: tagValue(block, "DomainName"),
    comment: tagValue(xml, "Comment"),
    enabled: tagValue(xml, "Enabled") === "true",
    lastModifiedTime: tagValue(block, "LastModifiedTime"),
    priceClass: tagValue(xml, "PriceClass"),
    httpVersion: tagValue(xml, "HttpVersion"),
    originDomainName: originBlock ? tagValue(originBlock[1], "DomainName") : "",
    callerReference: tagValue(xml, "CallerReference"),
    defaultRootObject: tagValue(xml, "DefaultRootObject"),
  };
}

export interface CreateDistributionInput {
  originDomain: string;
  comment?: string;
  enabled?: boolean;
}

export async function createDistribution(
  input: CreateDistributionInput,
): Promise<{ id: string }> {
  const ref = `cli-${Date.now()}`;
  const body = `<?xml version="1.0" encoding="UTF-8"?>
<DistributionConfig xmlns="http://cloudfront.amazonaws.com/doc/2020-05-31/">
  <CallerReference>${ref}</CallerReference>
  <Comment>${escapeXml(input.comment ?? "")}</Comment>
  <Enabled>${input.enabled === false ? "false" : "true"}</Enabled>
  <Origins>
    <Quantity>1</Quantity>
    <Items>
      <Origin>
        <Id>origin-1</Id>
        <DomainName>${escapeXml(input.originDomain)}</DomainName>
        <CustomOriginConfig>
          <HTTPPort>80</HTTPPort>
          <HTTPSPort>443</HTTPSPort>
          <OriginProtocolPolicy>https-only</OriginProtocolPolicy>
        </CustomOriginConfig>
      </Origin>
    </Items>
  </Origins>
  <DefaultCacheBehavior>
    <TargetOriginId>origin-1</TargetOriginId>
    <ViewerProtocolPolicy>redirect-to-https</ViewerProtocolPolicy>
    <ForwardedValues>
      <QueryString>false</QueryString>
      <Cookies><Forward>none</Forward></Cookies>
    </ForwardedValues>
    <MinTTL>0</MinTTL>
  </DefaultCacheBehavior>
</DistributionConfig>`;

  const xml = await request(
    "POST",
    "/distribution",
    "CreateDistribution",
    body,
  );
  return { id: tagValue(xml, "Id") };
}

// ---------- Origin access identities ----------

export async function listOriginAccessIdentities(): Promise<
  OriginAccessIdentity[]
> {
  const xml = await request(
    "GET",
    "/origin-access-identity/cloudfront",
    "ListCloudFrontOriginAccessIdentities",
  );
  return tagBlocks(xml, "CloudFrontOriginAccessIdentitySummary").map(
    (block) => ({
      id: tagValue(block, "Id"),
      s3CanonicalUserId: tagValue(block, "S3CanonicalUserId"),
      comment: tagValue(block, "Comment"),
    }),
  );
}

// ---------- Cache policies ----------

export async function listCachePolicies(): Promise<CachePolicy[]> {
  const xml = await request("GET", "/cache-policy", "ListCachePolicies");
  return tagBlocks(xml, "CachePolicySummary").map((block) => {
    const cpBlock = block.match(/<CachePolicy>([\s\S]*?)<\/CachePolicy>/);
    const cp = cpBlock ? cpBlock[1] : block;
    const cfgBlock = cp.match(
      /<CachePolicyConfig>([\s\S]*?)<\/CachePolicyConfig>/,
    );
    const cfg = cfgBlock ? cfgBlock[1] : cp;
    return {
      id: tagValue(cp, "Id"),
      name: tagValue(cfg, "Name"),
      comment: tagValue(cfg, "Comment"),
      type: tagValue(block, "Type"),
      defaultTtl: Number(tagValue(cfg, "DefaultTTL")) || 0,
      maxTtl: Number(tagValue(cfg, "MaxTTL")) || 0,
      minTtl: Number(tagValue(cfg, "MinTTL")) || 0,
      lastModifiedTime: tagValue(cp, "LastModifiedTime"),
    };
  });
}

// ---------- Origin request policies ----------

export async function listOriginRequestPolicies(): Promise<
  OriginRequestPolicy[]
> {
  const xml = await request(
    "GET",
    "/origin-request-policy",
    "ListOriginRequestPolicies",
  );
  return tagBlocks(xml, "OriginRequestPolicySummary").map((block) => {
    const prBlock = block.match(
      /<OriginRequestPolicy>([\s\S]*?)<\/OriginRequestPolicy>/,
    );
    const pr = prBlock ? prBlock[1] : block;
    const cfgBlock = pr.match(
      /<OriginRequestPolicyConfig>([\s\S]*?)<\/OriginRequestPolicyConfig>/,
    );
    const cfg = cfgBlock ? cfgBlock[1] : pr;
    return {
      id: tagValue(pr, "Id"),
      name: tagValue(cfg, "Name"),
      comment: tagValue(cfg, "Comment"),
      type: tagValue(block, "Type"),
      lastModifiedTime: tagValue(pr, "LastModifiedTime"),
    };
  });
}

// ---------- Key groups ----------

export async function listKeyGroups(): Promise<KeyGroup[]> {
  const xml = await request("GET", "/key-group", "ListKeyGroups");
  return tagBlocks(xml, "KeyGroupSummary").map((block) => {
    const kgBlock = block.match(/<KeyGroup>([\s\S]*?)<\/KeyGroup>/);
    const kg = kgBlock ? kgBlock[1] : block;
    const cfgBlock = kg.match(/<KeyGroupConfig>([\s\S]*?)<\/KeyGroupConfig>/);
    const cfg = cfgBlock ? cfgBlock[1] : kg;
    const itemsBlock = cfg.match(/<Items>([\s\S]*?)<\/Items>/);
    const ids = itemsBlock
      ? tagBlocks(itemsBlock[1], "PublicKey").map((s) => s.trim())
      : [];
    return {
      id: tagValue(kg, "Id"),
      name: tagValue(cfg, "Name"),
      comment: tagValue(cfg, "Comment"),
      publicKeyIds: ids,
      lastModifiedTime: tagValue(kg, "LastModifiedTime"),
    };
  });
}

// ---------- Public keys ----------

export async function listPublicKeys(): Promise<PublicKey[]> {
  const xml = await request("GET", "/public-key", "ListPublicKeys");
  return tagBlocks(xml, "PublicKeySummary").map((block) => ({
    id: tagValue(block, "Id"),
    name: tagValue(block, "Name"),
    comment: tagValue(block, "Comment"),
    encodedKey: tagValue(block, "EncodedKey"),
    createdTime: tagValue(block, "CreatedTime"),
  }));
}

// ---------- Functions ----------

export async function listFunctions(): Promise<CloudFrontFunction[]> {
  const xml = await request("GET", "/function", "ListFunctions");
  return tagBlocks(xml, "FunctionSummary").map((block) => {
    const cfgBlock = block.match(
      /<FunctionConfig>([\s\S]*?)<\/FunctionConfig>/,
    );
    const cfg = cfgBlock ? cfgBlock[1] : block;
    const metaBlock = block.match(
      /<FunctionMetadata>([\s\S]*?)<\/FunctionMetadata>/,
    );
    const meta = metaBlock ? metaBlock[1] : block;
    return {
      name: tagValue(block, "Name"),
      status: tagValue(block, "Status"),
      comment: tagValue(cfg, "Comment"),
      runtime: tagValue(cfg, "Runtime"),
      stage: tagValue(meta, "Stage"),
      lastModifiedTime: tagValue(meta, "LastModifiedTime"),
    };
  });
}
