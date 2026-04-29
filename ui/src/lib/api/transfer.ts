/**
 * Typed AWS Transfer Family API client. AwsJson1.1 — X-Amz-Target prefix
 * is `TransferService`.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "transfer";
const TARGET_PREFIX = "TransferService";

export interface ServerSummary {
  serverId: string;
  arn: string;
  state: string;
  identityProviderType: string;
  endpointType: string;
  userCount: number;
}

export interface Server extends ServerSummary {
  brokerName?: string;
  protocols: string[];
  domain: string;
  loggingRole?: string;
  created: number;
  tags: Record<string, string>;
}

export interface UserSummary {
  userName: string;
  homeDirectory?: string;
  homeDirectoryType: string;
  role: string;
  sshPublicKeyCount: number;
}

async function request<T>(
  action: string,
  body: Record<string, unknown> = {},
): Promise<T> {
  const res = await loggedFetch(SERVICE, action, "POST", ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.1",
      "X-Amz-Target": `${TARGET_PREFIX}.${action}`,
      Authorization: authHeader(SERVICE),
      "X-Amz-Date": amzDate(),
    },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) {
    let msg = text;
    try {
      const data = JSON.parse(text) as { message?: string; Message?: string };
      msg = data.message ?? data.Message ?? text;
    } catch {
      // not JSON
    }
    throw new Error(`Transfer ${action} failed (HTTP ${res.status}): ${msg}`);
  }
  return (text ? JSON.parse(text) : {}) as T;
}

interface RawServerSummary {
  Arn: string;
  ServerId: string;
  State: string;
  IdentityProviderType: string;
  EndpointType: string;
  UserCount: number;
}

interface RawServer extends RawServerSummary {
  BrokerName?: string;
  Protocols?: string[];
  Domain: string;
  LoggingRole?: string;
  Created: number;
  Tags?: Record<string, string>;
}

interface RawUser {
  Arn: string;
  UserName: string;
  HomeDirectory?: string;
  HomeDirectoryType: string;
  Role: string;
  SshPublicKeyCount: number;
}

const fromServerSummary = (r: RawServerSummary): ServerSummary => ({
  serverId: r.ServerId,
  arn: r.Arn,
  state: r.State,
  identityProviderType: r.IdentityProviderType,
  endpointType: r.EndpointType,
  userCount: r.UserCount,
});

const fromServer = (r: RawServer): Server => ({
  ...fromServerSummary(r),
  protocols: r.Protocols ?? [],
  domain: r.Domain,
  loggingRole: r.LoggingRole,
  created: r.Created,
  tags: r.Tags ?? {},
});

const fromUser = (r: RawUser): UserSummary => ({
  userName: r.UserName,
  homeDirectory: r.HomeDirectory,
  homeDirectoryType: r.HomeDirectoryType,
  role: r.Role,
  sshPublicKeyCount: r.SshPublicKeyCount,
});

export async function listServers(): Promise<ServerSummary[]> {
  const data = await request<{ Servers?: RawServerSummary[] }>("ListServers");
  return (data.Servers ?? []).map(fromServerSummary);
}

export async function describeServer(serverId: string): Promise<Server> {
  const data = await request<{ Server: RawServer }>("DescribeServer", {
    ServerId: serverId,
  });
  return fromServer(data.Server);
}

export async function createServer(
  protocols: string[] = ["SFTP"],
): Promise<{ serverId: string }> {
  const r = await request<{ ServerId: string }>("CreateServer", {
    Protocols: protocols,
  });
  return { serverId: r.ServerId };
}

export async function deleteServer(serverId: string): Promise<void> {
  await request<unknown>("DeleteServer", { ServerId: serverId });
}

export async function startServer(serverId: string): Promise<void> {
  await request<unknown>("StartServer", { ServerId: serverId });
}

export async function stopServer(serverId: string): Promise<void> {
  await request<unknown>("StopServer", { ServerId: serverId });
}

export async function listUsers(serverId: string): Promise<UserSummary[]> {
  const data = await request<{ Users?: RawUser[] }>("ListUsers", {
    ServerId: serverId,
  });
  return (data.Users ?? []).map(fromUser);
}

export async function createUser(input: {
  serverId: string;
  userName: string;
  role: string;
  homeDirectory?: string;
}): Promise<void> {
  const body: Record<string, unknown> = {
    ServerId: input.serverId,
    UserName: input.userName,
    Role: input.role,
  };
  if (input.homeDirectory) body.HomeDirectory = input.homeDirectory;
  await request<unknown>("CreateUser", body);
}

export async function deleteUser(
  serverId: string,
  userName: string,
): Promise<void> {
  await request<unknown>("DeleteUser", {
    ServerId: serverId,
    UserName: userName,
  });
}

export async function importSshPublicKey(
  serverId: string,
  userName: string,
  body: string,
): Promise<{ sshPublicKeyId: string }> {
  const r = await request<{ SshPublicKeyId: string }>("ImportSshPublicKey", {
    ServerId: serverId,
    UserName: userName,
    SshPublicKeyBody: body,
  });
  return { sshPublicKeyId: r.SshPublicKeyId };
}
