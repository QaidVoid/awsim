/**
 * Typed SSM (Systems Manager) API client.
 *
 * Wraps the AWS JSON 1.1 AmazonSSM API with strong types.
 * Names map directly to the AWS SDK SSM operations.
 */

import { ENDPOINT, amzDate, authHeader, loggedFetch } from "$lib/aws";

const SERVICE = "ssm";
const TARGET_PREFIX = "AmazonSSM";

// ---------- Types ----------

export type ParameterType = "String" | "StringList" | "SecureString";

export interface Parameter {
  name: string;
  type: ParameterType;
  version: number;
  lastModifiedDate?: string;
  description?: string;
  tier?: string;
  policies?: string;
}

export interface ParameterValue {
  name: string;
  type: ParameterType;
  value: string;
  version: number;
  lastModifiedDate?: string;
  arn?: string;
}

export interface SsmDocument {
  name: string;
  documentType?: string;
  documentFormat?: string;
  documentVersion?: string;
  owner?: string;
  platformTypes?: string[];
  schemaVersion?: string;
  status?: string;
  createdDate?: string;
}

export interface SsmDocumentDetail extends SsmDocument {
  content?: string;
  description?: string;
}

export interface Activation {
  activationId: string;
  description?: string;
  defaultInstanceName?: string;
  iamRole?: string;
  registrationLimit?: number;
  registrationsCount?: number;
  expirationDate?: string;
  expired?: boolean;
  createdDate?: string;
}

export interface MaintenanceWindow {
  windowId: string;
  name: string;
  description?: string;
  enabled?: boolean;
  duration?: number;
  cutoff?: number;
  schedule?: string;
  scheduleTimezone?: string;
  nextExecutionTime?: string;
}

export interface OpsItem {
  opsItemId: string;
  title?: string;
  status?: string;
  priority?: number;
  source?: string;
  category?: string;
  severity?: string;
  createdTime?: string;
  lastModifiedTime?: string;
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
  const text = await res.text();
  if (!res.ok) {
    let msg = text;
    try {
      const data = JSON.parse(text) as { message?: string; Message?: string };
      msg = data.message ?? data.Message ?? text;
    } catch {
      // not JSON
    }
    throw new Error(`SSM ${action} failed (HTTP ${res.status}): ${msg}`);
  }
  return (text ? JSON.parse(text) : {}) as T;
}

// ---------- Parameters ----------

interface RawParameterMeta {
  Name: string;
  Type?: ParameterType;
  Version?: number;
  LastModifiedDate?: number;
  Description?: string;
  Tier?: string;
  Policies?: string;
}

function mapParam(p: RawParameterMeta): Parameter {
  return {
    name: p.Name,
    type: p.Type ?? "String",
    version: p.Version ?? 1,
    lastModifiedDate: p.LastModifiedDate
      ? new Date(p.LastModifiedDate * 1000).toISOString()
      : undefined,
    description: p.Description,
    tier: p.Tier,
    policies: p.Policies,
  };
}

export async function describeParameters(): Promise<Parameter[]> {
  const data = await request<{ Parameters?: RawParameterMeta[] }>(
    "DescribeParameters",
  );
  return (data.Parameters ?? []).map(mapParam);
}

export async function getParameter(
  name: string,
  withDecryption = true,
): Promise<ParameterValue> {
  const data = await request<{
    Parameter?: {
      Name: string;
      Type: ParameterType;
      Value: string;
      Version: number;
      LastModifiedDate?: number;
      ARN?: string;
    };
  }>("GetParameter", { Name: name, WithDecryption: withDecryption });
  const p = data.Parameter;
  return {
    name: p?.Name ?? name,
    type: p?.Type ?? "String",
    value: p?.Value ?? "",
    version: p?.Version ?? 1,
    lastModifiedDate: p?.LastModifiedDate
      ? new Date(p.LastModifiedDate * 1000).toISOString()
      : undefined,
    arn: p?.ARN,
  };
}

export async function getParametersByPath(
  path: string,
  recursive = true,
  withDecryption = true,
): Promise<ParameterValue[]> {
  const data = await request<{
    Parameters?: {
      Name: string;
      Type: ParameterType;
      Value: string;
      Version: number;
      LastModifiedDate?: number;
      ARN?: string;
    }[];
  }>("GetParametersByPath", {
    Path: path,
    Recursive: recursive,
    WithDecryption: withDecryption,
  });
  return (data.Parameters ?? []).map((p) => ({
    name: p.Name,
    type: p.Type,
    value: p.Value,
    version: p.Version,
    lastModifiedDate: p.LastModifiedDate
      ? new Date(p.LastModifiedDate * 1000).toISOString()
      : undefined,
    arn: p.ARN,
  }));
}

export interface PutParameterInput {
  name: string;
  value: string;
  type: ParameterType;
  description?: string;
  overwrite?: boolean;
  tier?: "Standard" | "Advanced";
}

