/**
 * Typed IAM API client.
 *
 * Talks to the AWSim emulator at http://localhost:4566 over the legacy
 * IAM "query" protocol. Returns parsed, typed records — no XML escapes
 * out into UI components.
 */

const ENDPOINT = "http://localhost:4566";
const FAKE_DATE = new Date().toISOString().slice(0, 10).replace(/-/g, "");

function authHeader(): string {
  return `AWS4-HMAC-SHA256 Credential=awsim-admin/${FAKE_DATE}/us-east-1/iam/aws4_request, SignedHeaders=host;x-amz-date, Signature=fakesignature`;
}

function amzDate(): string {
  return new Date().toISOString().replace(/[:-]/g, "").slice(0, 15) + "Z";
}

async function iamRequest(
  action: string,
  params: Record<string, string> = {},
): Promise<string> {
  const body = new URLSearchParams({
    Action: action,
    Version: "2010-05-08",
    ...params,
  });
  const res = await fetch(ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-www-form-urlencoded",
      Authorization: authHeader(),
      "X-Amz-Date": amzDate(),
    },
    body: body.toString(),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`IAM ${action} failed: ${res.status} ${text}`);
  return text;
}

function xmlValue(xml: string, tag: string): string {
  const match = xml.match(new RegExp(`<${tag}>([^<]*)</${tag}>`));
  return match ? match[1] : "";
}

function xmlArray(
  xml: string,
  itemTag: string,
  fields: string[],
): Record<string, string>[] {
  const items: Record<string, string>[] = [];
  const regex = new RegExp(`<${itemTag}>([\\s\\S]*?)</${itemTag}>`, "g");
  let match: RegExpExecArray | null;
  while ((match = regex.exec(xml)) !== null) {
    const item: Record<string, string> = {};
    for (const field of fields) {
      item[field] = xmlValue(match[1], field);
    }
    items.push(item);
  }
  return items;
}

// ---- Types ----

export interface IamUser {
  userName: string;
  userId: string;
  arn: string;
  createDate: string;
}

export interface IamRole {
  roleName: string;
  roleId: string;
  arn: string;
  createDate?: string;
  description?: string;
  assumeRolePolicyDocument?: string;
}

export interface IamGroup {
  groupName: string;
  groupId: string;
  arn: string;
}

export interface IamPolicy {
  policyName: string;
  arn: string;
  attachmentCount: string;
  defaultVersionId?: string;
  description?: string;
  createDate?: string;
}

export interface IamPolicyVersion {
  versionId: string;
  isDefaultVersion: boolean;
  createDate: string;
}

export interface IamAttachedPolicy {
  policyName: string;
  policyArn: string;
}

// ---- Users ----

export async function listUsers(): Promise<IamUser[]> {
  const xml = await iamRequest("ListUsers");
  const raw = xmlArray(xml, "member", [
    "UserName",
    "UserId",
    "Arn",
    "CreateDate",
  ]);
  return raw.map((u) => ({
    userName: u["UserName"] ?? "",
    userId: u["UserId"] ?? "",
    arn: u["Arn"] ?? "",
    createDate: u["CreateDate"] ?? "",
  }));
}

export async function getUser(userName: string): Promise<IamUser> {
  const xml = await iamRequest("GetUser", { UserName: userName });
  return {
    userName: xmlValue(xml, "UserName"),
    userId: xmlValue(xml, "UserId"),
    arn: xmlValue(xml, "Arn"),
    createDate: xmlValue(xml, "CreateDate"),
  };
}

export async function createUser(
  userName: string,
  path?: string,
): Promise<IamUser> {
  const params: Record<string, string> = { UserName: userName };
  if (path) params["Path"] = path;
  const xml = await iamRequest("CreateUser", params);
  return {
    userName: xmlValue(xml, "UserName"),
    userId: xmlValue(xml, "UserId"),
    arn: xmlValue(xml, "Arn"),
    createDate: xmlValue(xml, "CreateDate"),
  };
}

export async function deleteUser(userName: string): Promise<void> {
  await iamRequest("DeleteUser", { UserName: userName });
}

