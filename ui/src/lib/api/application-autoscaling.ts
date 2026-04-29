/**
 * Typed Application Auto Scaling API client. AwsJson1.1 — X-Amz-Target prefix
 * is `AnyScaleFrontendService`.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "application-autoscaling";
const TARGET_PREFIX = "AnyScaleFrontendService";

export interface ScalableTarget {
  serviceNamespace: string;
  resourceId: string;
  scalableDimension: string;
  minCapacity: number;
  maxCapacity: number;
  roleArn: string;
  creationTime: number;
}

export interface ScalingPolicy {
  policyName: string;
  policyArn: string;
  serviceNamespace: string;
  resourceId: string;
  scalableDimension: string;
  policyType: string;
  creationTime: number;
}

async function request<T>(
  action: string,
  body: Record<string, unknown> = {},
): Promise<T> {
  const res = await loggedFetch(SERVICE, action, "POST", ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.1",
      "X-Amz-Target": `${TARGET_PREFIX}.${action}`,
      Authorization: authHeader(SERVICE),
      "X-Amz-Date": amzDate(),
    },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) {
    let msg = text;
    try {
      const data = JSON.parse(text) as { message?: string; Message?: string };
      msg = data.message ?? data.Message ?? text;
    } catch {
      // not JSON
    }
    throw new Error(
      `Application Auto Scaling ${action} failed (HTTP ${res.status}): ${msg}`,
    );
  }
  return (text ? JSON.parse(text) : {}) as T;
}

interface RawTarget {
  ServiceNamespace: string;
  ResourceId: string;
  ScalableDimension: string;
  MinCapacity: number;
  MaxCapacity: number;
  RoleARN: string;
  CreationTime: number;
}

interface RawPolicy {
  PolicyName: string;
  PolicyARN: string;
  ServiceNamespace: string;
  ResourceId: string;
  ScalableDimension: string;
  PolicyType: string;
  CreationTime: number;
}

function fromRawTarget(r: RawTarget): ScalableTarget {
  return {
    serviceNamespace: r.ServiceNamespace,
    resourceId: r.ResourceId,
    scalableDimension: r.ScalableDimension,
    minCapacity: r.MinCapacity,
    maxCapacity: r.MaxCapacity,
    roleArn: r.RoleARN,
    creationTime: r.CreationTime,
  };
}

function fromRawPolicy(r: RawPolicy): ScalingPolicy {
  return {
    policyName: r.PolicyName,
    policyArn: r.PolicyARN,
    serviceNamespace: r.ServiceNamespace,
    resourceId: r.ResourceId,
    scalableDimension: r.ScalableDimension,
    policyType: r.PolicyType,
    creationTime: r.CreationTime,
  };
}

export const SERVICE_NAMESPACES = [
  "ecs",
  "lambda",
  "dynamodb",
  "rds",
  "appstream",
  "elasticmapreduce",
  "comprehend",
  "kafka",
  "sagemaker",
  "cassandra",
  "neptune",
  "elasticache",
  "custom-resource",
] as const;

export async function describeScalableTargets(
  serviceNamespace: string,
): Promise<ScalableTarget[]> {
  const data = await request<{ ScalableTargets?: RawTarget[] }>(
    "DescribeScalableTargets",
    { ServiceNamespace: serviceNamespace },
  );
  return (data.ScalableTargets ?? []).map(fromRawTarget);
}

export async function registerScalableTarget(input: {
  serviceNamespace: string;
  resourceId: string;
  scalableDimension: string;
  minCapacity: number;
  maxCapacity: number;
  roleArn?: string;
}): Promise<void> {
  const body: Record<string, unknown> = {
    ServiceNamespace: input.serviceNamespace,
    ResourceId: input.resourceId,
    ScalableDimension: input.scalableDimension,
    MinCapacity: input.minCapacity,
    MaxCapacity: input.maxCapacity,
  };
  if (input.roleArn) body.RoleARN = input.roleArn;
  await request<unknown>("RegisterScalableTarget", body);
}

export async function deregisterScalableTarget(input: {
  serviceNamespace: string;
  resourceId: string;
  scalableDimension: string;
}): Promise<void> {
  await request<unknown>("DeregisterScalableTarget", {
    ServiceNamespace: input.serviceNamespace,
    ResourceId: input.resourceId,
    ScalableDimension: input.scalableDimension,
  });
}

export async function describeScalingPolicies(
  serviceNamespace: string,
  resourceId?: string,
): Promise<ScalingPolicy[]> {
  const body: Record<string, unknown> = {
    ServiceNamespace: serviceNamespace,
  };
  if (resourceId) body.ResourceId = resourceId;
  const data = await request<{ ScalingPolicies?: RawPolicy[] }>(
    "DescribeScalingPolicies",
    body,
  );
  return (data.ScalingPolicies ?? []).map(fromRawPolicy);
}

export async function deleteScalingPolicy(input: {
  serviceNamespace: string;
  resourceId: string;
  scalableDimension: string;
  policyName: string;
}): Promise<void> {
  await request<unknown>("DeleteScalingPolicy", {
    ServiceNamespace: input.serviceNamespace,
    ResourceId: input.resourceId,
    ScalableDimension: input.scalableDimension,
    PolicyName: input.policyName,
  });
}
