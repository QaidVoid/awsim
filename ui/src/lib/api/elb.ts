/**
 * ELBv2 (Elastic Load Balancing v2) API client.
 *
 * Wraps the LocalStack-compatible elasticloadbalancingv2 query API
 * (`Action=…&Version=2015-12-01`) with strongly typed camel-cased shapes
 * for load balancers, target groups, listeners, rules, and tag operations.
 */

import { ENDPOINT, amzDate, authHeader, loggedFetch } from "$lib/aws";

const SERVICE = "elasticloadbalancing";
const VERSION = "2015-12-01";

// ---------- Types ----------

export interface LoadBalancer {
  arn: string;
  name: string;
  dnsName: string;
  type: string;
  scheme: string;
  state: string;
  vpcId: string;
  createdTime: string;
}

export interface TargetGroup {
  arn: string;
  name: string;
  protocol: string;
  port: number;
  vpcId: string;
  targetType: string;
  healthCheckPath: string;
  healthCheckProtocol: string;
}

export interface Listener {
  arn: string;
  loadBalancerArn: string;
  port: number;
  protocol: string;
  defaultActions: string[];
}

export interface Rule {
  arn: string;
  priority: string;
  isDefault: boolean;
  conditions: { field: string; values: string[] }[];
  actions: string[];
}

export interface Tag {
  key: string;
  value: string;
}

// ---------- Internals ----------

