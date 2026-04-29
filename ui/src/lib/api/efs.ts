/**
 * Typed EFS API client. AWSim exposes the standard EFS REST routes under
 * /2015-02-01.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "elasticfilesystem";

export interface FileSystem {
  fileSystemId: string;
  fileSystemArn: string;
  creationToken: string;
  creationTime: number;
  lifeCycleState: string;
  numberOfMountTargets: number;
  sizeInBytes: number;
  performanceMode: string;
  throughputMode: string;
  encrypted: boolean;
  kmsKeyId?: string;
  name?: string;
  tags: Record<string, string>;
}

export interface MountTarget {
  mountTargetId: string;
  fileSystemId: string;
  subnetId: string;
  lifeCycleState: string;
  ipAddress: string;
  networkInterfaceId: string;
  availabilityZoneName: string;
  vpcId: string;
}

export interface AccessPoint {
  accessPointId: string;
  accessPointArn: string;
  fileSystemId: string;
  name?: string;
  posixUser?: Record<string, unknown>;
  rootDirectory?: Record<string, unknown>;
  lifeCycleState: string;
  tags: Record<string, string>;
}

export interface CreateFileSystemInput {
  creationToken: string;
  name?: string;
  encrypted?: boolean;
  performanceMode?: string;
  throughputMode?: string;
  tags?: Record<string, string>;
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
    throw new Error(`EFS ${action} failed (HTTP ${res.status}): ${msg}`);
  }
  return (text ? JSON.parse(text) : {}) as T;
}

interface RawFileSystem {
  FileSystemId: string;
  FileSystemArn: string;
  CreationToken: string;
  CreationTime: number;
  LifeCycleState: string;
  NumberOfMountTargets: number;
  SizeInBytes: { Value: number };
  PerformanceMode: string;
  ThroughputMode: string;
  Encrypted: boolean;
  KmsKeyId?: string;
  Name?: string;
  Tags?: Array<{ Key: string; Value: string }>;
}

interface RawMountTarget {
  MountTargetId: string;
  FileSystemId: string;
  SubnetId: string;
  LifeCycleState: string;
  IpAddress: string;
  NetworkInterfaceId: string;
  AvailabilityZoneName: string;
  VpcId: string;
}

interface RawAccessPoint {
  AccessPointId: string;
  AccessPointArn: string;
  FileSystemId: string;
  Name?: string;
  PosixUser?: Record<string, unknown>;
  RootDirectory?: Record<string, unknown>;
  LifeCycleState: string;
  Tags?: Array<{ Key: string; Value: string }>;
}

function tagsFromArray(
  raw: Array<{ Key: string; Value: string }> | undefined,
): Record<string, string> {
  const out: Record<string, string> = {};
  for (const t of raw ?? []) out[t.Key] = t.Value;
  return out;
}

function fromRawFileSystem(r: RawFileSystem): FileSystem {
  return {
    fileSystemId: r.FileSystemId,
    fileSystemArn: r.FileSystemArn,
    creationToken: r.CreationToken,
    creationTime: r.CreationTime,
    lifeCycleState: r.LifeCycleState,
    numberOfMountTargets: r.NumberOfMountTargets,
    sizeInBytes: r.SizeInBytes?.Value ?? 0,
    performanceMode: r.PerformanceMode,
    throughputMode: r.ThroughputMode,
    encrypted: r.Encrypted,
    kmsKeyId: r.KmsKeyId,
    name: r.Name,
    tags: tagsFromArray(r.Tags),
  };
}

function fromRawMountTarget(r: RawMountTarget): MountTarget {
  return {
    mountTargetId: r.MountTargetId,
    fileSystemId: r.FileSystemId,
    subnetId: r.SubnetId,
    lifeCycleState: r.LifeCycleState,
    ipAddress: r.IpAddress,
    networkInterfaceId: r.NetworkInterfaceId,
    availabilityZoneName: r.AvailabilityZoneName,
    vpcId: r.VpcId,
  };
}

function fromRawAccessPoint(r: RawAccessPoint): AccessPoint {
  return {
    accessPointId: r.AccessPointId,
    accessPointArn: r.AccessPointArn,
    fileSystemId: r.FileSystemId,
    name: r.Name,
    posixUser: r.PosixUser,
    rootDirectory: r.RootDirectory,
    lifeCycleState: r.LifeCycleState,
    tags: tagsFromArray(r.Tags),
  };
}

export async function listFileSystems(): Promise<FileSystem[]> {
  const data = await request<{ FileSystems?: RawFileSystem[] }>(
    "DescribeFileSystems",
    "GET",
    "/2015-02-01/file-systems",
  );
  return (data.FileSystems ?? []).map(fromRawFileSystem);
}

export async function createFileSystem(
  input: CreateFileSystemInput,
): Promise<FileSystem> {
  const body: Record<string, unknown> = {
    CreationToken: input.creationToken,
  };
  if (input.encrypted !== undefined) body.Encrypted = input.encrypted;
  if (input.performanceMode) body.PerformanceMode = input.performanceMode;
  if (input.throughputMode) body.ThroughputMode = input.throughputMode;
  if (input.name || input.tags) {
    const tags: Array<{ Key: string; Value: string }> = [];
    if (input.name) tags.push({ Key: "Name", Value: input.name });
    for (const [k, v] of Object.entries(input.tags ?? {})) {
      if (k !== "Name") tags.push({ Key: k, Value: v });
    }
    body.Tags = tags;
  }
  const r = await request<RawFileSystem>(
    "CreateFileSystem",
    "POST",
    "/2015-02-01/file-systems",
    body,
  );
  return fromRawFileSystem(r);
}

export async function deleteFileSystem(id: string): Promise<void> {
  await request<unknown>(
    "DeleteFileSystem",
    "DELETE",
    `/2015-02-01/file-systems/${encodeURIComponent(id)}`,
  );
}

export async function listMountTargets(
  fileSystemId: string,
): Promise<MountTarget[]> {
  const data = await request<{ MountTargets?: RawMountTarget[] }>(
    "DescribeMountTargets",
    "GET",
    `/2015-02-01/mount-targets?FileSystemId=${encodeURIComponent(fileSystemId)}`,
  );
  return (data.MountTargets ?? []).map(fromRawMountTarget);
}

export async function createMountTarget(
  fileSystemId: string,
  subnetId: string,
  ipAddress?: string,
): Promise<MountTarget> {
  const body: Record<string, unknown> = {
    FileSystemId: fileSystemId,
    SubnetId: subnetId,
  };
  if (ipAddress) body.IpAddress = ipAddress;
  const r = await request<RawMountTarget>(
    "CreateMountTarget",
    "POST",
    "/2015-02-01/mount-targets",
    body,
  );
  return fromRawMountTarget(r);
}

export async function deleteMountTarget(id: string): Promise<void> {
  await request<unknown>(
    "DeleteMountTarget",
    "DELETE",
    `/2015-02-01/mount-targets/${encodeURIComponent(id)}`,
  );
}

export async function listAccessPoints(
  fileSystemId?: string,
): Promise<AccessPoint[]> {
  const qs = fileSystemId
    ? `?FileSystemId=${encodeURIComponent(fileSystemId)}`
    : "";
  const data = await request<{ AccessPoints?: RawAccessPoint[] }>(
    "DescribeAccessPoints",
    "GET",
    `/2015-02-01/access-points${qs}`,
  );
  return (data.AccessPoints ?? []).map(fromRawAccessPoint);
}

export async function deleteAccessPoint(id: string): Promise<void> {
  await request<unknown>(
    "DeleteAccessPoint",
    "DELETE",
    `/2015-02-01/access-points/${encodeURIComponent(id)}`,
  );
}