export async function listAttachedUserPolicies(
  userName: string,
): Promise<IamAttachedPolicy[]> {
  const xml = await iamRequest("ListAttachedUserPolicies", {
    UserName: userName,
  });
  const raw = xmlArray(xml, "member", ["PolicyName", "PolicyArn"]);
  return raw.map((p) => ({
    policyName: p["PolicyName"] ?? "",
    policyArn: p["PolicyArn"] ?? "",
  }));
}

export async function attachUserPolicy(
  userName: string,
  policyArn: string,
): Promise<void> {
  await iamRequest("AttachUserPolicy", {
    UserName: userName,
    PolicyArn: policyArn,
  });
}

export async function detachUserPolicy(
  userName: string,
  policyArn: string,
): Promise<void> {
  await iamRequest("DetachUserPolicy", {
    UserName: userName,
    PolicyArn: policyArn,
  });
}

export async function listUserPolicies(userName: string): Promise<string[]> {
  const xml = await iamRequest("ListUserPolicies", { UserName: userName });
  return parseInlinePolicyNames(xml);
}

// `<PolicyNames><member>name</member>…</PolicyNames>` lists from
// IAM put extra whitespace inside the `<member>` tags, so we trim
// before returning. Shared by user/role list functions.
function parseInlinePolicyNames(xml: string): string[] {
  return [...xml.matchAll(/<member>([\s\S]*?)<\/member>/g)]
    .map((m) => m[1].trim())
    .filter((s) => s.length > 0);
}

export async function getUserPolicy(
  userName: string,
  policyName: string,
): Promise<string> {
  const xml = await iamRequest("GetUserPolicy", {
    UserName: userName,
    PolicyName: policyName,
  });
  const doc = xmlValue(xml, "PolicyDocument");
  return doc ? decodeURIComponent(doc) : "";
}

export async function putUserPolicy(
  userName: string,
  policyName: string,
  document: string,
): Promise<void> {
  await iamRequest("PutUserPolicy", {
    UserName: userName,
    PolicyName: policyName,
    PolicyDocument: document,
  });
}

export async function deleteUserPolicy(
  userName: string,
  policyName: string,
): Promise<void> {
  await iamRequest("DeleteUserPolicy", {
    UserName: userName,
    PolicyName: policyName,
  });
}

// ---- Access keys ----

export interface IamAccessKey {
  accessKeyId: string;
  status: string; // "Active" | "Inactive"
  createDate: string;
}

export interface IamAccessKeyWithSecret extends IamAccessKey {
  secretAccessKey: string;
}

export async function listAccessKeys(
  userName: string,
): Promise<IamAccessKey[]> {
  const xml = await iamRequest("ListAccessKeys", { UserName: userName });
  const raw = xmlArray(xml, "member", ["AccessKeyId", "Status", "CreateDate"]);
  return raw.map((k) => ({
    accessKeyId: k["AccessKeyId"] ?? "",
    status: k["Status"] ?? "",
    createDate: k["CreateDate"] ?? "",
  }));
}

export async function createAccessKey(
  userName: string,
): Promise<IamAccessKeyWithSecret> {
  const xml = await iamRequest("CreateAccessKey", { UserName: userName });
  return {
    accessKeyId: xmlValue(xml, "AccessKeyId"),
    secretAccessKey: xmlValue(xml, "SecretAccessKey"),
    status: xmlValue(xml, "Status") || "Active",
    createDate: xmlValue(xml, "CreateDate"),
  };
}

export async function updateAccessKey(
  userName: string,
  accessKeyId: string,
  status: "Active" | "Inactive",
): Promise<void> {
  await iamRequest("UpdateAccessKey", {
    UserName: userName,
    AccessKeyId: accessKeyId,
    Status: status,
  });
}

export async function deleteAccessKey(
  userName: string,
  accessKeyId: string,
): Promise<void> {
  await iamRequest("DeleteAccessKey", {
    UserName: userName,
    AccessKeyId: accessKeyId,
  });
}

