/**
 * Typed AWS Backup API client.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "backup";

export interface BackupVault {
  name: string;
  arn: string;
  creationDate: number;
  encryptionKeyArn?: string;
  numberOfRecoveryPoints: number;
  locked: boolean;
  minRetentionDays?: number;
  maxRetentionDays?: number;
}

export interface BackupPlan {
  planId: string;
  planArn: string;
  planName: string;
  versionId: string;
  creationDate: number;
  lastExecutionDate?: number;
}

export interface BackupSelection {
  selectionId: string;
  planId: string;
  selectionName: string;
  iamRoleArn: string;
  resources: string[];
  creationDate: number;
}

export interface BackupJob {
  jobId: string;
  vaultName: string;
  recoveryPointArn: string;
  resourceArn: string;
  resourceType: string;
  state: string;
  percentDone: string;
  creationDate: number;
  completionDate?: number;
  backupSizeInBytes: number;
}

function headers(): Record<string, string> {
  return {
    "Content-Type": "application/json",
    Authorization: authHeader(SERVICE),
    "X-Amz-Date": amzDate(),
  };
}

async function request<T>(
  action: string,
  method: "GET" | "POST" | "PUT" | "DELETE",
  path: string,
  body?: Record<string, unknown>,
): Promise<T> {
  const opts: RequestInit = { method, headers: headers() };
  if (body !== undefined) opts.body = JSON.stringify(body);
  const res = await loggedFetch(
    SERVICE,
    action,
    method,
    `${ENDPOINT}${path}`,
    opts,
  );
  const text = await res.text();
  if (!res.ok) {
    let msg = text;
    try {
      const data = JSON.parse(text) as { message?: string; Message?: string };
      msg = data.message ?? data.Message ?? text;
    } catch {
      // not JSON
    }
    throw new Error(`Backup ${action} failed (HTTP ${res.status}): ${msg}`);
  }
  return (text ? JSON.parse(text) : {}) as T;
}

interface RawVault {
  BackupVaultName: string;
  BackupVaultArn: string;
  CreationDate: number;
  EncryptionKeyArn?: string;
  NumberOfRecoveryPoints: number;
  Locked: boolean;
  MinRetentionDays?: number;
  MaxRetentionDays?: number;
}

interface RawPlan {
  BackupPlanId: string;
  BackupPlanArn: string;
  BackupPlanName: string;
  VersionId: string;
  CreationDate: number;
  LastExecutionDate?: number;
}

interface RawSelection {
  SelectionId: string;
  BackupPlanId: string;
  SelectionName: string;
  IamRoleArn: string;
  Resources?: string[];
  CreationDate: number;
}

interface RawJob {
  BackupJobId: string;
  BackupVaultName: string;
  RecoveryPointArn: string;
  ResourceArn: string;
  ResourceType: string;
  State: string;
  PercentDone: string;
  CreationDate: number;
  CompletionDate?: number;
  BackupSizeInBytes: number;
}

function fromRawVault(r: RawVault): BackupVault {
  return {
    name: r.BackupVaultName,
    arn: r.BackupVaultArn,
    creationDate: r.CreationDate,
    encryptionKeyArn: r.EncryptionKeyArn,
    numberOfRecoveryPoints: r.NumberOfRecoveryPoints,
    locked: r.Locked,
    minRetentionDays: r.MinRetentionDays,
    maxRetentionDays: r.MaxRetentionDays,
  };
}

function fromRawPlan(r: RawPlan): BackupPlan {
  return {
    planId: r.BackupPlanId,
    planArn: r.BackupPlanArn,
    planName: r.BackupPlanName,
    versionId: r.VersionId,
    creationDate: r.CreationDate,
    lastExecutionDate: r.LastExecutionDate,
  };
}

function fromRawSelection(r: RawSelection): BackupSelection {
  return {
    selectionId: r.SelectionId,
    planId: r.BackupPlanId,
    selectionName: r.SelectionName,
    iamRoleArn: r.IamRoleArn,
    resources: r.Resources ?? [],
    creationDate: r.CreationDate,
  };
}

function fromRawJob(r: RawJob): BackupJob {
  return {
    jobId: r.BackupJobId,
    vaultName: r.BackupVaultName,
    recoveryPointArn: r.RecoveryPointArn,
    resourceArn: r.ResourceArn,
    resourceType: r.ResourceType,
    state: r.State,
    percentDone: r.PercentDone,
    creationDate: r.CreationDate,
    completionDate: r.CompletionDate,
    backupSizeInBytes: r.BackupSizeInBytes,
  };
}

// ---------- Vaults ----------
export async function listVaults(): Promise<BackupVault[]> {
  const data = await request<{ BackupVaultList?: RawVault[] }>(
    "ListBackupVaults",
    "GET",
    "/backup-vaults",
  );
  return (data.BackupVaultList ?? []).map(fromRawVault);
}

export async function createVault(
  name: string,
  encryptionKeyArn?: string,
): Promise<void> {
  const body: Record<string, unknown> = { BackupVaultName: name };
  if (encryptionKeyArn) body.EncryptionKeyArn = encryptionKeyArn;
  await request<unknown>(
    "CreateBackupVault",
    "PUT",
    `/backup-vaults/${encodeURIComponent(name)}`,
    body,
  );
}

export async function deleteVault(name: string): Promise<void> {
  await request<unknown>(
    "DeleteBackupVault",
    "DELETE",
    `/backup-vaults/${encodeURIComponent(name)}`,
  );
}

// ---------- Plans ----------
export async function listPlans(): Promise<BackupPlan[]> {
  const data = await request<{ BackupPlansList?: RawPlan[] }>(
    "ListBackupPlans",
    "GET",
    "/backup/plans",
  );
  return (data.BackupPlansList ?? []).map(fromRawPlan);
}

export async function createPlan(
  name: string,
  ruleVaultName: string,
  scheduleExpression = "cron(0 5 ? * * *)",
  deleteAfterDays = 30,
): Promise<BackupPlan> {
  const body = {
    BackupPlan: {
      BackupPlanName: name,
      Rules: [
        {
          RuleName: "default",
          TargetBackupVaultName: ruleVaultName,
          ScheduleExpression: scheduleExpression,
          Lifecycle: { DeleteAfterDays: deleteAfterDays },
        },
      ],
    },
  };
  const r = await request<RawPlan>(
    "CreateBackupPlan",
    "PUT",
    "/backup/plans/",
    body,
  );
  return fromRawPlan(r);
}

export async function deletePlan(planId: string): Promise<void> {
  await request<unknown>(
    "DeleteBackupPlan",
    "DELETE",
    `/backup/plans/${encodeURIComponent(planId)}`,
  );
}

export async function listSelections(
  planId: string,
): Promise<BackupSelection[]> {
  const data = await request<{ BackupSelectionsList?: RawSelection[] }>(
    "ListBackupSelections",
    "GET",
    `/backup/plans/${encodeURIComponent(planId)}/selections`,
  );
  return (data.BackupSelectionsList ?? []).map(fromRawSelection);
}

// ---------- Jobs ----------
export async function listJobs(filter?: {
  vaultName?: string;
  state?: string;
}): Promise<BackupJob[]> {
  const params = new URLSearchParams();
  if (filter?.vaultName) params.set("ByBackupVaultName", filter.vaultName);
  if (filter?.state) params.set("ByState", filter.state);
  const qs = params.toString() ? `?${params.toString()}` : "";
  const data = await request<{ BackupJobs?: RawJob[] }>(
    "ListBackupJobs",
    "GET",
    `/backup-jobs${qs}`,
  );
  return (data.BackupJobs ?? []).map(fromRawJob);
}

export async function startJob(
  vaultName: string,
  resourceArn: string,
  iamRoleArn = "arn:aws:iam::000000000000:role/BackupRole",
): Promise<void> {
  await request<unknown>(
    "StartBackupJob",
    "PUT",
    "/backup-jobs",
    {
      BackupVaultName: vaultName,
      ResourceArn: resourceArn,
      IamRoleArn: iamRoleArn,
    },
  );
}
