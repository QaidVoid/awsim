/**
 * EC2 API client.
 *
 * Wraps the LocalStack-compatible EC2 query API
 * (`Action=...&Version=2016-11-15`) with strongly typed camel-cased
 * shapes for instance / vpc / subnet / security-group / key-pair /
 * volume operations used by the UI.
 */

import { ENDPOINT, amzDate, authHeader, loggedFetch } from "$lib/aws";

const SERVICE = "ec2";
const VERSION = "2016-11-15";

// ---------- Types ----------

export interface Instance {
  instanceId: string;
  instanceType: string;
  state: string;
  privateIp: string;
  publicIp: string;
  imageId: string;
  vpcId: string;
  subnetId: string;
  keyName: string;
  launchTime: string;
  architecture: string;
  platform: string;
  availabilityZone: string;
  securityGroupIds: string[];
  tags: Record<string, string>;
}

export interface SecurityGroup {
  groupId: string;
  groupName: string;
  description: string;
  vpcId: string;
  ingress: SecurityGroupRule[];
  egress: SecurityGroupRule[];
}

export interface SecurityGroupRule {
  protocol: string;
  fromPort: number | null;
  toPort: number | null;
  cidrIpv4: string[];
  cidrIpv6: string[];
  description: string;
}

export interface KeyPair {
  keyPairId: string;
  keyName: string;
  keyType: string;
  fingerprint: string;
  createTime: string;
}

export interface Vpc {
  vpcId: string;
  cidrBlock: string;
  state: string;
  isDefault: boolean;
  dhcpOptionsId: string;
  instanceTenancy: string;
}

export interface Subnet {
  subnetId: string;
  vpcId: string;
  cidrBlock: string;
  availabilityZone: string;
  availableIpAddressCount: number;
  state: string;
  defaultForAz: boolean;
  mapPublicIpOnLaunch: boolean;
}

export interface Volume {
  volumeId: string;
  size: number;
  state: string;
  volumeType: string;
  availabilityZone: string;
  createTime: string;
  encrypted: boolean;
  attachments: VolumeAttachment[];
}

export interface VolumeAttachment {
  instanceId: string;
  device: string;
  state: string;
}

export interface RunInstancesInput {
  imageId: string;
  instanceType: string;
  minCount?: number;
  maxCount?: number;
  keyName?: string;
  subnetId?: string;
  securityGroupIds?: string[];
  name?: string;
}

// ---------- Internal request helper ----------

async function request(
  action: string,
  params: Record<string, string | string[]> = {},
): Promise<Document> {
  const body = new URLSearchParams({ Action: action, Version: VERSION });
  for (const [k, v] of Object.entries(params)) {
    if (Array.isArray(v)) {
      v.forEach((item, i) => body.append(`${k}.${i + 1}`, item));
    } else {
      body.append(k, v);
    }
  }
  const res = await loggedFetch(SERVICE, action, "POST", ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-www-form-urlencoded",
      Authorization: authHeader(SERVICE),
      "X-Amz-Date": amzDate(),
    },
    body: body.toString(),
  });
  const text = await res.text();
  if (!res.ok) {
    throw new Error(`EC2 ${action} failed (HTTP ${res.status}): ${text}`);
  }
  return new DOMParser().parseFromString(text, "application/xml");
}

function text(el: Element | null, tag: string): string {
  return el?.querySelector(tag)?.textContent ?? "";
}

function items(el: Element | null, name: string): Element[] {
  if (!el) return [];
  const set = el.querySelector(`:scope > ${name}`);
  if (!set) return [];
  return Array.from(set.querySelectorAll(":scope > item"));
}

function tagsOf(el: Element | null): Record<string, string> {
  const out: Record<string, string> = {};
  for (const t of items(el, "tagSet")) {
    const k = text(t, "key");
    const v = text(t, "value");
    if (k) out[k] = v;
  }
  return out;
}

// ---------- Operations ----------