// ---- Roles ----

export async function listRoles(): Promise<IamRole[]> {
  const xml = await iamRequest("ListRoles");
  const raw = xmlArray(xml, "member", [
    "RoleName",
    "RoleId",
    "Arn",
    "CreateDate",
    "Description",
  ]);
  return raw.map((r) => ({
    roleName: r["RoleName"] ?? "",
    roleId: r["RoleId"] ?? "",
    arn: r["Arn"] ?? "",
    createDate: r["CreateDate"] ?? "",
    description: r["Description"] ?? "",
  }));
}

export async function getRole(roleName: string): Promise<IamRole> {
  const xml = await iamRequest("GetRole", { RoleName: roleName });
  const doc = xmlValue(xml, "AssumeRolePolicyDocument");
  return {
    roleName: xmlValue(xml, "RoleName"),
    roleId: xmlValue(xml, "RoleId"),
    arn: xmlValue(xml, "Arn"),
    description: xmlValue(xml, "Description") || undefined,
    createDate: xmlValue(xml, "CreateDate") || undefined,
    assumeRolePolicyDocument: doc ? decodeURIComponent(doc) : "",
  };
}

export async function createRole(
  roleName: string,
  assumeRolePolicyDocument: string,
  description?: string,
): Promise<IamRole> {
  const params: Record<string, string> = {
    RoleName: roleName,
    AssumeRolePolicyDocument: assumeRolePolicyDocument,
  };
  if (description) params["Description"] = description;
  const xml = await iamRequest("CreateRole", params);
  return {
    roleName: xmlValue(xml, "RoleName"),
    roleId: xmlValue(xml, "RoleId"),
    arn: xmlValue(xml, "Arn"),
    createDate: xmlValue(xml, "CreateDate"),
  };
}

export async function deleteRole(roleName: string): Promise<void> {
  await iamRequest("DeleteRole", { RoleName: roleName });
}

export async function listAttachedRolePolicies(
  roleName: string,
): Promise<IamAttachedPolicy[]> {
  const xml = await iamRequest("ListAttachedRolePolicies", {
    RoleName: roleName,
  });
  const raw = xmlArray(xml, "member", ["PolicyName", "PolicyArn"]);
  return raw.map((p) => ({
    policyName: p["PolicyName"] ?? "",
    policyArn: p["PolicyArn"] ?? "",
  }));
}

export async function attachRolePolicy(
  roleName: string,
  policyArn: string,
): Promise<void> {
  await iamRequest("AttachRolePolicy", {
    RoleName: roleName,
    PolicyArn: policyArn,
  });
}

export async function detachRolePolicy(
  roleName: string,
  policyArn: string,
): Promise<void> {
  await iamRequest("DetachRolePolicy", {
    RoleName: roleName,
    PolicyArn: policyArn,
  });
}

export async function listRolePolicies(roleName: string): Promise<string[]> {
  const xml = await iamRequest("ListRolePolicies", { RoleName: roleName });
  return parseInlinePolicyNames(xml);
}

export async function getRolePolicy(
  roleName: string,
  policyName: string,
): Promise<string> {
  const xml = await iamRequest("GetRolePolicy", {
    RoleName: roleName,
    PolicyName: policyName,
  });
  const doc = xmlValue(xml, "PolicyDocument");
  return doc ? decodeURIComponent(doc) : "";
}

export async function putRolePolicy(
  roleName: string,
  policyName: string,
  document: string,
): Promise<void> {
  await iamRequest("PutRolePolicy", {
    RoleName: roleName,
    PolicyName: policyName,
    PolicyDocument: document,
  });
}

export async function deleteRolePolicy(
  roleName: string,
  policyName: string,
): Promise<void> {
  await iamRequest("DeleteRolePolicy", {
    RoleName: roleName,
    PolicyName: policyName,
  });
}

export async function updateAssumeRolePolicy(
  roleName: string,
  document: string,
): Promise<void> {
  await iamRequest("UpdateAssumeRolePolicy", {
    RoleName: roleName,
    PolicyDocument: document,
  });
}