export async function putParameter(
  input: PutParameterInput,
): Promise<{ version: number }> {
  const params: Record<string, unknown> = {
    Name: input.name,
    Value: input.value,
    Type: input.type,
    Overwrite: input.overwrite ?? true,
  };
  if (input.description) params["Description"] = input.description;
  if (input.tier) params["Tier"] = input.tier;
  const data = await request<{ Version?: number }>("PutParameter", params);
  return { version: data.Version ?? 1 };
}

export async function deleteParameter(name: string): Promise<void> {
  await request("DeleteParameter", { Name: name });
}

// ---------- Documents ----------

export async function listDocuments(): Promise<SsmDocument[]> {
  const data = await request<{
    DocumentIdentifiers?: {
      Name: string;
      Owner?: string;
      DocumentType?: string;
      DocumentFormat?: string;
      DocumentVersion?: string;
      PlatformTypes?: string[];
      SchemaVersion?: string;
      CreatedDate?: number;
    }[];
  }>("ListDocuments");
  return (data.DocumentIdentifiers ?? []).map((d) => ({
    name: d.Name,
    documentType: d.DocumentType,
    documentFormat: d.DocumentFormat,
    documentVersion: d.DocumentVersion,
    owner: d.Owner,
    platformTypes: d.PlatformTypes,
    schemaVersion: d.SchemaVersion,
    createdDate: d.CreatedDate
      ? new Date(d.CreatedDate * 1000).toISOString()
      : undefined,
  }));
}

export async function getDocument(
  name: string,
  documentVersion?: string,
): Promise<SsmDocumentDetail> {
  const params: Record<string, unknown> = { Name: name };
  if (documentVersion) params["DocumentVersion"] = documentVersion;
  const data = await request<{
    Name: string;
    Content?: string;
    DocumentType?: string;
    DocumentFormat?: string;
    DocumentVersion?: string;
    Status?: string;
  }>("GetDocument", params);
  return {
    name: data.Name,
    content: data.Content,
    documentType: data.DocumentType,
    documentFormat: data.DocumentFormat,
    documentVersion: data.DocumentVersion,
    status: data.Status,
  };
}

// ---------- Activations ----------

export async function describeActivations(): Promise<Activation[]> {
  const data = await request<{
    ActivationList?: {
      ActivationId: string;
      Description?: string;
      DefaultInstanceName?: string;
      IamRole?: string;
      RegistrationLimit?: number;
      RegistrationsCount?: number;
      ExpirationDate?: number;
      Expired?: boolean;
      CreatedDate?: number;
    }[];
  }>("DescribeActivations");
  return (data.ActivationList ?? []).map((a) => ({
    activationId: a.ActivationId,
    description: a.Description,
    defaultInstanceName: a.DefaultInstanceName,
    iamRole: a.IamRole,
    registrationLimit: a.RegistrationLimit,
    registrationsCount: a.RegistrationsCount,
    expirationDate: a.ExpirationDate
      ? new Date(a.ExpirationDate * 1000).toISOString()
      : undefined,
    expired: a.Expired,
    createdDate: a.CreatedDate
      ? new Date(a.CreatedDate * 1000).toISOString()
      : undefined,
  }));
}

// ---------- Maintenance Windows ----------

export async function describeMaintenanceWindows(): Promise<
  MaintenanceWindow[]
> {
  const data = await request<{
    WindowIdentities?: {
      WindowId: string;
      Name: string;
      Description?: string;
      Enabled?: boolean;
      Duration?: number;
      Cutoff?: number;
      Schedule?: string;
      ScheduleTimezone?: string;
      NextExecutionTime?: string;
    }[];
  }>("DescribeMaintenanceWindows");
  return (data.WindowIdentities ?? []).map((w) => ({
    windowId: w.WindowId,
    name: w.Name,
    description: w.Description,
    enabled: w.Enabled,
    duration: w.Duration,
    cutoff: w.Cutoff,
    schedule: w.Schedule,
    scheduleTimezone: w.ScheduleTimezone,
    nextExecutionTime: w.NextExecutionTime,
  }));
}

// ---------- Ops Items ----------

export async function describeOpsItems(): Promise<OpsItem[]> {
  const data = await request<{
    OpsItemSummaries?: {
      OpsItemId: string;
      Title?: string;
      Status?: string;
      Priority?: number;
      Source?: string;
      Category?: string;
      Severity?: string;
      CreatedTime?: number;
      LastModifiedTime?: number;
    }[];
  }>("DescribeOpsItems");
  return (data.OpsItemSummaries ?? []).map((o) => ({
    opsItemId: o.OpsItemId,
    title: o.Title,
    status: o.Status,
    priority: o.Priority,
    source: o.Source,
    category: o.Category,
    severity: o.Severity,
    createdTime: o.CreatedTime
      ? new Date(o.CreatedTime * 1000).toISOString()
      : undefined,
    lastModifiedTime: o.LastModifiedTime
      ? new Date(o.LastModifiedTime * 1000).toISOString()
      : undefined,
  }));
}
