/**
 * AWS Batch API client.
 *
 * LocalStack exposes Batch via REST endpoints under `/v1/<action>` with
 * camel-cased JSON bodies. This module wraps the operations consumed by
 * the UI and normalises shapes.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "batch";

async function batchRequest<T>(
  action: string,
  path: string,
  body: unknown = {},
  method: "POST" | "GET" = "POST",
): Promise<T> {
  const res = await loggedFetch(SERVICE, action, method, `${ENDPOINT}${path}`, {
    method,
    headers: {
      "Content-Type": "application/json",
      Authorization: authHeader(SERVICE),
      "X-Amz-Date": amzDate(),
    },
    body: method === "GET" ? undefined : JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`HTTP ${res.status}: ${text || res.statusText}`);
  }
  const text = await res.text();
  return text ? (JSON.parse(text) as T) : ({} as T);
}

// -- Types --

export interface ComputeEnvironment {
  computeEnvironmentName: string;
  computeEnvironmentArn: string;
  type: string;
  state: string;
  status: string;
  statusReason?: string;
  serviceRole?: string;
}

export interface JobQueue {
  jobQueueName: string;
  jobQueueArn: string;
  priority: number;
  state: string;
  status: string;
  statusReason?: string;
  computeEnvironmentOrder: { order: number; computeEnvironment: string }[];
}

export interface JobDefinition {
  jobDefinitionName: string;
  jobDefinitionArn: string;
  revision: number;
  status: string;
  type: string;
  containerImage?: string;
  vcpus?: number;
  memory?: number;
}

export interface JobSummary {
  jobId: string;
  jobName: string;
  status: string;
  statusReason?: string;
  createdAt?: number;
  startedAt?: number;
  stoppedAt?: number;
  jobDefinition?: string;
  jobQueue?: string;
  container?: { image?: string; exitCode?: number; reason?: string };
}

// -- Raw shapes --

interface RawComputeEnv {
  computeEnvironmentName?: string;
  computeEnvironmentArn?: string;
  type?: string;
  state?: string;
  status?: string;
  statusReason?: string;
  serviceRole?: string;
}

interface RawJobQueue {
  jobQueueName?: string;
  jobQueueArn?: string;
  priority?: number;
  state?: string;
  status?: string;
  statusReason?: string;
  computeEnvironmentOrder?: { order?: number; computeEnvironment?: string }[];
}

interface RawJobDefinition {
  jobDefinitionName?: string;
  jobDefinitionArn?: string;
  revision?: number;
  status?: string;
  type?: string;
  containerProperties?: {
    image?: string;
    vcpus?: number;
    memory?: number;
  };
}

interface RawJob {
  jobId?: string;
  jobName?: string;
  status?: string;
  statusReason?: string;
  createdAt?: number;
  startedAt?: number;
  stoppedAt?: number;
  jobDefinition?: string;
  jobQueue?: string;
  container?: { image?: string; exitCode?: number; reason?: string };
}

// -- Operations --

export async function describeComputeEnvironments(): Promise<{
  computeEnvironments: ComputeEnvironment[];
}> {
  const data = await batchRequest<{ computeEnvironments?: RawComputeEnv[] }>(
    "DescribeComputeEnvironments",
    "/v1/describecomputeenvironments",
  );
  return {
    computeEnvironments: (data.computeEnvironments ?? []).map((c) => ({
      computeEnvironmentName: c.computeEnvironmentName ?? "",
      computeEnvironmentArn: c.computeEnvironmentArn ?? "",
      type: c.type ?? "",
      state: c.state ?? "",
      status: c.status ?? "",
      statusReason: c.statusReason,
      serviceRole: c.serviceRole,
    })),
  };
}

export async function describeJobQueues(): Promise<{ jobQueues: JobQueue[] }> {
  const data = await batchRequest<{ jobQueues?: RawJobQueue[] }>(
    "DescribeJobQueues",
    "/v1/describejobqueues",
  );
  return {
    jobQueues: (data.jobQueues ?? []).map((q) => ({
      jobQueueName: q.jobQueueName ?? "",
      jobQueueArn: q.jobQueueArn ?? "",
      priority: q.priority ?? 0,
      state: q.state ?? "",
      status: q.status ?? "",
      statusReason: q.statusReason,
      computeEnvironmentOrder: (q.computeEnvironmentOrder ?? []).map((o) => ({
        order: o.order ?? 0,
        computeEnvironment: o.computeEnvironment ?? "",
      })),
    })),
  };
}

export async function describeJobDefinitions(): Promise<{
  jobDefinitions: JobDefinition[];
}> {
  const data = await batchRequest<{ jobDefinitions?: RawJobDefinition[] }>(
    "DescribeJobDefinitions",
    "/v1/describejobdefinitions",
  );
  return {
    jobDefinitions: (data.jobDefinitions ?? []).map((d) => ({
      jobDefinitionName: d.jobDefinitionName ?? "",
      jobDefinitionArn: d.jobDefinitionArn ?? "",
      revision: d.revision ?? 0,
      status: d.status ?? "",
      type: d.type ?? "",
      containerImage: d.containerProperties?.image,
      vcpus: d.containerProperties?.vcpus,
      memory: d.containerProperties?.memory,
    })),
  };
}

export async function listJobs(
  jobQueue?: string,
  jobStatus?: string,
): Promise<{ jobs: JobSummary[] }> {
  const body: Record<string, string> = {};
  if (jobQueue) body.jobQueue = jobQueue;
  if (jobStatus) body.jobStatus = jobStatus;
  const data = await batchRequest<{ jobSummaryList?: RawJob[] }>(
    "ListJobs",
    "/v1/listjobs",
    body,
  );
  return {
    jobs: (data.jobSummaryList ?? []).map((j) => ({
      jobId: j.jobId ?? "",
      jobName: j.jobName ?? "",
      status: j.status ?? "",
      statusReason: j.statusReason,
      createdAt: j.createdAt,
      startedAt: j.startedAt,
      stoppedAt: j.stoppedAt,
      jobDefinition: j.jobDefinition,
      jobQueue: j.jobQueue,
      container: j.container,
    })),
  };
}

export async function describeJob(jobId: string): Promise<JobSummary | null> {
  const data = await batchRequest<{ jobs?: RawJob[] }>(
    "DescribeJobs",
    "/v1/describejobs",
    { jobs: [jobId] },
  );
  const j = data.jobs?.[0];
  if (!j) return null;
  return {
    jobId: j.jobId ?? "",
    jobName: j.jobName ?? "",
    status: j.status ?? "",
    statusReason: j.statusReason,
    createdAt: j.createdAt,
    startedAt: j.startedAt,
    stoppedAt: j.stoppedAt,
    jobDefinition: j.jobDefinition,
    jobQueue: j.jobQueue,
    container: j.container,
  };
}

export interface SubmitJobInput {
  jobName: string;
  jobQueue: string;
  jobDefinition: string;
}

export async function submitJob(
  input: SubmitJobInput,
): Promise<{ jobId: string }> {
  const data = await batchRequest<{ jobId?: string }>(
    "SubmitJob",
    "/v1/submitjob",
    input,
  );
  return { jobId: data.jobId ?? "" };
}

export async function terminateJob(
  jobId: string,
  reason: string,
): Promise<void> {
  await batchRequest("TerminateJob", "/v1/terminatejob", { jobId, reason });
}

export async function cancelJob(jobId: string, reason: string): Promise<void> {
  await batchRequest("CancelJob", "/v1/canceljob", { jobId, reason });
}

// -- Helpers --

export function jobStatusVariant(
  status: string,
): "default" | "secondary" | "destructive" | "outline" {
  if (status === "SUCCEEDED" || status === "VALID" || status === "ENABLED")
    return "default";
  if (status === "FAILED") return "destructive";
  if (status === "RUNNING" || status === "RUNNABLE" || status === "STARTING")
    return "secondary";
  return "outline";
}

export function shortArn(arn: string): string {
  return arn.split("/").pop() ?? arn;
}
