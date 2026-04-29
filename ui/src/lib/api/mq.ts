/**
 * Typed Amazon MQ API client. RestJson1.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "mq";

export interface BrokerSummary {
  brokerId: string;
  brokerArn: string;
  brokerName: string;
  brokerState: string;
  deploymentMode: string;
  engineType: string;
  hostInstanceType: string;
  created: number;
}

export interface BrokerInstance {
  endpoints: string[];
  consoleURL?: string;
  ipAddress?: string;
}

export interface Broker extends BrokerSummary {
  brokerInstances: BrokerInstance[];
  autoMinorVersionUpgrade: boolean;
  engineVersion: string;
  publiclyAccessible: boolean;
  authenticationStrategy: string;
  storageType: string;
  securityGroups: string[];
  subnetIds: string[];
  users: BrokerUserSummary[];
}

export interface BrokerUserSummary {
  username: string;
  pendingChange?: string | null;
}

export interface BrokerUser extends BrokerUserSummary {
  brokerId: string;
  consoleAccess: boolean;
  groups: string[];
  replicationUser: boolean;
}

export interface CreateBrokerInput {
  brokerName: string;
  engineType: "RABBITMQ" | "ACTIVEMQ";
  engineVersion: string;
  hostInstanceType: string;
  deploymentMode?: string;
  publiclyAccessible?: boolean;
  initialUser?: { username: string; consoleAccess?: boolean };
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
    throw new Error(`MQ ${action} failed (HTTP ${res.status}): ${msg}`);
  }
  return (text ? JSON.parse(text) : {}) as T;
}

interface RawSummary {
  BrokerId: string;
  BrokerArn: string;
  BrokerName: string;
  BrokerState: string;
  DeploymentMode: string;
  EngineType: string;
  HostInstanceType: string;
  Created: number;
}

interface RawBroker extends RawSummary {
  BrokerInstances?: Array<{ Endpoints?: string[]; ConsoleURL?: string; IpAddress?: string }>;
  AutoMinorVersionUpgrade: boolean;
  EngineVersion: string;
  PubliclyAccessible: boolean;
  AuthenticationStrategy: string;
  StorageType: string;
  SecurityGroups?: string[];
  SubnetIds?: string[];
  Users?: Array<{ Username: string; PendingChange?: string | null }>;
}

interface RawUser {
  BrokerId: string;
  Username: string;
  ConsoleAccess: boolean;
  Groups?: string[];
  ReplicationUser: boolean;
  Pending?: unknown;
}

const fromSummary = (r: RawSummary): BrokerSummary => ({
  brokerId: r.BrokerId,
  brokerArn: r.BrokerArn,
  brokerName: r.BrokerName,
  brokerState: r.BrokerState,
  deploymentMode: r.DeploymentMode,
  engineType: r.EngineType,
  hostInstanceType: r.HostInstanceType,
  created: r.Created,
});

const fromBroker = (r: RawBroker): Broker => ({
  ...fromSummary(r),
  brokerInstances: (r.BrokerInstances ?? []).map((bi) => ({
    endpoints: bi.Endpoints ?? [],
    consoleURL: bi.ConsoleURL,
    ipAddress: bi.IpAddress,
  })),
  autoMinorVersionUpgrade: r.AutoMinorVersionUpgrade,
  engineVersion: r.EngineVersion,
  publiclyAccessible: r.PubliclyAccessible,
  authenticationStrategy: r.AuthenticationStrategy,
  storageType: r.StorageType,
  securityGroups: r.SecurityGroups ?? [],
  subnetIds: r.SubnetIds ?? [],
  users: (r.Users ?? []).map((u) => ({
    username: u.Username,
    pendingChange: u.PendingChange,
  })),
});

const fromUser = (r: RawUser): BrokerUser => ({
  brokerId: r.BrokerId,
  username: r.Username,
  consoleAccess: r.ConsoleAccess,
  groups: r.Groups ?? [],
  replicationUser: r.ReplicationUser,
  pendingChange: null,
});

export async function listBrokers(): Promise<BrokerSummary[]> {
  const data = await request<{ BrokerSummaries?: RawSummary[] }>(
    "ListBrokers",
    "GET",
    "/v1/brokers",
  );
  return (data.BrokerSummaries ?? []).map(fromSummary);
}

export async function describeBroker(id: string): Promise<Broker> {
  const r = await request<RawBroker>(
    "DescribeBroker",
    "GET",
    `/v1/brokers/${encodeURIComponent(id)}`,
  );
  return fromBroker(r);
}

export async function createBroker(
  input: CreateBrokerInput,
): Promise<{ brokerId: string; brokerArn: string }> {
  const body: Record<string, unknown> = {
    BrokerName: input.brokerName,
    EngineType: input.engineType,
    EngineVersion: input.engineVersion,
    HostInstanceType: input.hostInstanceType,
    DeploymentMode: input.deploymentMode ?? "SINGLE_INSTANCE",
    PubliclyAccessible: input.publiclyAccessible ?? false,
  };
  if (input.initialUser) {
    body.Users = [
      {
        Username: input.initialUser.username,
        ConsoleAccess: input.initialUser.consoleAccess ?? false,
      },
    ];
  }
  const r = await request<{ BrokerId: string; BrokerArn: string }>(
    "CreateBroker",
    "POST",
    "/v1/brokers",
    body,
  );
  return { brokerId: r.BrokerId, brokerArn: r.BrokerArn };
}

export async function deleteBroker(id: string): Promise<void> {
  await request<unknown>(
    "DeleteBroker",
    "DELETE",
    `/v1/brokers/${encodeURIComponent(id)}`,
  );
}

export async function rebootBroker(id: string): Promise<void> {
  await request<unknown>(
    "RebootBroker",
    "POST",
    `/v1/brokers/${encodeURIComponent(id)}/reboot`,
    {},
  );
}

export async function describeUser(
  brokerId: string,
  username: string,
): Promise<BrokerUser> {
  const r = await request<RawUser>(
    "DescribeUser",
    "GET",
    `/v1/brokers/${encodeURIComponent(brokerId)}/users/${encodeURIComponent(username)}`,
  );
  return fromUser(r);
}

export async function createUser(
  brokerId: string,
  username: string,
  consoleAccess = false,
  groups: string[] = [],
): Promise<void> {
  await request<unknown>(
    "CreateUser",
    "POST",
    `/v1/brokers/${encodeURIComponent(brokerId)}/users/${encodeURIComponent(username)}`,
    { ConsoleAccess: consoleAccess, Groups: groups },
  );
}

export async function deleteUser(
  brokerId: string,
  username: string,
): Promise<void> {
  await request<unknown>(
    "DeleteUser",
    "DELETE",
    `/v1/brokers/${encodeURIComponent(brokerId)}/users/${encodeURIComponent(username)}`,
  );
}
