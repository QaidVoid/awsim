/**
 * ECS API client.
 *
 * Wraps the LocalStack-compatible ECS JSON-1.1 API
 * (`AmazonEC2ContainerServiceV20141113.<Action>`) with strongly typed
 * camel-cased shapes for cluster / service / task / task definition
 * operations used by the UI.
 */

const ENDPOINT = "http://localhost:4566";
const FAKE_DATE = new Date().toISOString().slice(0, 10).replace(/-/g, "");
const TARGET_PREFIX = "AmazonEC2ContainerServiceV20141113";

function authHeader(): string {
  return `AWS4-HMAC-SHA256 Credential=test/${FAKE_DATE}/us-east-1/ecs/aws4_request, SignedHeaders=host;x-amz-date, Signature=fakesignature`;
}

function amzDate(): string {
  return new Date().toISOString().replace(/[:-]/g, "").slice(0, 15) + "Z";
}

async function ecsCall<T>(action: string, body: unknown = {}): Promise<T> {
  const res = await fetch(`${ENDPOINT}/`, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.1",
      "X-Amz-Target": `${TARGET_PREFIX}.${action}`,
      Authorization: authHeader(),
      "X-Amz-Date": amzDate(),
    },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`HTTP ${res.status}: ${text || res.statusText}`);
  }
  const text = await res.text();
  return text ? (JSON.parse(text) as T) : ({} as T);
}

// -- Types --

export interface Cluster {
  arn: string;
  name: string;
  status: string;
  registeredContainerInstances: number;
  runningTasks: number;
  pendingTasks: number;
  activeServices: number;
}

export interface ServiceSummary {
  arn: string;
  name: string;
  cluster: string;
  status: string;
  desiredCount: number;
  runningCount: number;
  pendingCount: number;
  taskDefinition: string;
  launchType: string;
  createdAt: string;
}

export interface ContainerSummary {
  name: string;
  image: string;
  lastStatus: string;
  exitCode?: number;
  reason?: string;
}

export interface Task {
  arn: string;
  cluster: string;
  taskDefinitionArn: string;
  lastStatus: string;
  desiredStatus: string;
  launchType: string;
  cpu: string;
  memory: string;
  startedAt?: string;
  stoppedAt?: string;
  stoppedReason?: string;
  containers: ContainerSummary[];
}

export interface TaskDefinition {
  arn: string;
  family: string;
  revision: number;
  status: string;
  cpu?: string;
  memory?: string;
  networkMode?: string;
  containers: { name: string; image: string }[];
}

// -- Raw types (subset of LocalStack response shapes) --

interface RawCluster {
  clusterArn?: string;
  clusterName?: string;
  status?: string;
  registeredContainerInstancesCount?: number;
  runningTasksCount?: number;
  pendingTasksCount?: number;
  activeServicesCount?: number;
}

interface RawService {
  serviceArn?: string;
  serviceName?: string;
  clusterArn?: string;
  status?: string;
  desiredCount?: number;
  runningCount?: number;
  pendingCount?: number;
  taskDefinition?: string;
  launchType?: string;
  createdAt?: number;
}

interface RawContainer {
  name?: string;
  image?: string;
  lastStatus?: string;
  exitCode?: number;
  reason?: string;
}

interface RawTask {
  taskArn?: string;
  clusterArn?: string;
  taskDefinitionArn?: string;
  lastStatus?: string;
  desiredStatus?: string;
  launchType?: string;
  cpu?: string;
  memory?: string;
  startedAt?: number;
  stoppedAt?: number;
  stoppedReason?: string;
  containers?: RawContainer[];
}

interface RawTaskDefinition {
  taskDefinitionArn?: string;
  family?: string;
  revision?: number;
  status?: string;
  cpu?: string;
  memory?: string;
  networkMode?: string;
  containerDefinitions?: { name?: string; image?: string }[];
}

function toIso(ts?: number): string | undefined {
  return ts ? new Date(ts * 1000).toISOString() : undefined;
}

function mapCluster(raw: RawCluster): Cluster {
  return {
    arn: raw.clusterArn ?? "",
    name: raw.clusterName ?? raw.clusterArn?.split("/").pop() ?? "",
    status: raw.status ?? "",
    registeredContainerInstances: raw.registeredContainerInstancesCount ?? 0,
    runningTasks: raw.runningTasksCount ?? 0,
    pendingTasks: raw.pendingTasksCount ?? 0,
    activeServices: raw.activeServicesCount ?? 0,
  };
}

function mapService(raw: RawService): ServiceSummary {
  return {
    arn: raw.serviceArn ?? "",
    name: raw.serviceName ?? "",
    cluster: raw.clusterArn ?? "",
    status: raw.status ?? "",
    desiredCount: raw.desiredCount ?? 0,
    runningCount: raw.runningCount ?? 0,
    pendingCount: raw.pendingCount ?? 0,
    taskDefinition: raw.taskDefinition ?? "",
    launchType: raw.launchType ?? "",
    createdAt: toIso(raw.createdAt) ?? "",
  };
}

