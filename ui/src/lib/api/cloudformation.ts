/**
 * CloudFormation API client.
 *
 * Wraps the LocalStack-compatible CloudFormation Query API
 * (`Action=<action>&Version=2010-05-15`). All XML responses are parsed
 * into camel-cased shapes consumed directly by the UI.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "cloudformation";
const VERSION = "2010-05-15";

async function cfRequest(
  action: string,
  params: Record<string, string> = {},
): Promise<string> {
  const body = new URLSearchParams({ Action: action, Version: VERSION });
  for (const [k, v] of Object.entries(params)) body.set(k, v);
  const res = await loggedFetch(SERVICE, action, "POST", ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-www-form-urlencoded",
      Authorization: authHeader(SERVICE),
      "X-Amz-Date": amzDate(),
    },
    body: body.toString(),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`HTTP ${res.status}: ${text || res.statusText}`);
  return text;
}

// -- XML helpers --

function tagText(xml: string, tag: string): string {
  const m = xml.match(new RegExp(`<${tag}>([\\s\\S]*?)</${tag}>`));
  return m ? decodeXml(m[1]) : "";
}

function tagOptional(xml: string, tag: string): string | undefined {
  const m = xml.match(new RegExp(`<${tag}>([\\s\\S]*?)</${tag}>`));
  return m ? decodeXml(m[1]) : undefined;
}

function members(xml: string): string[] {
  const out: string[] = [];
  const re = /<member>([\s\S]*?)<\/member>/g;
  let m;
  while ((m = re.exec(xml)) !== null) out.push(m[1]);
  return out;
}

function decodeXml(s: string): string {
  return s
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&#10;/g, "\n")
    .replace(/&#13;/g, "\r")
    .replace(/&amp;/g, "&");
}

// -- Types --

export interface Stack {
  stackName: string;
  stackId: string;
  stackStatus: string;
  stackStatusReason?: string;
  creationTime: string;
  lastUpdatedTime?: string;
  description?: string;
  parameters: { key: string; value: string }[];
  outputs: { key: string; value: string; description?: string }[];
  tags: { key: string; value: string }[];
  capabilities: string[];
  roleArn?: string;
  disableRollback?: boolean;
}

export interface StackSummary {
  stackName: string;
  stackId: string;
  stackStatus: string;
  creationTime: string;
  lastUpdatedTime?: string;
  templateDescription?: string;
}

export interface StackResource {
  logicalResourceId: string;
  physicalResourceId: string;
  resourceType: string;
  resourceStatus: string;
  resourceStatusReason?: string;
  timestamp?: string;
}

export interface StackEvent {
  eventId: string;
  stackName: string;
  logicalResourceId: string;
  physicalResourceId?: string;
  resourceType: string;
  resourceStatus: string;
  resourceStatusReason?: string;
  timestamp: string;
}

export interface ChangeSetSummary {
  changeSetId: string;
  changeSetName: string;
  status: string;
  executionStatus?: string;
  description?: string;
  creationTime?: string;
}

export interface Template {
  body: string;
  stagesAvailable: string[];
}

// -- Operations --

export async function listStacks(): Promise<{ stacks: StackSummary[] }> {
  const xml = await cfRequest("ListStacks");
  return {
    stacks: members(xml).map((m) => ({
      stackName: tagText(m, "StackName"),
      stackId: tagText(m, "StackId"),
      stackStatus: tagText(m, "StackStatus"),
      creationTime: tagText(m, "CreationTime"),
      lastUpdatedTime: tagOptional(m, "LastUpdatedTime"),
      templateDescription: tagOptional(m, "TemplateDescription"),
    })),
  };
}

function parseStack(xml: string): Stack {
  const params: { key: string; value: string }[] = [];
  const paramsBlock = xml.match(/<Parameters>([\s\S]*?)<\/Parameters>/);
  if (paramsBlock) {
    for (const m of members(paramsBlock[1])) {
      params.push({
        key: tagText(m, "ParameterKey"),
        value: tagText(m, "ParameterValue"),
      });
    }
  }
  const outputs: { key: string; value: string; description?: string }[] = [];
  const outputsBlock = xml.match(/<Outputs>([\s\S]*?)<\/Outputs>/);
  if (outputsBlock) {
    for (const m of members(outputsBlock[1])) {
      outputs.push({
        key: tagText(m, "OutputKey"),
        value: tagText(m, "OutputValue"),
        description: tagOptional(m, "Description"),
      });
    }
  }
  const tags: { key: string; value: string }[] = [];
  const tagsBlock = xml.match(/<Tags>([\s\S]*?)<\/Tags>/);
  if (tagsBlock) {
    for (const m of members(tagsBlock[1])) {
      tags.push({ key: tagText(m, "Key"), value: tagText(m, "Value") });
    }
  }
  const capabilities: string[] = [];
  const capBlock = xml.match(/<Capabilities>([\s\S]*?)<\/Capabilities>/);
  if (capBlock) {
    const re = /<member>([\s\S]*?)<\/member>/g;
    let mm;
    while ((mm = re.exec(capBlock[1])) !== null) capabilities.push(mm[1]);
  }
  return {
    stackName: tagText(xml, "StackName"),
    stackId: tagText(xml, "StackId"),
    stackStatus: tagText(xml, "StackStatus"),
    stackStatusReason: tagOptional(xml, "StackStatusReason"),
    creationTime: tagText(xml, "CreationTime"),
    lastUpdatedTime: tagOptional(xml, "LastUpdatedTime"),
    description: tagOptional(xml, "Description"),
    parameters: params,
    outputs,
    tags,
    capabilities,
    roleArn: tagOptional(xml, "RoleARN"),
    disableRollback: tagOptional(xml, "DisableRollback") === "true",
  };
}

export async function describeStacks(
  stackName?: string,
): Promise<{ stacks: Stack[] }> {
  const params: Record<string, string> = stackName
    ? { StackName: stackName }
    : {};
  const xml = await cfRequest("DescribeStacks", params);
  const block = xml.match(/<Stacks>([\s\S]*?)<\/Stacks>/);
  if (!block) return { stacks: [] };
  return { stacks: members(block[1]).map(parseStack) };
}

export async function describeStack(stackName: string): Promise<Stack | null> {
  const { stacks } = await describeStacks(stackName);
  return stacks[0] ?? null;
}

export async function describeStackResources(
  stackName: string,
): Promise<{ resources: StackResource[] }> {
  const xml = await cfRequest("DescribeStackResources", {
    StackName: stackName,
  });
  return {
    resources: members(xml).map((m) => ({
      logicalResourceId: tagText(m, "LogicalResourceId"),
      physicalResourceId: tagText(m, "PhysicalResourceId"),
      resourceType: tagText(m, "ResourceType"),
      resourceStatus: tagText(m, "ResourceStatus"),
      resourceStatusReason: tagOptional(m, "ResourceStatusReason"),
      timestamp: tagOptional(m, "Timestamp"),
    })),
  };
}

export async function describeStackEvents(
  stackName: string,
): Promise<{ events: StackEvent[] }> {
  const xml = await cfRequest("DescribeStackEvents", { StackName: stackName });
  return {
    events: members(xml).map((m) => ({
      eventId: tagText(m, "EventId"),
      stackName: tagText(m, "StackName"),
      logicalResourceId: tagText(m, "LogicalResourceId"),
      physicalResourceId: tagOptional(m, "PhysicalResourceId"),
      resourceType: tagText(m, "ResourceType"),
      resourceStatus: tagText(m, "ResourceStatus"),
      resourceStatusReason: tagOptional(m, "ResourceStatusReason"),
      timestamp: tagText(m, "Timestamp"),
    })),
  };
}

export async function listChangeSets(
  stackName: string,
): Promise<{ changeSets: ChangeSetSummary[] }> {
  const xml = await cfRequest("ListChangeSets", { StackName: stackName });
  return {
    changeSets: members(xml).map((m) => ({
      changeSetId: tagText(m, "ChangeSetId"),
      changeSetName: tagText(m, "ChangeSetName"),
      status: tagText(m, "Status"),
      executionStatus: tagOptional(m, "ExecutionStatus"),
      description: tagOptional(m, "Description"),
      creationTime: tagOptional(m, "CreationTime"),
    })),
  };
}

export async function getTemplate(stackName: string): Promise<Template> {
  const xml = await cfRequest("GetTemplate", { StackName: stackName });
  const body = tagText(xml, "TemplateBody");
  const stages: string[] = [];
  const stagesBlock = xml.match(
    /<StagesAvailable>([\s\S]*?)<\/StagesAvailable>/,
  );
  if (stagesBlock) {
    const re = /<member>([\s\S]*?)<\/member>/g;
    let mm;
    while ((mm = re.exec(stagesBlock[1])) !== null) stages.push(mm[1]);
  }
  return { body, stagesAvailable: stages };
}

export interface CreateStackInput {
  stackName: string;
  templateBody: string;
  parameters?: { key: string; value: string }[];
  capabilities?: string[];
  tags?: { key: string; value: string }[];
}

export async function createStack(input: CreateStackInput): Promise<void> {
  const params: Record<string, string> = {
    StackName: input.stackName,
    TemplateBody: input.templateBody,
  };
  (input.parameters ?? []).forEach((p, i) => {
    params[`Parameters.member.${i + 1}.ParameterKey`] = p.key;
    params[`Parameters.member.${i + 1}.ParameterValue`] = p.value;
  });
  (input.capabilities ?? []).forEach((c, i) => {
    params[`Capabilities.member.${i + 1}`] = c;
  });
  (input.tags ?? []).forEach((t, i) => {
    params[`Tags.member.${i + 1}.Key`] = t.key;
    params[`Tags.member.${i + 1}.Value`] = t.value;
  });
  await cfRequest("CreateStack", params);
}

export interface UpdateStackInput {
  stackName: string;
  templateBody: string;
  parameters?: { key: string; value: string }[];
  capabilities?: string[];
}

export async function updateStack(input: UpdateStackInput): Promise<void> {
  const params: Record<string, string> = {
    StackName: input.stackName,
    TemplateBody: input.templateBody,
  };
  (input.parameters ?? []).forEach((p, i) => {
    params[`Parameters.member.${i + 1}.ParameterKey`] = p.key;
    params[`Parameters.member.${i + 1}.ParameterValue`] = p.value;
  });
  (input.capabilities ?? []).forEach((c, i) => {
    params[`Capabilities.member.${i + 1}`] = c;
  });
  await cfRequest("UpdateStack", params);
}

export async function deleteStack(stackName: string): Promise<void> {
  await cfRequest("DeleteStack", { StackName: stackName });
}

// -- Helpers --

export function stackStatusVariant(
  status: string,
): "default" | "secondary" | "destructive" | "outline" {
  if (status.includes("FAILED") || status.includes("ROLLBACK"))
    return "destructive";
  if (status.includes("IN_PROGRESS")) return "secondary";
  if (status === "CREATE_COMPLETE" || status === "UPDATE_COMPLETE")
    return "default";
  return "outline";
}

export function shortStackId(id: string): string {
  return id.split("/").slice(-2).join("/") || id;
}