export async function describeInstances(): Promise<Instance[]> {
  const doc = await request("DescribeInstances");
  const instances: Instance[] = [];
  const reservations = doc.querySelectorAll("reservationSet > item");
  for (const reservation of Array.from(reservations)) {
    const instSet = reservation.querySelector(":scope > instancesSet");
    const instItems = instSet
      ? Array.from(instSet.querySelectorAll(":scope > item"))
      : [];
    for (const it of instItems) {
      const sgIds: string[] = [];
      const sgSet = it.querySelector(":scope > groupSet");
      if (sgSet) {
        for (const g of Array.from(
          sgSet.querySelectorAll(":scope > item > groupId"),
        )) {
          if (g.textContent) sgIds.push(g.textContent);
        }
      }
      instances.push({
        instanceId: text(it, "instanceId"),
        instanceType: text(it, "instanceType"),
        state: text(it, "instanceState > name"),
        privateIp: text(it, "privateIpAddress"),
        publicIp: text(it, "ipAddress"),
        imageId: text(it, "imageId"),
        vpcId: text(it, "vpcId"),
        subnetId: text(it, "subnetId"),
        keyName: text(it, "keyName"),
        launchTime: text(it, "launchTime"),
        architecture: text(it, "architecture"),
        platform: text(it, "platformDetails") || "linux",
        availabilityZone: text(it, "placement > availabilityZone"),
        securityGroupIds: sgIds,
        tags: tagsOf(it),
      });
    }
  }
  return instances;
}

export async function runInstances(
  input: RunInstancesInput,
): Promise<string[]> {
  const params: Record<string, string | string[]> = {
    ImageId: input.imageId,
    InstanceType: input.instanceType,
    MinCount: String(input.minCount ?? 1),
    MaxCount: String(input.maxCount ?? 1),
  };
  if (input.keyName) params["KeyName"] = input.keyName;
  if (input.subnetId) params["SubnetId"] = input.subnetId;
  if (input.securityGroupIds && input.securityGroupIds.length > 0) {
    params["SecurityGroupId"] = input.securityGroupIds;
  }
  const doc = await request("RunInstances", params);
  const ids: string[] = [];
  for (const el of Array.from(
    doc.querySelectorAll("instancesSet > item > instanceId"),
  )) {
    if (el.textContent) ids.push(el.textContent);
  }
  if (input.name && ids.length > 0) {
    await createTags(ids, { Name: input.name });
  }
  return ids;
}

export async function terminateInstances(instanceIds: string[]): Promise<void> {
  await request("TerminateInstances", { InstanceId: instanceIds });
}

export async function startInstances(instanceIds: string[]): Promise<void> {
  await request("StartInstances", { InstanceId: instanceIds });
}

export async function stopInstances(instanceIds: string[]): Promise<void> {
  await request("StopInstances", { InstanceId: instanceIds });
}

export async function rebootInstances(instanceIds: string[]): Promise<void> {
  await request("RebootInstances", { InstanceId: instanceIds });
}

export async function createTags(
  resourceIds: string[],
  tags: Record<string, string>,
): Promise<void> {
  const params: Record<string, string | string[]> = {
    ResourceId: resourceIds,
  };
  let i = 1;
  for (const [k, v] of Object.entries(tags)) {
    params[`Tag.${i}.Key`] = k;
    params[`Tag.${i}.Value`] = v;
    i++;
  }
  await request("CreateTags", params);
}

function parsePort(value: string): number | null {
  if (!value) return null;
  const n = parseInt(value, 10);
  return Number.isNaN(n) ? null : n;
}

function parseSgRules(el: Element, name: string): SecurityGroupRule[] {
  const rules: SecurityGroupRule[] = [];
  for (const r of items(el, name)) {
    const cidrIpv4: string[] = [];
    const cidrIpv6: string[] = [];
    for (const range of items(r, "ipRanges")) {
      const c = text(range, "cidrIp");
      if (c) cidrIpv4.push(c);
    }
    for (const range of items(r, "ipv6Ranges")) {
      const c = text(range, "cidrIpv6");
      if (c) cidrIpv6.push(c);
    }
    rules.push({
      protocol: text(r, "ipProtocol"),
      fromPort: parsePort(text(r, "fromPort")),
      toPort: parsePort(text(r, "toPort")),
      cidrIpv4,
      cidrIpv6,
      description: "",
    });
  }
  return rules;
}

export async function describeSecurityGroups(): Promise<SecurityGroup[]> {
  const doc = await request("DescribeSecurityGroups");
  const sgs: SecurityGroup[] = [];
  for (const el of Array.from(
    doc.querySelectorAll("securityGroupInfo > item"),
  )) {
    sgs.push({
      groupId: text(el, "groupId"),
      groupName: text(el, "groupName"),
      description: text(el, "groupDescription"),
      vpcId: text(el, "vpcId"),
      ingress: parseSgRules(el, "ipPermissions"),
      egress: parseSgRules(el, "ipPermissionsEgress"),
    });
  }
  return sgs;
}