// ---- Groups ----

export async function listGroups(): Promise<IamGroup[]> {
  const xml = await iamRequest("ListGroups");
  const raw = xmlArray(xml, "member", ["GroupName", "GroupId", "Arn"]);
  return raw.map((g) => ({
    groupName: g["GroupName"] ?? "",
    groupId: g["GroupId"] ?? "",
    arn: g["Arn"] ?? "",
  }));
}

export async function getGroup(
  name: string,
): Promise<{ group: IamGroup; users: IamUser[] }> {
  const xml = await iamRequest("GetGroup", { GroupName: name });
  const group: IamGroup = {
    groupName: xmlValue(xml, "GroupName"),
    groupId: xmlValue(xml, "GroupId"),
    arn: xmlValue(xml, "Arn"),
  };
  const raw = xmlArray(xml, "member", [
    "UserName",
    "UserId",
    "Arn",
    "CreateDate",
  ]);
  const users: IamUser[] = raw
    .filter((u) => u["UserName"])
    .map((u) => ({
      userName: u["UserName"] ?? "",
      userId: u["UserId"] ?? "",
      arn: u["Arn"] ?? "",
      createDate: u["CreateDate"] ?? "",
    }));
  return { group, users };
}

export async function createGroup(groupName: string): Promise<IamGroup> {
  const xml = await iamRequest("CreateGroup", { GroupName: groupName });
  return {
    groupName: xmlValue(xml, "GroupName"),
    groupId: xmlValue(xml, "GroupId"),
    arn: xmlValue(xml, "Arn"),
  };
}

export async function deleteGroup(groupName: string): Promise<void> {
  await iamRequest("DeleteGroup", { GroupName: groupName });
}

export async function listAttachedGroupPolicies(
  groupName: string,
): Promise<IamAttachedPolicy[]> {
  const xml = await iamRequest("ListAttachedGroupPolicies", {
    GroupName: groupName,
  });
  const raw = xmlArray(xml, "member", ["PolicyName", "PolicyArn"]);
  return raw.map((p) => ({
    policyName: p["PolicyName"] ?? "",
    policyArn: p["PolicyArn"] ?? "",
  }));
}

export async function attachGroupPolicy(
  groupName: string,
  policyArn: string,
): Promise<void> {
  await iamRequest("AttachGroupPolicy", {
    GroupName: groupName,
    PolicyArn: policyArn,
  });
}

export async function detachGroupPolicy(
  groupName: string,
  policyArn: string,
): Promise<void> {
  await iamRequest("DetachGroupPolicy", {
    GroupName: groupName,
    PolicyArn: policyArn,
  });
}

export async function addUserToGroup(
  groupName: string,
  userName: string,
): Promise<void> {
  await iamRequest("AddUserToGroup", {
    GroupName: groupName,
    UserName: userName,
  });
}

export async function removeUserFromGroup(
  groupName: string,
  userName: string,
): Promise<void> {
  await iamRequest("RemoveUserFromGroup", {
    GroupName: groupName,
    UserName: userName,
  });
}

export async function listGroupsForUser(userName: string): Promise<IamGroup[]> {
  const xml = await iamRequest("ListGroupsForUser", { UserName: userName });
  const raw = xmlArray(xml, "member", ["GroupName", "GroupId", "Arn"]);
  return raw.map((g) => ({
    groupName: g["GroupName"] ?? "",
    groupId: g["GroupId"] ?? "",
    arn: g["Arn"] ?? "",
  }));
}

export async function listGroupPolicies(
  groupName: string,
): Promise<string[]> {
  const xml = await iamRequest("ListGroupPolicies", { GroupName: groupName });
  return parseInlinePolicyNames(xml);
}

export async function getGroupPolicy(
  groupName: string,
  policyName: string,
): Promise<string> {
  const xml = await iamRequest("GetGroupPolicy", {
    GroupName: groupName,
    PolicyName: policyName,
  });
  const doc = xmlValue(xml, "PolicyDocument");
  return doc ? decodeURIComponent(doc) : "";
}

