/**
 * Route53 API client.
 *
 * Wraps the AWSim Route53 REST/XML API
 * (`/2013-04-01/...`) with strongly typed camel-cased shapes for hosted
 * zones, resource record sets, and health checks.
 */

import { ENDPOINT, amzDate, authHeader, loggedFetch } from "$lib/aws";

const SERVICE = "route53";
const BASE = `${ENDPOINT}/2013-04-01`;

// ---------- Types ----------

export interface HostedZone {
  id: string;
  name: string;
  callerReference: string;
  resourceRecordSetCount: number;
  comment?: string;
  privateZone: boolean;
}

export interface ResourceRecord {
  value: string;
}

export interface ResourceRecordSet {
  name: string;
  type: string;
  ttl: number;
  records: ResourceRecord[];
  setIdentifier?: string;
  aliasTarget?: { dnsName: string; hostedZoneId: string };
}

export interface HealthCheck {
  id: string;
  callerReference: string;
  type: string;
  resourcePath?: string;
  fullyQualifiedDomainName?: string;
  port?: number;
  ipAddress?: string;
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
    throw new Error(`Route53 ${action} failed (HTTP ${res.status}): ${text}`);
  }
  return text;
}

function tagValue(xml: string, tag: string): string {
  const match = xml.match(new RegExp(`<${tag}>([^<]*)</${tag}>`));
  return match ? match[1] : "";
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

// ---------- Hosted zones ----------

export async function listHostedZones(): Promise<HostedZone[]> {
  const xml = await request("GET", "/hostedzone", "ListHostedZones");
  return tagBlocks(xml, "HostedZone").map((block) => ({
    id: tagValue(block, "Id"),
    name: tagValue(block, "Name"),
    callerReference: tagValue(block, "CallerReference"),
    resourceRecordSetCount:
      Number(tagValue(block, "ResourceRecordSetCount")) || 0,
    comment: tagValue(block, "Comment") || undefined,
    privateZone: tagValue(block, "PrivateZone") === "true",
  }));
}

export interface HostedZoneDetail extends HostedZone {
  nameServers: string[];
}

export async function getHostedZone(id: string): Promise<HostedZoneDetail> {
  const cleanId = id.replace(/^\/?hostedzone\//, "");
  const xml = await request("GET", `/hostedzone/${cleanId}`, "GetHostedZone");
  const zoneBlock = xml.match(/<HostedZone>([\s\S]*?)<\/HostedZone>/);
  const block = zoneBlock ? zoneBlock[1] : xml;
  const nsBlock = xml.match(/<NameServers>([\s\S]*?)<\/NameServers>/);
  return {
    id: tagValue(block, "Id"),
    name: tagValue(block, "Name"),
    callerReference: tagValue(block, "CallerReference"),
    resourceRecordSetCount:
      Number(tagValue(block, "ResourceRecordSetCount")) || 0,
    comment: tagValue(block, "Comment") || undefined,
    privateZone: tagValue(block, "PrivateZone") === "true",
    nameServers: nsBlock
      ? tagBlocks(nsBlock[1], "NameServer").map((s) => s.trim())
      : [],
  };
}

export interface CreateHostedZoneInput {
  name: string;
  comment?: string;
  privateZone?: boolean;
}

export async function createHostedZone(
  input: CreateHostedZoneInput,
): Promise<{ id: string }> {
  const ref = `cli-${Date.now()}`;
  const body = `<?xml version="1.0" encoding="UTF-8"?>
<CreateHostedZoneRequest xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <Name>${escapeXml(input.name)}</Name>
  <CallerReference>${ref}</CallerReference>
  <HostedZoneConfig>
    <Comment>${escapeXml(input.comment ?? "")}</Comment>
    <PrivateZone>${input.privateZone ? "true" : "false"}</PrivateZone>
  </HostedZoneConfig>
</CreateHostedZoneRequest>`;
  const xml = await request("POST", "/hostedzone", "CreateHostedZone", body);
  return { id: tagValue(xml, "Id") };
}

export async function deleteHostedZone(id: string): Promise<void> {
  const cleanId = id.replace(/^\/?hostedzone\//, "");
  await request("DELETE", `/hostedzone/${cleanId}`, "DeleteHostedZone");
}

// ---------- Resource record sets ----------

export async function listResourceRecordSets(
  hostedZoneId: string,
): Promise<ResourceRecordSet[]> {
  const cleanId = hostedZoneId.replace(/^\/?hostedzone\//, "");
  const xml = await request(
    "GET",
    `/hostedzone/${cleanId}/rrset`,
    "ListResourceRecordSets",
  );
  return tagBlocks(xml, "ResourceRecordSet").map((block) => {
    const records: ResourceRecord[] = [];
    const rrBlock = block.match(
      /<ResourceRecords>([\s\S]*?)<\/ResourceRecords>/,
    );
    if (rrBlock) {
      tagBlocks(rrBlock[1], "Value").forEach((v) => records.push({ value: v }));
    }
    const aliasBlock = block.match(/<AliasTarget>([\s\S]*?)<\/AliasTarget>/);
    return {
      name: tagValue(block, "Name"),
      type: tagValue(block, "Type"),
      ttl: Number(tagValue(block, "TTL")) || 0,
      records,
      setIdentifier: tagValue(block, "SetIdentifier") || undefined,
      aliasTarget: aliasBlock
        ? {
            dnsName: tagValue(aliasBlock[1], "DNSName"),
            hostedZoneId: tagValue(aliasBlock[1], "HostedZoneId"),
          }
        : undefined,
    };
  });
}

export interface ChangeRecordInput {
  action: "CREATE" | "DELETE" | "UPSERT";
  name: string;
  type: string;
  ttl: number;
  values: string[];
}

export async function changeResourceRecordSets(
  hostedZoneId: string,
  changes: ChangeRecordInput[],
): Promise<void> {
  const cleanId = hostedZoneId.replace(/^\/?hostedzone\//, "");
  const changeXml = changes
    .map((c) => {
      const valuesXml = c.values
        .map(
          (v) =>
            `<ResourceRecord><Value>${escapeXml(v)}</Value></ResourceRecord>`,
        )
        .join("");
      return `<Change>
        <Action>${c.action}</Action>
        <ResourceRecordSet>
          <Name>${escapeXml(c.name)}</Name>
          <Type>${c.type}</Type>
          <TTL>${c.ttl}</TTL>
          <ResourceRecords>${valuesXml}</ResourceRecords>
        </ResourceRecordSet>
      </Change>`;
    })
    .join("");
  const body = `<?xml version="1.0" encoding="UTF-8"?>
<ChangeResourceRecordSetsRequest xmlns="https://route53.amazonaws.com/doc/2013-04-01/">
  <ChangeBatch><Changes>${changeXml}</Changes></ChangeBatch>
</ChangeResourceRecordSetsRequest>`;
  await request(
    "POST",
    `/hostedzone/${cleanId}/rrset`,
    "ChangeResourceRecordSets",
    body,
  );
}

// ---------- Health checks ----------

export async function listHealthChecks(): Promise<HealthCheck[]> {
  const xml = await request("GET", "/healthcheck", "ListHealthChecks");
  return tagBlocks(xml, "HealthCheck").map((block) => {
    const cfgBlock = block.match(
      /<HealthCheckConfig>([\s\S]*?)<\/HealthCheckConfig>/,
    );
    const cfg = cfgBlock ? cfgBlock[1] : "";
    return {
      id: tagValue(block, "Id"),
      callerReference: tagValue(block, "CallerReference"),
      type: tagValue(cfg, "Type"),
      resourcePath: tagValue(cfg, "ResourcePath") || undefined,
      fullyQualifiedDomainName:
        tagValue(cfg, "FullyQualifiedDomainName") || undefined,
      port: Number(tagValue(cfg, "Port")) || undefined,
      ipAddress: tagValue(cfg, "IPAddress") || undefined,
    };
  });
}

export async function getHealthCheck(id: string): Promise<HealthCheck> {
  const xml = await request("GET", `/healthcheck/${id}`, "GetHealthCheck");
  const cfgBlock = xml.match(
    /<HealthCheckConfig>([\s\S]*?)<\/HealthCheckConfig>/,
  );
  const cfg = cfgBlock ? cfgBlock[1] : "";
  return {
    id: tagValue(xml, "Id"),
    callerReference: tagValue(xml, "CallerReference"),
    type: tagValue(cfg, "Type"),
    resourcePath: tagValue(cfg, "ResourcePath") || undefined,
    fullyQualifiedDomainName:
      tagValue(cfg, "FullyQualifiedDomainName") || undefined,
    port: Number(tagValue(cfg, "Port")) || undefined,
    ipAddress: tagValue(cfg, "IPAddress") || undefined,
  };
}