export async function createSecurityGroup(
  name: string,
  description: string,
  vpcId: string,
): Promise<string> {
  const doc = await request("CreateSecurityGroup", {
    GroupName: name,
    GroupDescription: description,
    VpcId: vpcId,
  });
  return text(doc.documentElement, "groupId");
}

export async function deleteSecurityGroup(groupId: string): Promise<void> {
  await request("DeleteSecurityGroup", { GroupId: groupId });
}

export async function describeKeyPairs(): Promise<KeyPair[]> {
  const doc = await request("DescribeKeyPairs");
  const keys: KeyPair[] = [];
  for (const el of Array.from(doc.querySelectorAll("keySet > item"))) {
    keys.push({
      keyPairId: text(el, "keyPairId"),
      keyName: text(el, "keyName"),
      keyType: text(el, "keyType") || "rsa",
      fingerprint: text(el, "keyFingerprint"),
      createTime: text(el, "createTime"),
    });
  }
  return keys;
}

export async function createKeyPair(
  keyName: string,
  keyType: "rsa" | "ed25519" = "rsa",
): Promise<{ keyName: string; fingerprint: string; material: string }> {
  const doc = await request("CreateKeyPair", {
    KeyName: keyName,
    KeyType: keyType,
  });
  return {
    keyName: text(doc.documentElement, "keyName"),
    fingerprint: text(doc.documentElement, "keyFingerprint"),
    material: text(doc.documentElement, "keyMaterial"),
  };
}

export async function deleteKeyPair(keyName: string): Promise<void> {
  await request("DeleteKeyPair", { KeyName: keyName });
}

export async function describeVpcs(): Promise<Vpc[]> {
  const doc = await request("DescribeVpcs");
  const vpcs: Vpc[] = [];
  for (const el of Array.from(doc.querySelectorAll("vpcSet > item"))) {
    vpcs.push({
      vpcId: text(el, "vpcId"),
      cidrBlock: text(el, "cidrBlock"),
      state: text(el, "state"),
      isDefault: text(el, "isDefault") === "true",
      dhcpOptionsId: text(el, "dhcpOptionsId"),
      instanceTenancy: text(el, "instanceTenancy"),
    });
  }
  return vpcs;
}

export async function createVpc(cidrBlock: string): Promise<string> {
  const doc = await request("CreateVpc", { CidrBlock: cidrBlock });
  return text(doc.documentElement, "vpc > vpcId");
}

export async function deleteVpc(vpcId: string): Promise<void> {
  await request("DeleteVpc", { VpcId: vpcId });
}

export async function describeSubnets(): Promise<Subnet[]> {
  const doc = await request("DescribeSubnets");
  const subnets: Subnet[] = [];
  for (const el of Array.from(doc.querySelectorAll("subnetSet > item"))) {
    subnets.push({
      subnetId: text(el, "subnetId"),
      vpcId: text(el, "vpcId"),
      cidrBlock: text(el, "cidrBlock"),
      availabilityZone: text(el, "availabilityZone"),
      availableIpAddressCount: parseInt(
        text(el, "availableIpAddressCount") || "0",
        10,
      ),
      state: text(el, "state"),
      defaultForAz: text(el, "defaultForAz") === "true",
      mapPublicIpOnLaunch: text(el, "mapPublicIpOnLaunch") === "true",
    });
  }
  return subnets;
}

export async function describeVolumes(): Promise<Volume[]> {
  const doc = await request("DescribeVolumes");
  const volumes: Volume[] = [];
  for (const el of Array.from(doc.querySelectorAll("volumeSet > item"))) {
    const attachments: VolumeAttachment[] = [];
    for (const a of items(el, "attachmentSet")) {
      attachments.push({
        instanceId: text(a, "instanceId"),
        device: text(a, "device"),
        state: text(a, "status"),
      });
    }
    volumes.push({
      volumeId: text(el, "volumeId"),
      size: parseInt(text(el, "size") || "0", 10),
      state: text(el, "status"),
      volumeType: text(el, "volumeType"),
      availabilityZone: text(el, "availabilityZone"),
      createTime: text(el, "createTime"),
      encrypted: text(el, "encrypted") === "true",
      attachments,
    });
  }
  return volumes;
}

export function tagName(tags: Record<string, string>): string {
  return tags["Name"] ?? "";
}

export function shortId(id: string): string {
  return id || "—";
}