export async function putGroupPolicy(
  groupName: string,
  policyName: string,
  document: string,
): Promise<void> {
  await iamRequest("PutGroupPolicy", {
    GroupName: groupName,
    PolicyName: policyName,
    PolicyDocument: document,
  });
}

export async function deleteGroupPolicy(
  groupName: string,
  policyName: string,
): Promise<void> {
  await iamRequest("DeleteGroupPolicy", {
    GroupName: groupName,
    PolicyName: policyName,
  });
}

// ---- Instance Profiles ----

export interface IamInstanceProfile {
  instanceProfileName: string;
  instanceProfileId: string;
  arn: string;
  createDate: string;
  roles: IamRole[];
}

export async function listInstanceProfiles(): Promise<IamInstanceProfile[]> {
  const xml = await iamRequest("ListInstanceProfiles");
  const doc = new DOMParser().parseFromString(xml, "application/xml");
  if (doc.querySelector("parsererror")) return [];

  const text = (parent: Element | null, tag: string): string =>
    parent?.getElementsByTagName(tag)[0]?.textContent?.trim() ?? "";

  const profiles: IamInstanceProfile[] = [];
  const list = doc.getElementsByTagName("InstanceProfiles")[0];
  if (!list) return [];

  for (const member of Array.from(list.children)) {
    if (member.tagName !== "member") continue;
    const roles: IamRole[] = [];
    const rolesEl = member.getElementsByTagName("Roles")[0];
    if (rolesEl) {
      for (const rm of Array.from(rolesEl.children)) {
        if (rm.tagName !== "member") continue;
        const doc2 = xmlValue(rm.innerHTML, "AssumeRolePolicyDocument");
        roles.push({
          roleName: text(rm, "RoleName"),
          roleId: text(rm, "RoleId"),
          arn: text(rm, "Arn"),
          createDate: text(rm, "CreateDate"),
          assumeRolePolicyDocument: doc2 ? decodeURIComponent(doc2) : "",
        });
      }
    }
    profiles.push({
      instanceProfileName: text(member, "InstanceProfileName"),
      instanceProfileId: text(member, "InstanceProfileId"),
      arn: text(member, "Arn"),
      createDate: text(member, "CreateDate"),
      roles,
    });
  }
  return profiles;
}

export async function createInstanceProfile(
  name: string,
  path?: string,
): Promise<IamInstanceProfile> {
  const params: Record<string, string> = { InstanceProfileName: name };
  if (path) params["Path"] = path;
  const xml = await iamRequest("CreateInstanceProfile", params);
  return {
    instanceProfileName: xmlValue(xml, "InstanceProfileName"),
    instanceProfileId: xmlValue(xml, "InstanceProfileId"),
    arn: xmlValue(xml, "Arn"),
    createDate: xmlValue(xml, "CreateDate"),
    roles: [],
  };
}

export async function deleteInstanceProfile(name: string): Promise<void> {
  await iamRequest("DeleteInstanceProfile", { InstanceProfileName: name });
}

export async function addRoleToInstanceProfile(
  instanceProfileName: string,
  roleName: string,
): Promise<void> {
  await iamRequest("AddRoleToInstanceProfile", {
    InstanceProfileName: instanceProfileName,
    RoleName: roleName,
  });
}

export async function removeRoleFromInstanceProfile(
  instanceProfileName: string,
  roleName: string,
): Promise<void> {
  await iamRequest("RemoveRoleFromInstanceProfile", {
    InstanceProfileName: instanceProfileName,
    RoleName: roleName,
  });
}

// ---- Policies ----

export async function listPolicies(
  scope: "Local" | "All" = "Local",
): Promise<IamPolicy[]> {
  const xml = await iamRequest("ListPolicies", { Scope: scope });
  const raw = xmlArray(xml, "member", [
    "PolicyName",
    "Arn",
    "AttachmentCount",
    "DefaultVersionId",
    "Description",
    "CreateDate",
  ]);
  return raw.map((p) => ({
    policyName: p["PolicyName"] ?? "",
    arn: p["Arn"] ?? "",
    attachmentCount: p["AttachmentCount"] ?? "0",
    defaultVersionId: p["DefaultVersionId"] || undefined,
    description: p["Description"] || undefined,
    createDate: p["CreateDate"] || undefined,
  }));
}

