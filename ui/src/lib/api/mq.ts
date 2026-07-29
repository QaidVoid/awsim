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

// Amazon MQ names every field in camelCase on the wire, which already
// matches the exported interfaces, so no key mapping is needed.
interface RawBrokerInstance {
  endpoints?: string[];
  consoleURL?: string;
  ipAddress?: string;
}

interface RawBroker extends BrokerSummary {
  brokerInstances?: RawBrokerInstance[];
  autoMinorVersionUpgrade: boolean;
  engineVersion: string;
  publiclyAccessible: boolean;
  authenticationStrategy: string;
  storageType: string;
  securityGroups?: string[];
  subnetIds?: string[];
  users?: Array<{ username: string; pendingChange?: string | null }>;
}

interface RawUser {
  brokerId: string;
  username: string;
  consoleAccess: boolean;
  groups?: string[];
  replicationUser: boolean;
  pending?: unknown;
}

const fromBroker = (r: RawBroker): Broker => ({
  brokerId: r.brokerId,
  brokerArn: r.brokerArn,
  brokerName: r.brokerName,
  brokerState: r.brokerState,
  deploymentMode: r.deploymentMode,
  engineType: r.engineType,
  hostInstanceType: r.hostInstanceType,
  created: r.created,
  brokerInstances: (r.brokerInstances ?? []).map((bi) => ({
    endpoints: bi.endpoints ?? [],
    consoleURL: bi.consoleURL,
    ipAddress: bi.ipAddress,
  })),
  autoMinorVersionUpgrade: r.autoMinorVersionUpgrade,
  engineVersion: r.engineVersion,
  publiclyAccessible: r.publiclyAccessible,
  authenticationStrategy: r.authenticationStrategy,
  storageType: r.storageType,
  securityGroups: r.securityGroups ?? [],
  subnetIds: r.subnetIds ?? [],
  users: (r.users ?? []).map((u) => ({
    username: u.username,
    pendingChange: u.pendingChange,
  })),
});

const fromUser = (r: RawUser): BrokerUser => ({
  brokerId: r.brokerId,
  username: r.username,
  consoleAccess: r.consoleAccess,
  groups: r.groups ?? [],
  replicationUser: r.replicationUser,
  pendingChange: null,
});

export async function listBrokers(): Promise<BrokerSummary[]> {
  const data = await request<{ brokerSummaries?: BrokerSummary[] }>(
    "ListBrokers",
    "GET",
    "/v1/brokers",
  );
  return data.brokerSummaries ?? [];
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
    brokerName: input.brokerName,
    engineType: input.engineType,
    engineVersion: input.engineVersion,
    hostInstanceType: input.hostInstanceType,
    deploymentMode: input.deploymentMode ?? "SINGLE_INSTANCE",
    publiclyAccessible: input.publiclyAccessible ?? false,
  };
  if (input.initialUser) {
    body.users = [
      {
        username: input.initialUser.username,
        consoleAccess: input.initialUser.consoleAccess ?? false,
      },
    ];
  }
  const r = await request<{ brokerId: string; brokerArn: string }>(
    "CreateBroker",
    "POST",
    "/v1/brokers",
    body,
  );
  return { brokerId: r.brokerId, brokerArn: r.brokerArn };
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
    { consoleAccess, groups },
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
