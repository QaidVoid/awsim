/**
 * Typed AWS Cloud Map (Service Discovery) API client. AwsJson1.1 — X-Amz-Target
 * prefix is `Route53AutoNaming_v20170314`.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "servicediscovery";
const TARGET_PREFIX = "Route53AutoNaming_v20170314";

export interface Namespace {
  id: string;
  arn: string;
  name: string;
  type: "DNS_PUBLIC" | "DNS_PRIVATE" | "HTTP";
  description?: string;
  serviceCount: number;
  createDate: number;
}

export interface SDService {
  id: string;
  arn: string;
  name: string;
  namespaceId: string;
  description?: string;
  instanceCount: number;
  type: "DNS" | "HTTP";
  createDate: number;
}

export interface Instance {
  id: string;
  attributes: Record<string, string>;
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
    throw new Error(`Cloud Map ${action} failed (HTTP ${res.status}): ${msg}`);
  }
  return (text ? JSON.parse(text) : {}) as T;
}

interface RawNamespace {
  Id: string;
  Arn: string;
  Name: string;
  Type: "DNS_PUBLIC" | "DNS_PRIVATE" | "HTTP";
  Description?: string;
  ServiceCount: number;
  CreateDate: number;
}

interface RawService {
  Id: string;
  Arn: string;
  Name: string;
  NamespaceId: string;
  Description?: string;
  InstanceCount: number;
  Type: "DNS" | "HTTP";
  CreateDate: number;
}

interface RawInstance {
  Id: string;
  Attributes?: Record<string, string>;
}

function fromNs(r: RawNamespace): Namespace {
  return {
    id: r.Id,
    arn: r.Arn,
    name: r.Name,
    type: r.Type,
    description: r.Description,
    serviceCount: r.ServiceCount,
    createDate: r.CreateDate,
  };
}

function fromSvc(r: RawService): SDService {
  return {
    id: r.Id,
    arn: r.Arn,
    name: r.Name,
    namespaceId: r.NamespaceId,
    description: r.Description,
    instanceCount: r.InstanceCount,
    type: r.Type,
    createDate: r.CreateDate,
  };
}

function fromInst(r: RawInstance): Instance {
  return {
    id: r.Id,
    attributes: r.Attributes ?? {},
  };
}

// ---------- Namespaces ----------
export async function listNamespaces(): Promise<Namespace[]> {
  const data = await request<{ Namespaces?: RawNamespace[] }>("ListNamespaces");
  return (data.Namespaces ?? []).map(fromNs);
}

export async function createHttpNamespace(name: string): Promise<void> {
  await request<unknown>("CreateHttpNamespace", { Name: name });
}

export async function createPrivateDnsNamespace(name: string): Promise<void> {
  await request<unknown>("CreatePrivateDnsNamespace", { Name: name });
}

export async function createPublicDnsNamespace(name: string): Promise<void> {
  await request<unknown>("CreatePublicDnsNamespace", { Name: name });
}

export async function deleteNamespace(id: string): Promise<void> {
  await request<unknown>("DeleteNamespace", { Id: id });
}

// ---------- Services ----------
export async function listServices(namespaceId?: string): Promise<SDService[]> {
  const body: Record<string, unknown> = {};
  if (namespaceId) {
    body.Filters = [
      { Name: "NAMESPACE_ID", Values: [namespaceId], Condition: "EQ" },
    ];
  }
  const data = await request<{ Services?: RawService[] }>("ListServices", body);
  return (data.Services ?? []).map(fromSvc);
}

export async function createService(
  namespaceId: string,
  name: string,
): Promise<SDService> {
  const data = await request<{ Service?: RawService }>("CreateService", {
    NamespaceId: namespaceId,
    Name: name,
  });
  return fromSvc(
    data.Service ?? {
      Id: "",
      Arn: "",
      Name: name,
      NamespaceId: namespaceId,
      InstanceCount: 0,
      Type: "HTTP",
      CreateDate: 0,
    },
  );
}

export async function deleteService(id: string): Promise<void> {
  await request<unknown>("DeleteService", { Id: id });
}

// ---------- Instances ----------
export async function listInstances(serviceId: string): Promise<Instance[]> {
  const data = await request<{ Instances?: RawInstance[] }>("ListInstances", {
    ServiceId: serviceId,
  });
  return (data.Instances ?? []).map(fromInst);
}

export async function registerInstance(
  serviceId: string,
  instanceId: string,
  attributes: Record<string, string>,
): Promise<void> {
  await request<unknown>("RegisterInstance", {
    ServiceId: serviceId,
    InstanceId: instanceId,
    Attributes: attributes,
  });
}

export async function deregisterInstance(
  serviceId: string,
  instanceId: string,
): Promise<void> {
  await request<unknown>("DeregisterInstance", {
    ServiceId: serviceId,
    InstanceId: instanceId,
  });
}