export async function getPolicy(arn: string): Promise<IamPolicy> {
  const xml = await iamRequest("GetPolicy", { PolicyArn: arn });
  return {
    policyName: xmlValue(xml, "PolicyName"),
    arn: xmlValue(xml, "Arn"),
    attachmentCount: xmlValue(xml, "AttachmentCount"),
    defaultVersionId: xmlValue(xml, "DefaultVersionId"),
    description: xmlValue(xml, "Description") || undefined,
    createDate: xmlValue(xml, "CreateDate") || undefined,
  };
}

export async function createPolicy(
  policyName: string,
  document: string,
  description?: string,
): Promise<IamPolicy> {
  const params: Record<string, string> = {
    PolicyName: policyName,
    PolicyDocument: document,
  };
  if (description) params["Description"] = description;
  const xml = await iamRequest("CreatePolicy", params);
  return {
    policyName: xmlValue(xml, "PolicyName"),
    arn: xmlValue(xml, "Arn"),
    attachmentCount: xmlValue(xml, "AttachmentCount") || "0",
    defaultVersionId: xmlValue(xml, "DefaultVersionId") || undefined,
    description: xmlValue(xml, "Description") || undefined,
    createDate: xmlValue(xml, "CreateDate") || undefined,
  };
}

export async function deletePolicy(arn: string): Promise<void> {
  await iamRequest("DeletePolicy", { PolicyArn: arn });
}

export async function listPolicyVersions(
  arn: string,
): Promise<IamPolicyVersion[]> {
  const xml = await iamRequest("ListPolicyVersions", { PolicyArn: arn });
  const raw = xmlArray(xml, "member", [
    "VersionId",
    "IsDefaultVersion",
    "CreateDate",
  ]);
  return raw.map((v) => ({
    versionId: v["VersionId"] ?? "",
    isDefaultVersion: v["IsDefaultVersion"] === "true",
    createDate: v["CreateDate"] ?? "",
  }));
}

export async function getPolicyVersion(
  arn: string,
  versionId: string,
): Promise<{ document: string; isDefaultVersion: boolean }> {
  const xml = await iamRequest("GetPolicyVersion", {
    PolicyArn: arn,
    VersionId: versionId,
  });
  const doc = xmlValue(xml, "Document");
  return {
    document: doc ? decodeURIComponent(doc) : "",
    isDefaultVersion: xmlValue(xml, "IsDefaultVersion") === "true",
  };
}

export async function createPolicyVersion(
  arn: string,
  document: string,
  setAsDefault = true,
): Promise<void> {
  await iamRequest("CreatePolicyVersion", {
    PolicyArn: arn,
    PolicyDocument: document,
    SetAsDefault: String(setAsDefault),
  });
}

export async function setDefaultPolicyVersion(
  arn: string,
  versionId: string,
): Promise<void> {
  await iamRequest("SetDefaultPolicyVersion", {
    PolicyArn: arn,
    VersionId: versionId,
  });
}

// ---- Simulator ----

export type EvalDecision = "allowed" | "explicitDeny" | "implicitDeny";

export interface MatchedStatement {
  sourcePolicyId: string;
  sourcePolicyType: string;
  startPosition?: { line: number; column: number };
}

export interface MissingContextValue {
  key: string;
}

export interface EvaluationResult {
  evalActionName: string;
  evalResourceName?: string;
  evalDecision: EvalDecision;
  matchedStatements: MatchedStatement[];
  missingContextValues: string[];
}

export interface SimulationResult {
  results: EvaluationResult[];
}

export interface ContextEntry {
  key: string;
  values: string[];
  type: string;
}

function applyActions(params: Record<string, string>, actions: string[]): void {
  actions.forEach((a, i) => {
    params[`ActionNames.member.${i + 1}`] = a;
  });
}