function mapContainer(raw: RawContainer): ContainerSummary {
  return {
    name: raw.name ?? "",
    image: raw.image ?? "",
    lastStatus: raw.lastStatus ?? "",
    exitCode: raw.exitCode,
    reason: raw.reason,
  };
}

function mapTask(raw: RawTask): Task {
  return {
    arn: raw.taskArn ?? "",
    cluster: raw.clusterArn ?? "",
    taskDefinitionArn: raw.taskDefinitionArn ?? "",
    lastStatus: raw.lastStatus ?? "",
    desiredStatus: raw.desiredStatus ?? "",
    launchType: raw.launchType ?? "",
    cpu: raw.cpu ?? "",
    memory: raw.memory ?? "",
    startedAt: toIso(raw.startedAt),
    stoppedAt: toIso(raw.stoppedAt),
    stoppedReason: raw.stoppedReason,
    containers: (raw.containers ?? []).map(mapContainer),
  };
}

function mapTaskDefinition(raw: RawTaskDefinition): TaskDefinition {
  return {
    arn: raw.taskDefinitionArn ?? "",
    family: raw.family ?? "",
    revision: raw.revision ?? 0,
    status: raw.status ?? "",
    cpu: raw.cpu,
    memory: raw.memory,
    networkMode: raw.networkMode,
    containers: (raw.containerDefinitions ?? []).map((c) => ({
      name: c.name ?? "",
      image: c.image ?? "",
    })),
  };
}

// -- Operations --

export async function listClusters(): Promise<{ clusters: Cluster[] }> {
  const list = await ecsCall<{ clusterArns?: string[] }>("ListClusters");
  const arns = list.clusterArns ?? [];
  if (arns.length === 0) return { clusters: [] };
  const detail = await ecsCall<{ clusters?: RawCluster[] }>(
    "DescribeClusters",
    {
      clusters: arns,
    },
  );
  return { clusters: (detail.clusters ?? []).map(mapCluster) };
}

export async function createCluster(name: string): Promise<void> {
  await ecsCall("CreateCluster", { clusterName: name });
}

export async function deleteCluster(clusterArn: string): Promise<void> {
  await ecsCall("DeleteCluster", { cluster: clusterArn });
}

export async function listServices(
  clusterArn: string,
): Promise<{ services: ServiceSummary[] }> {
  const list = await ecsCall<{ serviceArns?: string[] }>("ListServices", {
    cluster: clusterArn,
  });
  const arns = list.serviceArns ?? [];
  if (arns.length === 0) return { services: [] };
  const detail = await ecsCall<{ services?: RawService[] }>(
    "DescribeServices",
    {
      cluster: clusterArn,
      services: arns,
    },
  );
  return { services: (detail.services ?? []).map(mapService) };
}

export async function listTasks(
  clusterArn: string,
): Promise<{ tasks: Task[] }> {
  const list = await ecsCall<{ taskArns?: string[] }>("ListTasks", {
    cluster: clusterArn,
  });
  const arns = list.taskArns ?? [];
  if (arns.length === 0) return { tasks: [] };
  const detail = await ecsCall<{ tasks?: RawTask[] }>("DescribeTasks", {
    cluster: clusterArn,
    tasks: arns,
  });
  return { tasks: (detail.tasks ?? []).map(mapTask) };
}

export async function describeTask(
  clusterArn: string,
  taskArn: string,
): Promise<Task | null> {
  const detail = await ecsCall<{ tasks?: RawTask[] }>("DescribeTasks", {
    cluster: clusterArn,
    tasks: [taskArn],
  });
  const t = detail.tasks?.[0];
  return t ? mapTask(t) : null;
}

export async function listTaskDefinitions(): Promise<{
  taskDefinitionArns: string[];
}> {
  const data = await ecsCall<{ taskDefinitionArns?: string[] }>(
    "ListTaskDefinitions",
  );
  return { taskDefinitionArns: data.taskDefinitionArns ?? [] };
}

export async function describeTaskDefinition(
  arn: string,
): Promise<TaskDefinition | null> {
  const detail = await ecsCall<{ taskDefinition?: RawTaskDefinition }>(
    "DescribeTaskDefinition",
    { taskDefinition: arn },
  );
  return detail.taskDefinition
    ? mapTaskDefinition(detail.taskDefinition)
    : null;
}

export interface RunTaskInput {
  clusterArn: string;
  taskDefinition: string;
  count?: number;
  launchType?: string;
}

export async function runTask(input: RunTaskInput): Promise<Task[]> {
  const detail = await ecsCall<{ tasks?: RawTask[] }>("RunTask", {
    cluster: input.clusterArn,
    taskDefinition: input.taskDefinition,
    count: input.count ?? 1,
    launchType: input.launchType ?? "FARGATE",
  });
  return (detail.tasks ?? []).map(mapTask);
}

export function clusterShortName(arn: string): string {
  return arn.split("/").pop() ?? arn;
}

export function taskDefShortName(arn: string): string {
  return arn.split("/").pop() ?? arn;
}
