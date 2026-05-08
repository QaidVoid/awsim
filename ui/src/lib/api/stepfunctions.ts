/**
 * Step Functions API client.
 *
 * Wraps the AWSim Step Functions JSON-1.0 API
 * (`AWSStepFunctions.<Action>`) with typed, camel-cased shapes for
 * state machines, executions, and history events.
 */

const ENDPOINT = "http://localhost:4566";
const FAKE_DATE = new Date().toISOString().slice(0, 10).replace(/-/g, "");
const TARGET_PREFIX = "AWSStepFunctions";

function authHeader(): string {
  return `AWS4-HMAC-SHA256 Credential=awsim-admin/${FAKE_DATE}/us-east-1/states/aws4_request, SignedHeaders=host;x-amz-date, Signature=fakesignature`;
}

function amzDate(): string {
  return new Date().toISOString().replace(/[:-]/g, "").slice(0, 15) + "Z";
}

async function sfnCall<T>(action: string, body: unknown = {}): Promise<T> {
  const res = await fetch(`${ENDPOINT}/`, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.0",
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

export interface StateMachine {
  arn: string;
  name: string;
  type: string;
  creationDate: string;
}

export interface StateMachineDetail extends StateMachine {
  definition: string;
  roleArn: string;
  status: string;
}

export interface Execution {
  arn: string;
  name: string;
  stateMachineArn: string;
  status: string;
  startDate: string;
  stopDate?: string;
}

export interface ExecutionDetail extends Execution {
  input: string;
  output: string;
  error?: string;
  cause?: string;
}

export interface HistoryEvent {
  id: number;
  type: string;
  timestamp: number;
  previousEventId?: number;
  stateName?: string;
  input?: string;
  output?: string;
  error?: string;
  cause?: string;
}

interface RawStateMachine {
  stateMachineArn?: string;
  name?: string;
  type?: string;
  creationDate?: number;
}

interface RawExecution {
  executionArn?: string;
  name?: string;
  stateMachineArn?: string;
  status?: string;
  startDate?: number;
  stopDate?: number;
}

interface RawHistoryEvent {
  id?: number;
  type?: string;
  timestamp?: number;
  previousEventId?: number;
  stateEnteredEventDetails?: { name?: string; input?: string };
  stateExitedEventDetails?: { name?: string; output?: string };
  taskScheduledEventDetails?: { resource?: string; parameters?: string };
  taskStartedEventDetails?: { resource?: string };
  taskSucceededEventDetails?: { output?: string };
  taskFailedEventDetails?: { error?: string; cause?: string };
  executionStartedEventDetails?: { input?: string };
  executionSucceededEventDetails?: { output?: string };
  executionFailedEventDetails?: { error?: string; cause?: string };
  executionAbortedEventDetails?: { error?: string; cause?: string };
  executionTimedOutEventDetails?: { error?: string; cause?: string };
}

function isoFromTs(ts?: number): string {
  if (!ts) return "";
  // Step Functions returns seconds-since-epoch (sometimes fractional).
  return new Date(ts < 1e12 ? ts * 1000 : ts).toISOString();
}

function mapStateMachine(raw: RawStateMachine): StateMachine {
  return {
    arn: raw.stateMachineArn ?? "",
    name: raw.name ?? "",
    type: raw.type ?? "STANDARD",
    creationDate: isoFromTs(raw.creationDate),
  };
}

function mapExecution(raw: RawExecution): Execution {
  return {
    arn: raw.executionArn ?? "",
    name: raw.name ?? "",
    stateMachineArn: raw.stateMachineArn ?? "",
    status: raw.status ?? "",
    startDate: isoFromTs(raw.startDate),
    stopDate: raw.stopDate ? isoFromTs(raw.stopDate) : undefined,
  };
}

function mapHistoryEvent(raw: RawHistoryEvent): HistoryEvent {
  const enteredName = raw.stateEnteredEventDetails?.name;
  const exitedName = raw.stateExitedEventDetails?.name;
  const stateName = enteredName ?? exitedName;
  const input =
    raw.executionStartedEventDetails?.input ??
    raw.stateEnteredEventDetails?.input;
  const output =
    raw.executionSucceededEventDetails?.output ??
    raw.stateExitedEventDetails?.output ??
    raw.taskSucceededEventDetails?.output;
  const error =
    raw.executionFailedEventDetails?.error ??
    raw.executionAbortedEventDetails?.error ??
    raw.executionTimedOutEventDetails?.error ??
    raw.taskFailedEventDetails?.error;
  const cause =
    raw.executionFailedEventDetails?.cause ??
    raw.executionAbortedEventDetails?.cause ??
    raw.executionTimedOutEventDetails?.cause ??
    raw.taskFailedEventDetails?.cause;

  return {
    id: raw.id ?? 0,
    type: raw.type ?? "",
    timestamp: raw.timestamp
      ? raw.timestamp < 1e12
        ? raw.timestamp * 1000
        : raw.timestamp
      : 0,
    previousEventId: raw.previousEventId,
    stateName,
    input,
    output,
    error,
    cause,
  };
}

// -- Operations --

export async function listStateMachines(): Promise<{
  stateMachines: StateMachine[];
}> {
  const data = await sfnCall<{ stateMachines?: RawStateMachine[] }>(
    "ListStateMachines",
  );
  return {
    stateMachines: (data.stateMachines ?? []).map(mapStateMachine),
  };
}

export async function describeStateMachine(
  arn: string,
): Promise<StateMachineDetail> {
  const data = await sfnCall<{
    stateMachineArn?: string;
    name?: string;
    type?: string;
    definition?: string;
    roleArn?: string;
    status?: string;
    creationDate?: number;
  }>("DescribeStateMachine", { stateMachineArn: arn });
  return {
    arn: data.stateMachineArn ?? arn,
    name: data.name ?? "",
    type: data.type ?? "STANDARD",
    creationDate: isoFromTs(data.creationDate),
    definition: data.definition ?? "{}",
    roleArn: data.roleArn ?? "",
    status: data.status ?? "",
  };
}

export interface CreateStateMachineInput {
  name: string;
  definition: string;
  type?: "STANDARD" | "EXPRESS";
  roleArn?: string;
}

export async function createStateMachine(
  input: CreateStateMachineInput,
): Promise<{ arn: string }> {
  const data = await sfnCall<{ stateMachineArn?: string }>(
    "CreateStateMachine",
    {
      name: input.name,
      definition: input.definition,
      type: input.type ?? "STANDARD",
      roleArn: input.roleArn ?? "arn:aws:iam::000000000000:role/exec",
    },
  );
  return { arn: data.stateMachineArn ?? "" };
}

export async function deleteStateMachine(arn: string): Promise<void> {
  await sfnCall("DeleteStateMachine", { stateMachineArn: arn });
}

export async function listExecutions(
  stateMachineArn: string,
): Promise<{ executions: Execution[] }> {
  const data = await sfnCall<{ executions?: RawExecution[] }>(
    "ListExecutions",
    { stateMachineArn },
  );
  return {
    executions: (data.executions ?? []).map(mapExecution),
  };
}

export async function describeExecution(arn: string): Promise<ExecutionDetail> {
  const data = await sfnCall<
    RawExecution & {
      input?: string;
      output?: string;
      error?: string;
      cause?: string;
    }
  >("DescribeExecution", { executionArn: arn });
  return {
    ...mapExecution(data),
    input: data.input ?? "",
    output: data.output ?? "",
    error: data.error,
    cause: data.cause,
  };
}

export async function getExecutionHistory(
  arn: string,
  reverse = false,
): Promise<{ events: HistoryEvent[] }> {
  const data = await sfnCall<{ events?: RawHistoryEvent[] }>(
    "GetExecutionHistory",
    { executionArn: arn, reverseOrder: reverse },
  );
  return { events: (data.events ?? []).map(mapHistoryEvent) };
}

export async function startExecution(
  stateMachineArn: string,
  input: string,
  name?: string,
): Promise<{ executionArn: string; startDate: string }> {
  const body: Record<string, unknown> = {
    stateMachineArn,
    input: input || "{}",
  };
  if (name) body["name"] = name;
  const data = await sfnCall<{ executionArn?: string; startDate?: number }>(
    "StartExecution",
    body,
  );
  return {
    executionArn: data.executionArn ?? "",
    startDate: isoFromTs(data.startDate),
  };
}

export async function stopExecution(
  executionArn: string,
  error?: string,
  cause?: string,
): Promise<void> {
  const body: Record<string, unknown> = { executionArn };
  if (error) body["error"] = error;
  if (cause) body["cause"] = cause;
  await sfnCall("StopExecution", body);
}