function applyResources(
  params: Record<string, string>,
  resources: string[],
): void {
  resources.forEach((r, i) => {
    params[`ResourceArns.member.${i + 1}`] = r;
  });
}

function applyContext(
  params: Record<string, string>,
  ctx: ContextEntry[],
): void {
  ctx.forEach((entry, i) => {
    const idx = i + 1;
    params[`ContextEntries.member.${idx}.ContextKeyName`] = entry.key;
    params[`ContextEntries.member.${idx}.ContextKeyType`] = entry.type;
    entry.values.forEach((v, j) => {
      params[`ContextEntries.member.${idx}.ContextKeyValues.member.${j + 1}`] =
        v;
    });
  });
}

function parseSimulationResult(xml: string): SimulationResult {
  // The simulator response has nested `<member>` tags
  // (EvaluationResults > member > MatchedStatements > member). A
  // naive lazy regex like `/<member>(.*?)</member>/` matches the
  // first inner closing tag and corrupts the outer capture, so use
  // a real XML parser here.
  const doc = new DOMParser().parseFromString(xml, "application/xml");
  if (doc.querySelector("parsererror")) {
    return { results: [] };
  }
  const text = (parent: Element | null, tag: string): string =>
    parent?.getElementsByTagName(tag)[0]?.textContent?.trim() ?? "";

  const results: EvaluationResult[] = [];
  const evalsList = doc.getElementsByTagName("EvaluationResults")[0];
  if (!evalsList) return { results };

  // Top-level <member>s are direct children of EvaluationResults —
  // children, not getElementsByTagName which would also pick up
  // nested members from MatchedStatements / MissingContextValues.
  for (const member of Array.from(evalsList.children)) {
    if (member.tagName !== "member") continue;
    const decisionRaw = text(member, "EvalDecision");
    let decision: EvalDecision = "implicitDeny";
    if (decisionRaw === "allowed") decision = "allowed";
    else if (decisionRaw === "explicitDeny") decision = "explicitDeny";

    const matched: MatchedStatement[] = [];
    const stmtsList = member.getElementsByTagName("MatchedStatements")[0];
    if (stmtsList) {
      for (const sm of Array.from(stmtsList.children)) {
        if (sm.tagName !== "member") continue;
        matched.push({
          sourcePolicyId: text(sm, "SourcePolicyId"),
          sourcePolicyType: text(sm, "SourcePolicyType"),
        });
      }
    }

    const missing: string[] = [];
    const missList = member.getElementsByTagName("MissingContextValues")[0];
    if (missList) {
      for (const im of Array.from(missList.children)) {
        if (im.tagName !== "member") continue;
        const key = (im.textContent ?? "").trim();
        if (key) missing.push(key);
      }
    }

    results.push({
      evalActionName: text(member, "EvalActionName"),
      evalResourceName: text(member, "EvalResourceName") || undefined,
      evalDecision: decision,
      matchedStatements: matched,
      missingContextValues: missing,
    });
  }
  return { results };
}

export async function simulatePrincipalPolicy(input: {
  policySourceArn: string;
  actions: string[];
  resources?: string[];
  contextEntries?: ContextEntry[];
}): Promise<SimulationResult> {
  const params: Record<string, string> = {
    PolicySourceArn: input.policySourceArn,
  };
  applyActions(params, input.actions);
  if (input.resources?.length) applyResources(params, input.resources);
  if (input.contextEntries?.length) applyContext(params, input.contextEntries);
  const xml = await iamRequest("SimulatePrincipalPolicy", params);
  return parseSimulationResult(xml);
}

export async function simulateCustomPolicy(input: {
  policyInputList: string[];
  actions: string[];
  resources?: string[];
  contextEntries?: ContextEntry[];
}): Promise<SimulationResult> {
  const params: Record<string, string> = {};
  input.policyInputList.forEach((p, i) => {
    params[`PolicyInputList.member.${i + 1}`] = p;
  });
  applyActions(params, input.actions);
  if (input.resources?.length) applyResources(params, input.resources);
  if (input.contextEntries?.length) applyContext(params, input.contextEntries);
  const xml = await iamRequest("SimulateCustomPolicy", params);
  return parseSimulationResult(xml);
}

