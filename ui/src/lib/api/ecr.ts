/**
 * ECR API client.
 *
 * Wraps the LocalStack-compatible ECR JSON-1.1 API
 * (`AmazonEC2ContainerRegistry_V20150921.<Action>`) with strongly typed
 * camel-cased shapes for repository / image operations used by the UI.
 */

import { ENDPOINT, amzDate, authHeader, loggedFetch } from "$lib/aws";

const SERVICE = "ecr";
const TARGET_PREFIX = "AmazonEC2ContainerRegistry_V20150921";

// ---------- Types ----------

export interface Repository {
  repositoryName: string;
  repositoryUri: string;
  repositoryArn: string;
  registryId: string;
  createdAt: string;
  imageTagMutability: string;
  scanOnPush: boolean;
}

export interface Image {
  registryId: string;
  repositoryName: string;
  imageDigest: string;
  imageTags: string[];
  imageSizeInBytes: number;
  imagePushedAt: string;
  artifactMediaType: string;
  imageManifestMediaType: string;
}

// ---------- Internal request helper ----------

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
    throw new Error(`ECR ${action} failed (HTTP ${res.status}): ${text}`);
  }
  return text ? (JSON.parse(text) as T) : ({} as T);
}

// ---------- Raw shapes ----------

interface RawRepository {
  repositoryName?: string;
  repositoryUri?: string;
  repositoryArn?: string;
  registryId?: string;
  createdAt?: number;
  imageTagMutability?: string;
  imageScanningConfiguration?: { scanOnPush?: boolean };
}

interface RawImageDetail {
  registryId?: string;
  repositoryName?: string;
  imageDigest?: string;
  imageTags?: string[];
  imageSizeInBytes?: number;
  imagePushedAt?: number;
  artifactMediaType?: string;
  imageManifestMediaType?: string;
}

function toIso(ts?: number): string {
  return ts ? new Date(ts * 1000).toISOString() : "";
}

function mapRepo(raw: RawRepository): Repository {
  return {
    repositoryName: raw.repositoryName ?? "",
    repositoryUri: raw.repositoryUri ?? "",
    repositoryArn: raw.repositoryArn ?? "",
    registryId: raw.registryId ?? "",
    createdAt: toIso(raw.createdAt),
    imageTagMutability: raw.imageTagMutability ?? "MUTABLE",
    scanOnPush: raw.imageScanningConfiguration?.scanOnPush ?? false,
  };
}

function mapImage(raw: RawImageDetail): Image {
  return {
    registryId: raw.registryId ?? "",
    repositoryName: raw.repositoryName ?? "",
    imageDigest: raw.imageDigest ?? "",
    imageTags: raw.imageTags ?? [],
    imageSizeInBytes: raw.imageSizeInBytes ?? 0,
    imagePushedAt: toIso(raw.imagePushedAt),
    artifactMediaType: raw.artifactMediaType ?? "",
    imageManifestMediaType: raw.imageManifestMediaType ?? "",
  };
}

// ---------- Operations ----------

export async function describeRepositories(): Promise<Repository[]> {
  const data = await request<{ repositories?: RawRepository[] }>(
    "DescribeRepositories",
  );
  return (data.repositories ?? []).map(mapRepo);
}

export interface CreateRepositoryInput {
  repositoryName: string;
  imageTagMutability?: "MUTABLE" | "IMMUTABLE";
  scanOnPush?: boolean;
}

export async function createRepository(
  input: CreateRepositoryInput,
): Promise<Repository> {
  const data = await request<{ repository?: RawRepository }>(
    "CreateRepository",
    {
      repositoryName: input.repositoryName,
      imageTagMutability: input.imageTagMutability ?? "MUTABLE",
      imageScanningConfiguration: {
        scanOnPush: input.scanOnPush ?? false,
      },
    },
  );
  return mapRepo(data.repository ?? {});
}

export async function deleteRepository(name: string, force = true): Promise<void> {
  await request("DeleteRepository", { repositoryName: name, force });
}

export async function describeImages(repositoryName: string): Promise<Image[]> {
  const data = await request<{ imageDetails?: RawImageDetail[] }>(
    "DescribeImages",
    { repositoryName },
  );
  return (data.imageDetails ?? []).map(mapImage);
}

export async function batchDeleteImage(
  repositoryName: string,
  imageDigests: string[],
): Promise<void> {
  await request("BatchDeleteImage", {
    repositoryName,
    imageIds: imageDigests.map((d) => ({ imageDigest: d })),
  });
}

export function shortDigest(digest: string): string {
  if (!digest) return "—";
  const idx = digest.indexOf(":");
  const hex = idx >= 0 ? digest.slice(idx + 1) : digest;
  return hex.length > 12 ? `${digest.slice(0, idx + 1)}${hex.slice(0, 12)}` : digest;
}
