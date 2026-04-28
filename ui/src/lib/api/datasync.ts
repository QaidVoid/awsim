/**
 * AWS DataSync API client.
 *
 * AWSim uses the JSON-1.1 protocol with `X-Amz-Target:
 * FmrsService.<Action>` for DataSync. All shapes are normalised to the
 * camel-cased forms consumed by the UI components.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "datasync";
const TARGET_PREFIX = "FmrsService";

async function dsRequest<T>(action: string, body: unknown = {}): Promise<T> {
  const res = await loggedFetch(SERVICE, action, "POST", `${ENDPOINT}/`, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.1",
      "X-Amz-Target": `${TARGET_PREFIX}.${action}`,
      Authorization: authHeader(SERVICE),
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

export interface Location {
  locationArn: string;
  locationUri: string;
  type?: string;
}

export interface LocationS3Detail {
  locationArn: string;
  locationUri: string;
  s3BucketArn?: string;
  subdirectory?: string;
  s3StorageClass?: string;
  bucketAccessRoleArn?: string;
  creationTime?: number;
}

export interface Task {
  taskArn: string;
  name?: string;
  status: string;
  sourceLocationArn?: string;
  destinationLocationArn?: string;
  cloudWatchLogGroupArn?: string;
  errorCode?: string;
  errorDetail?: string;
  creationTime?: number;
}

export interface TaskExecution {
  taskExecutionArn: string;
  status: string;
  startTime?: number;
  estimatedFilesToTransfer?: number;
  filesTransferred?: number;
  bytesTransferred?: number;
  bytesWritten?: number;
}

// -- Raw shapes --

interface RawLocation {
  LocationArn?: string;
  LocationUri?: string;
}

interface RawTask {
  TaskArn?: string;
  Name?: string;
  Status?: string;
  SourceLocationArn?: string;
  DestinationLocationArn?: string;
  CloudWatchLogGroupArn?: string;
  ErrorCode?: string;
  ErrorDetail?: string;
  CreationTime?: number;
}

interface RawExecution {
  TaskExecutionArn?: string;
  Status?: string;
  StartTime?: number;
  EstimatedFilesToTransfer?: number;
  FilesTransferred?: number;
  BytesTransferred?: number;
  BytesWritten?: number;
}

function inferLocationType(uri: string): string {
  const m = uri.match(/^(\w+):\/\//);
  return m ? m[1].toUpperCase() : "";
}

// -- Operations --

export async function listLocations(): Promise<{ locations: Location[] }> {
  const data = await dsRequest<{ Locations?: RawLocation[] }>("ListLocations");
  return {
    locations: (data.Locations ?? []).map((l) => ({
      locationArn: l.LocationArn ?? "",
      locationUri: l.LocationUri ?? "",
      type: inferLocationType(l.LocationUri ?? ""),
    })),
  };
}

export async function describeLocationS3(
  locationArn: string,
): Promise<LocationS3Detail | null> {
  const data = await dsRequest<{
    LocationArn?: string;
    LocationUri?: string;
    S3BucketArn?: string;
    Subdirectory?: string;
    S3StorageClass?: string;
    S3Config?: { BucketAccessRoleArn?: string };
    CreationTime?: number;
  }>("DescribeLocationS3", { LocationArn: locationArn });
  if (!data.LocationArn) return null;
  return {
    locationArn: data.LocationArn,
    locationUri: data.LocationUri ?? "",
    s3BucketArn: data.S3BucketArn,
    subdirectory: data.Subdirectory,
    s3StorageClass: data.S3StorageClass,
    bucketAccessRoleArn: data.S3Config?.BucketAccessRoleArn,
    creationTime: data.CreationTime,
  };
}

export async function listTasks(): Promise<{ tasks: Task[] }> {
  const data = await dsRequest<{ Tasks?: RawTask[] }>("ListTasks");
  return {
    tasks: (data.Tasks ?? []).map((t) => ({
      taskArn: t.TaskArn ?? "",
      name: t.Name,
      status: t.Status ?? "",
    })),
  };
}

export async function describeTask(taskArn: string): Promise<Task | null> {
  const data = await dsRequest<RawTask>("DescribeTask", { TaskArn: taskArn });
  if (!data.TaskArn) return null;
  return {
    taskArn: data.TaskArn,
    name: data.Name,
    status: data.Status ?? "",
    sourceLocationArn: data.SourceLocationArn,
    destinationLocationArn: data.DestinationLocationArn,
    cloudWatchLogGroupArn: data.CloudWatchLogGroupArn,
    errorCode: data.ErrorCode,
    errorDetail: data.ErrorDetail,
    creationTime: data.CreationTime,
  };
}

export async function listTaskExecutions(
  taskArn?: string,
): Promise<{ executions: TaskExecution[] }> {
  const body = taskArn ? { TaskArn: taskArn } : {};
  const data = await dsRequest<{ TaskExecutions?: RawExecution[] }>(
    "ListTaskExecutions",
    body,
  );
  return {
    executions: (data.TaskExecutions ?? []).map((e) => ({
      taskExecutionArn: e.TaskExecutionArn ?? "",
      status: e.Status ?? "",
    })),
  };
}

export async function describeTaskExecution(
  taskExecutionArn: string,
): Promise<TaskExecution | null> {
  const data = await dsRequest<RawExecution>("DescribeTaskExecution", {
    TaskExecutionArn: taskExecutionArn,
  });
  if (!data.TaskExecutionArn) return null;
  return {
    taskExecutionArn: data.TaskExecutionArn,
    status: data.Status ?? "",
    startTime: data.StartTime,
    estimatedFilesToTransfer: data.EstimatedFilesToTransfer,
    filesTransferred: data.FilesTransferred,
    bytesTransferred: data.BytesTransferred,
    bytesWritten: data.BytesWritten,
  };
}

export async function startTaskExecution(
  taskArn: string,
): Promise<{ taskExecutionArn: string }> {
  const data = await dsRequest<{ TaskExecutionArn?: string }>(
    "StartTaskExecution",
    { TaskArn: taskArn },
  );
  return { taskExecutionArn: data.TaskExecutionArn ?? "" };
}

export interface CreateLocationS3Input {
  s3BucketArn: string;
  subdirectory: string;
  bucketAccessRoleArn: string;
  s3StorageClass?: string;
}

export async function createLocationS3(
  input: CreateLocationS3Input,
): Promise<{ locationArn: string }> {
  const data = await dsRequest<{ LocationArn?: string }>("CreateLocationS3", {
    S3BucketArn: input.s3BucketArn,
    Subdirectory: input.subdirectory || "/",
    S3Config: { BucketAccessRoleArn: input.bucketAccessRoleArn },
    S3StorageClass: input.s3StorageClass,
  });
  return { locationArn: data.LocationArn ?? "" };
}

export async function deleteLocation(locationArn: string): Promise<void> {
  await dsRequest("DeleteLocation", { LocationArn: locationArn });
}

// -- Helpers --

export function dsStatusVariant(
  status: string,
): "default" | "secondary" | "destructive" | "outline" {
  if (status === "AVAILABLE" || status === "SUCCESS") return "default";
  if (status === "ERROR") return "destructive";
  if (
    status === "QUEUED" ||
    status === "LAUNCHING" ||
    status === "PREPARING" ||
    status === "TRANSFERRING" ||
    status === "VERIFYING"
  )
    return "secondary";
  return "outline";
}