// ---- Common AWS action suggestions for simulator autocomplete ----

// Curated list of common IAM actions for the simulator autocomplete.
// Ordered so the first ~30 entries cover a broad spectrum of services
// (so the empty-input dropdown doesn't read as "S3 only"). Service
// blocks are roughly grouped: storage, compute, messaging, identity,
// observability, data, infra.
export const ACTION_SUGGESTIONS: string[] = [
  "*",
  // Storage
  "s3:GetObject",
  "s3:PutObject",
  "s3:ListBucket",
  "s3:DeleteObject",
  "s3:*",
  // Database
  "dynamodb:GetItem",
  "dynamodb:PutItem",
  "dynamodb:Query",
  "dynamodb:Scan",
  "dynamodb:DeleteItem",
  "dynamodb:UpdateItem",
  "dynamodb:BatchGetItem",
  "dynamodb:BatchWriteItem",
  "dynamodb:CreateTable",
  "dynamodb:DescribeTable",
  "dynamodb:*",
  // Compute
  "lambda:InvokeFunction",
  "lambda:GetFunction",
  "lambda:CreateFunction",
  "lambda:UpdateFunctionCode",
  "lambda:DeleteFunction",
  "lambda:ListFunctions",
  "lambda:*",
  "ec2:DescribeInstances",
  "ec2:RunInstances",
  "ec2:TerminateInstances",
  "ec2:StartInstances",
  "ec2:StopInstances",
  "ec2:CreateSecurityGroup",
  "ecs:RunTask",
  "ecs:StopTask",
  "ecs:DescribeTasks",
  // Messaging / events
  "sqs:SendMessage",
  "sqs:ReceiveMessage",
  "sqs:DeleteMessage",
  "sqs:CreateQueue",
  "sqs:GetQueueAttributes",
  "sns:Publish",
  "sns:Subscribe",
  "sns:CreateTopic",
  "sns:Unsubscribe",
  "events:PutEvents",
  "events:PutRule",
  "events:PutTargets",
  "kinesis:PutRecord",
  "kinesis:GetRecords",
  // Identity / security
  "iam:GetUser",
  "iam:ListUsers",
  "iam:PassRole",
  "iam:CreateRole",
  "iam:AttachRolePolicy",
  "iam:CreatePolicy",
  "sts:AssumeRole",
  "sts:GetCallerIdentity",
  "cognito-idp:AdminInitiateAuth",
  "cognito-idp:AdminCreateUser",
  // Crypto / secrets / config
  "kms:Encrypt",
  "kms:Decrypt",
  "kms:GenerateDataKey",
  "kms:DescribeKey",
  "secretsmanager:GetSecretValue",
  "secretsmanager:CreateSecret",
  "ssm:GetParameter",
  "ssm:PutParameter",
  "ssm:GetParameters",
  // Observability
  "logs:CreateLogGroup",
  "logs:CreateLogStream",
  "logs:PutLogEvents",
  "logs:DescribeLogGroups",
  "cloudwatch:PutMetricData",
  "cloudwatch:GetMetricData",
  // API / networking
  "apigateway:GET",
  "apigateway:POST",
  "apigateway:PUT",
  "apigateway:DELETE",
  // Data / search
  "es:ESHttpGet",
  "es:ESHttpPost",
  "athena:StartQueryExecution",
  "glue:GetTable",
  // Infra
  "rds:DescribeDBInstances",
  "rds:CreateDBInstance",
  "ecr:GetAuthorizationToken",
  "ecr:BatchGetImage",
  "ecr:PutImage",
  "cloudformation:CreateStack",
  "cloudformation:DescribeStacks",
  // AI / ML
  "bedrock:InvokeModel",
  "bedrock:InvokeModelWithResponseStream",
  "bedrock:Converse",
];
