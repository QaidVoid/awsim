/**
 * Typed Amazon QLDB API client. RestJson1.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "qldb";

export interface LedgerSummary {
  name: string;
  state: string;
  creationDateTime: number;
}

export interface Ledger extends LedgerSummary {
  arn: string;
  permissionsMode: string;
  deletionProtection: boolean;
  kmsKeyArn?: string;
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
  method: "GET" | "POST" | "PUT" | "DELETE" | "PATCH",
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
    throw new Error(`QLDB ${action} failed (HTTP ${res.status}): ${msg}`);
  }
  return (text ? JSON.parse(text) : {}) as T;
}

interface RawSummary {
  Name: string;
  State: string;
  CreationDateTime: number;
}

interface RawLedger extends RawSummary {
  Arn: string;
  PermissionsMode: string;
  DeletionProtection: boolean;
  KmsKeyArn?: string;
}

const fromSummary = (r: RawSummary): LedgerSummary => ({
  name: r.Name,
  state: r.State,
  creationDateTime: r.CreationDateTime,
});

const fromLedger = (r: RawLedger): Ledger => ({
  ...fromSummary(r),
  arn: r.Arn,
  permissionsMode: r.PermissionsMode,
  deletionProtection: r.DeletionProtection,
  kmsKeyArn: r.KmsKeyArn,
});

export async function listLedgers(): Promise<LedgerSummary[]> {
  const data = await request<{ Ledgers?: RawSummary[] }>(
    "ListLedgers",
    "GET",
    "/ledgers",
  );
  return (data.Ledgers ?? []).map(fromSummary);
}

export async function describeLedger(name: string): Promise<Ledger> {
  const r = await request<RawLedger>(
    "DescribeLedger",
    "GET",
    `/ledgers/${encodeURIComponent(name)}`,
  );
  return fromLedger(r);
}

export async function createLedger(
  name: string,
  permissionsMode: "ALLOW_ALL" | "STANDARD" = "STANDARD",
  deletionProtection = true,
): Promise<Ledger> {
  const r = await request<RawLedger>(
    "CreateLedger",
    "POST",
    "/ledgers",
    {
      Name: name,
      PermissionsMode: permissionsMode,
      DeletionProtection: deletionProtection,
    },
  );
  return fromLedger(r);
}

export async function updateLedgerProtection(
  name: string,
  deletionProtection: boolean,
): Promise<void> {
  await request<unknown>(
    "UpdateLedger",
    "PATCH",
    `/ledgers/${encodeURIComponent(name)}`,
    { DeletionProtection: deletionProtection },
  );
}

export async function deleteLedger(name: string): Promise<void> {
  await request<unknown>(
    "DeleteLedger",
    "DELETE",
    `/ledgers/${encodeURIComponent(name)}`,
  );
}
