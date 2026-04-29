/**
 * Typed Amazon S3 Glacier API client. RestJson1 — paths use `-` as the
 * accountId placeholder (resolves to the caller's account).
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "glacier";

export interface Vault {
  vaultName: string;
  vaultArn: string;
  creationDate: string;
  numberOfArchives: number;
  sizeInBytes: number;
}

export interface ArchiveJob {
  jobId: string;
  vaultArn: string;
  action: string;
  archiveId?: string;
  statusCode: string;
  creationDate: string;
  completionDate?: string;
  jobDescription?: string;
  tier?: string;
  completed: boolean;
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
  const res = await loggedFetch(SERVICE, action, method, `${ENDPOINT}${path}`, opts);
  const text = await res.text();
  if (!res.ok) {
    let msg = text;
    try {
      const data = JSON.parse(text) as { message?: string; Message?: string };
      msg = data.message ?? data.Message ?? text;
    } catch {
      // not JSON
    }
    throw new Error(`Glacier ${action} failed (HTTP ${res.status}): ${msg}`);
  }
  return (text ? JSON.parse(text) : {}) as T;
}

interface RawVault {
  VaultName: string;
  VaultARN: string;
  CreationDate: string;
  NumberOfArchives: number;
  SizeInBytes: number;
}

interface RawJob {
  JobId: string;
  VaultARN: string;
  Action: string;
  ArchiveId?: string;
  StatusCode: string;
  CreationDate: string;
  CompletionDate?: string;
  JobDescription?: string;
  Tier?: string;
  Completed: boolean;
}

const fromVault = (r: RawVault): Vault => ({
  vaultName: r.VaultName,
  vaultArn: r.VaultARN,
  creationDate: r.CreationDate,
  numberOfArchives: r.NumberOfArchives,
  sizeInBytes: r.SizeInBytes,
});

const fromJob = (r: RawJob): ArchiveJob => ({
  jobId: r.JobId,
  vaultArn: r.VaultARN,
  action: r.Action,
  archiveId: r.ArchiveId,
  statusCode: r.StatusCode,
  creationDate: r.CreationDate,
  completionDate: r.CompletionDate,
  jobDescription: r.JobDescription,
  tier: r.Tier,
  completed: r.Completed,
});

const ACCT = "-"; // requester account

export async function listVaults(): Promise<Vault[]> {
  const data = await request<{ VaultList?: RawVault[] }>(
    "ListVaults",
    "GET",
    `/${ACCT}/vaults`,
  );
  return (data.VaultList ?? []).map(fromVault);
}

export async function createVault(name: string): Promise<void> {
  await request<unknown>(
    "CreateVault",
    "PUT",
    `/${ACCT}/vaults/${encodeURIComponent(name)}`,
  );
}

export async function deleteVault(name: string): Promise<void> {
  await request<unknown>(
    "DeleteVault",
    "DELETE",
    `/${ACCT}/vaults/${encodeURIComponent(name)}`,
  );
}

export async function uploadArchive(
  vaultName: string,
  contentBase64: string,
  description?: string,
): Promise<{ archiveId: string; checksum: string }> {
  const body: Record<string, unknown> = { body: contentBase64 };
  if (description) body.archiveDescription = description;
  const r = await request<{ ArchiveId: string; Checksum: string }>(
    "UploadArchive",
    "POST",
    `/${ACCT}/vaults/${encodeURIComponent(vaultName)}/archives`,
    body,
  );
  return { archiveId: r.ArchiveId, checksum: r.Checksum };
}

export async function deleteArchive(
  vaultName: string,
  archiveId: string,
): Promise<void> {
  await request<unknown>(
    "DeleteArchive",
    "DELETE",
    `/${ACCT}/vaults/${encodeURIComponent(vaultName)}/archives/${encodeURIComponent(archiveId)}`,
  );
}

export async function listJobs(vaultName: string): Promise<ArchiveJob[]> {
  const data = await request<{ JobList?: RawJob[] }>(
    "ListJobs",
    "GET",
    `/${ACCT}/vaults/${encodeURIComponent(vaultName)}/jobs`,
  );
  return (data.JobList ?? []).map(fromJob);
}

export async function initiateJob(
  vaultName: string,
  type: "inventory-retrieval" | "archive-retrieval",
  archiveId?: string,
): Promise<string> {
  const body: Record<string, unknown> = {
    jobParameters: { Type: type },
  };
  if (archiveId && type === "archive-retrieval") {
    (body.jobParameters as Record<string, unknown>).ArchiveId = archiveId;
  }
  const r = await request<{ JobId: string }>(
    "InitiateJob",
    "POST",
    `/${ACCT}/vaults/${encodeURIComponent(vaultName)}/jobs`,
    body,
  );
  return r.JobId;
}
