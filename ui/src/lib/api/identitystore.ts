/**
 * Typed AWS Identity Store API client. AwsJson1.1 — X-Amz-Target prefix
 * is `AWSIdentityStore`.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "identitystore";
const TARGET_PREFIX = "AWSIdentityStore";

export interface IdUser {
  identityStoreId: string;
  userId: string;
  userName: string;
  displayName?: string;
  emails: string[];
}

export interface IdGroup {
  identityStoreId: string;
  groupId: string;
  displayName: string;
  description?: string;
}

export interface GroupMembership {
  identityStoreId: string;
  membershipId: string;
  groupId: string;
  memberUserId: string;
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
    throw new Error(
      `Identity Store ${action} failed (HTTP ${res.status}): ${msg}`,
    );
  }
  return (text ? JSON.parse(text) : {}) as T;
}

interface RawUser {
  IdentityStoreId: string;
  UserId: string;
  UserName: string;
  DisplayName?: string;
  Emails?: Array<{ Value?: string } | string>;
}

interface RawGroup {
  IdentityStoreId: string;
  GroupId: string;
  DisplayName: string;
  Description?: string;
}

interface RawMembership {
  IdentityStoreId: string;
  MembershipId: string;
  GroupId: string;
  MemberId: { UserId: string };
}

const fromUser = (r: RawUser): IdUser => ({
  identityStoreId: r.IdentityStoreId,
  userId: r.UserId,
  userName: r.UserName,
  displayName: r.DisplayName,
  emails: (r.Emails ?? []).map((e) => (typeof e === "string" ? e : e.Value ?? "")),
});

const fromGroup = (r: RawGroup): IdGroup => ({
  identityStoreId: r.IdentityStoreId,
  groupId: r.GroupId,
  displayName: r.DisplayName,
  description: r.Description,
});

const fromMembership = (r: RawMembership): GroupMembership => ({
  identityStoreId: r.IdentityStoreId,
  membershipId: r.MembershipId,
  groupId: r.GroupId,
  memberUserId: r.MemberId.UserId,
});

// ---------- Users ----------
export async function listUsers(identityStoreId: string): Promise<IdUser[]> {
  const data = await request<{ Users?: RawUser[] }>("ListUsers", {
    IdentityStoreId: identityStoreId,
  });
  return (data.Users ?? []).map(fromUser);
}

export async function createUser(input: {
  identityStoreId: string;
  userName: string;
  displayName?: string;
  email?: string;
}): Promise<{ userId: string }> {
  const body: Record<string, unknown> = {
    IdentityStoreId: input.identityStoreId,
    UserName: input.userName,
  };
  if (input.displayName) body.DisplayName = input.displayName;
  if (input.email) {
    body.Emails = [{ Value: input.email, Primary: true, Type: "work" }];
  }
  const r = await request<{ UserId: string }>("CreateUser", body);
  return { userId: r.UserId };
}

export async function deleteUser(
  identityStoreId: string,
  userId: string,
): Promise<void> {
  await request<unknown>("DeleteUser", {
    IdentityStoreId: identityStoreId,
    UserId: userId,
  });
}

// ---------- Groups ----------
export async function listGroups(identityStoreId: string): Promise<IdGroup[]> {
  const data = await request<{ Groups?: RawGroup[] }>("ListGroups", {
    IdentityStoreId: identityStoreId,
  });
  return (data.Groups ?? []).map(fromGroup);
}

export async function createGroup(input: {
  identityStoreId: string;
  displayName: string;
  description?: string;
}): Promise<{ groupId: string }> {
  const body: Record<string, unknown> = {
    IdentityStoreId: input.identityStoreId,
    DisplayName: input.displayName,
  };
  if (input.description) body.Description = input.description;
  const r = await request<{ GroupId: string }>("CreateGroup", body);
  return { groupId: r.GroupId };
}

export async function deleteGroup(
  identityStoreId: string,
  groupId: string,
): Promise<void> {
  await request<unknown>("DeleteGroup", {
    IdentityStoreId: identityStoreId,
    GroupId: groupId,
  });
}

// ---------- Memberships ----------
export async function listGroupMemberships(
  identityStoreId: string,
  groupId: string,
): Promise<GroupMembership[]> {
  const data = await request<{ GroupMemberships?: RawMembership[] }>(
    "ListGroupMemberships",
    { IdentityStoreId: identityStoreId, GroupId: groupId },
  );
  return (data.GroupMemberships ?? []).map(fromMembership);
}

export async function createGroupMembership(input: {
  identityStoreId: string;
  groupId: string;
  userId: string;
}): Promise<{ membershipId: string }> {
  const r = await request<{ MembershipId: string }>("CreateGroupMembership", {
    IdentityStoreId: input.identityStoreId,
    GroupId: input.groupId,
    MemberId: { UserId: input.userId },
  });
  return { membershipId: r.MembershipId };
}

export async function deleteGroupMembership(
  identityStoreId: string,
  membershipId: string,
): Promise<void> {
  await request<unknown>("DeleteGroupMembership", {
    IdentityStoreId: identityStoreId,
    MembershipId: membershipId,
  });
}