async function request(
  action: string,
  params: Record<string, string> = {},
): Promise<string> {
  const body = new URLSearchParams({
    Action: action,
    Version: VERSION,
    ...params,
  });
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
    throw new Error(`ELB ${action} failed (HTTP ${res.status}): ${text}`);
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

// Walk member blocks at the *top level* of the supplied xml fragment
function topLevelMembers(xml: string): string[] {
  const out: string[] = [];
  const stack: string[] = [];
  const regex = /<(\/?)member>/g;
  let depth = 0;
  let start = -1;
  let m;
  while ((m = regex.exec(xml)) !== null) {
    if (m[1] === "") {
      if (depth === 0) start = m.index + m[0].length;
      depth += 1;
      stack.push("member");
    } else {
      depth -= 1;
      stack.pop();
      if (depth === 0 && start >= 0) {
        out.push(xml.slice(start, m.index));
        start = -1;
      }
    }
  }
  return out;
}

// ---------- Load balancers ----------

export async function describeLoadBalancers(): Promise<LoadBalancer[]> {
  const xml = await request("DescribeLoadBalancers");
  return topLevelMembers(xml).map((block) => ({
    arn: tagValue(block, "LoadBalancerArn"),
    name: tagValue(block, "LoadBalancerName"),
    dnsName: tagValue(block, "DNSName"),
    type: tagValue(block, "Type"),
    scheme: tagValue(block, "Scheme"),
    state: tagValue(block, "Code") || "active",
    vpcId: tagValue(block, "VpcId"),
    createdTime: tagValue(block, "CreatedTime"),
  }));
}

export interface CreateLoadBalancerInput {
  name: string;
  type: "application" | "network" | "gateway";
  scheme: "internet-facing" | "internal";
  subnetIds: string[];
}

export async function createLoadBalancer(
  input: CreateLoadBalancerInput,
): Promise<void> {
  const params: Record<string, string> = {
    Name: input.name,
    Type: input.type,
    Scheme: input.scheme,
  };
  input.subnetIds.forEach((id, i) => {
    params[`Subnets.member.${i + 1}`] = id;
  });
  await request("CreateLoadBalancer", params);
}

export async function deleteLoadBalancer(arn: string): Promise<void> {
  await request("DeleteLoadBalancer", { LoadBalancerArn: arn });
}

// ---------- Target groups ----------

export async function describeTargetGroups(
  loadBalancerArn?: string,
): Promise<TargetGroup[]> {
  const params: Record<string, string> = {};
  if (loadBalancerArn) params.LoadBalancerArn = loadBalancerArn;
  const xml = await request("DescribeTargetGroups", params);
  return topLevelMembers(xml).map((block) => ({
    arn: tagValue(block, "TargetGroupArn"),
    name: tagValue(block, "TargetGroupName"),
    protocol: tagValue(block, "Protocol"),
    port: Number(tagValue(block, "Port")) || 0,
    vpcId: tagValue(block, "VpcId"),
    targetType: tagValue(block, "TargetType"),
    healthCheckPath: tagValue(block, "HealthCheckPath"),
    healthCheckProtocol: tagValue(block, "HealthCheckProtocol"),
  }));
}

export interface CreateTargetGroupInput {
  name: string;
  protocol: "HTTP" | "HTTPS" | "TCP" | "TLS" | "UDP";
  port: number;
  targetType: "instance" | "ip" | "lambda" | "alb";
  vpcId?: string;
}

export async function createTargetGroup(
  input: CreateTargetGroupInput,
): Promise<void> {
  const params: Record<string, string> = {
    Name: input.name,
    Protocol: input.protocol,
    Port: String(input.port),
    TargetType: input.targetType,
  };
  if (input.vpcId) params.VpcId = input.vpcId;
  await request("CreateTargetGroup", params);
}

export async function deleteTargetGroup(arn: string): Promise<void> {
  await request("DeleteTargetGroup", { TargetGroupArn: arn });
}

// ---------- Listeners ----------

export async function describeListeners(
  loadBalancerArn: string,
): Promise<Listener[]> {
  const xml = await request("DescribeListeners", {
    LoadBalancerArn: loadBalancerArn,
  });
  return topLevelMembers(xml).map((block) => {
    const actions: string[] = [];
    const actionsBlock = xml.match(
      /<DefaultActions>([\s\S]*?)<\/DefaultActions>/,
    );
    if (actionsBlock) {
      tagBlocks(actionsBlock[1], "Type").forEach((t) => actions.push(t));
    }
    return {
      arn: tagValue(block, "ListenerArn"),
      loadBalancerArn: tagValue(block, "LoadBalancerArn"),
      port: Number(tagValue(block, "Port")) || 0,
      protocol: tagValue(block, "Protocol"),
      defaultActions: actions,
    };
  });
}

// ---------- Rules ----------

export async function describeRules(listenerArn: string): Promise<Rule[]> {
  const xml = await request("DescribeRules", { ListenerArn: listenerArn });
  return topLevelMembers(xml).map((block) => {
    const isDefault = tagValue(block, "IsDefault") === "true";
    const priority = tagValue(block, "Priority");

    const conditions: Rule["conditions"] = [];
    const condBlock = block.match(/<Conditions>([\s\S]*?)<\/Conditions>/);
    if (condBlock) {
      for (const m of topLevelMembers(condBlock[1])) {
        const field = tagValue(m, "Field");
        const valuesBlock = m.match(/<Values>([\s\S]*?)<\/Values>/);
        const values: string[] = [];
        if (valuesBlock) {
          tagBlocks(valuesBlock[1], "member").forEach((v) =>
            values.push(v.trim()),
          );
        }
        conditions.push({ field, values });
      }
    }

    const actions: string[] = [];
    const actionsBlock = block.match(/<Actions>([\s\S]*?)<\/Actions>/);
    if (actionsBlock) {
      tagBlocks(actionsBlock[1], "Type").forEach((t) => actions.push(t));
    }

    return {
      arn: tagValue(block, "RuleArn"),
      priority,
      isDefault,
      conditions,
      actions,
    };
  });
}

// ---------- Tags ----------

export async function describeTags(arn: string): Promise<Tag[]> {
  const xml = await request("DescribeTags", { "ResourceArns.member.1": arn });
  const tagDescBlock = xml.match(/<Tags>([\s\S]*?)<\/Tags>/);
  if (!tagDescBlock) return [];
  return topLevelMembers(tagDescBlock[1]).map((m) => ({
    key: tagValue(m, "Key"),
    value: tagValue(m, "Value"),
  }));
}

export async function addTags(arn: string, tags: Tag[]): Promise<void> {
  const params: Record<string, string> = { "ResourceArns.member.1": arn };
  tags.forEach((t, i) => {
    params[`Tags.member.${i + 1}.Key`] = t.key;
    params[`Tags.member.${i + 1}.Value`] = t.value;
  });
  await request("AddTags", params);
}

export async function removeTags(arn: string, keys: string[]): Promise<void> {
  const params: Record<string, string> = { "ResourceArns.member.1": arn };
  keys.forEach((k, i) => {
    params[`TagKeys.member.${i + 1}`] = k;
  });
  await request("RemoveTags", params);
}
