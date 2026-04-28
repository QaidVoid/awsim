/**
 * API Gateway (REST APIs) typed client.
 *
 * Wraps the AWSim API Gateway HTTP REST endpoints
 * (`/restapis/...`) with typed, camel-cased shapes so the UI never has to
 * touch fetch headers or AWS-cased payloads directly.
 */

import { ENDPOINT, amzDate, authHeader, loggedFetch } from "$lib/aws";

const SERVICE = "apigateway";

// ---- Shared headers ----

function apigwHeaders(): Record<string, string> {
  return {
    Authorization: authHeader(SERVICE),
    "X-Amz-Date": amzDate(),
  };
}

async function apigwFetch<T>(
  method: "GET" | "POST" | "PUT" | "DELETE" | "PATCH",
  path: string,
  body?: unknown,
): Promise<T> {
  const init: RequestInit = {
    method,
    headers: {
      ...apigwHeaders(),
      ...(body !== undefined ? { "Content-Type": "application/json" } : {}),
    },
  };
  if (body !== undefined) {
    init.body = typeof body === "string" ? body : JSON.stringify(body);
  }
  const res = await loggedFetch(
    SERVICE,
    path,
    method,
    `${ENDPOINT}${path}`,
    init,
  );
  if (!res.ok) {
    const text = await res.text();
    throw new Error(
      `API Gateway ${method} ${path} failed (HTTP ${res.status}): ${text}`,
    );
  }
  const text = await res.text();
  return (text ? JSON.parse(text) : {}) as T;
}

// ---- Types ----

export interface RestApi {
  id: string;
  name: string;
  description: string;
  createdDate: string;
  version: string;
  apiKeySource: string;
  endpointTypes: string[];
}

export interface Resource {
  id: string;
  parentId: string;
  pathPart: string;
  path: string;
  resourceMethods: string[];
}

export interface Method {
  httpMethod: string;
  authorizationType: string;
  authorizerId: string;
  apiKeyRequired: boolean;
  requestParameters: Record<string, boolean>;
  methodIntegration: Integration | null;
}

export interface Integration {
  type: string;
  httpMethod: string;
  uri: string;
  connectionType: string;
  passthroughBehavior: string;
  timeoutInMillis: number;
  cacheNamespace: string;
}

export interface Stage {
  stageName: string;
  deploymentId: string;
  description: string;
  cacheClusterEnabled: boolean;
  createdDate: string;
  lastUpdatedDate: string;
  variables: Record<string, string>;
}

export interface Deployment {
  id: string;
  description: string;
  createdDate: string;
}

export interface Authorizer {
  id: string;
  name: string;
  type: string;
  authType: string;
  authorizerUri: string;
  identitySource: string;
}

// ---- Raw response shapes ----

interface RawRestApi {
  id?: string;
  name?: string;
  description?: string;
  createdDate?: number | string;
  version?: string;
  apiKeySource?: string;
  endpointConfiguration?: { types?: string[] };
}

interface RawListRestApis {
  items?: RawRestApi[];
}

interface RawResource {
  id?: string;
  parentId?: string;
  pathPart?: string;
  path?: string;
  resourceMethods?: Record<string, unknown>;
}

interface RawListResources {
  items?: RawResource[];
}

interface RawMethod {
  httpMethod?: string;
  authorizationType?: string;
  authorizerId?: string;
  apiKeyRequired?: boolean;
  requestParameters?: Record<string, boolean>;
  methodIntegration?: RawIntegration;
}

interface RawIntegration {
  type?: string;
  httpMethod?: string;
  uri?: string;
  connectionType?: string;
  passthroughBehavior?: string;
  timeoutInMillis?: number;
  cacheNamespace?: string;
}

interface RawStage {
  stageName?: string;
  deploymentId?: string;
  description?: string;
  cacheClusterEnabled?: boolean;
  createdDate?: number | string;
  lastUpdatedDate?: number | string;
  variables?: Record<string, string>;
}

interface RawListStages {
  item?: RawStage[];
}

interface RawDeployment {
  id?: string;
  description?: string;
  createdDate?: number | string;
}

interface RawListDeployments {
  items?: RawDeployment[];
}

interface RawAuthorizer {
  id?: string;
  name?: string;
  type?: string;
  authType?: string;
  authorizerUri?: string;
  identitySource?: string;
}

interface RawListAuthorizers {
  items?: RawAuthorizer[];
}

// ---- Mappers ----

function isoDate(v: number | string | undefined): string {
  if (v === undefined || v === null) return "";
  if (typeof v === "number") return new Date(v * 1000).toISOString();
  // Already a string; pass through.
  return v;
}

function mapRestApi(r: RawRestApi): RestApi {
  return {
    id: r.id ?? "",
    name: r.name ?? "",
    description: r.description ?? "",
    createdDate: isoDate(r.createdDate),
    version: r.version ?? "",
    apiKeySource: r.apiKeySource ?? "",
    endpointTypes: r.endpointConfiguration?.types ?? [],
  };
}

function mapResource(r: RawResource): Resource {
  return {
    id: r.id ?? "",
    parentId: r.parentId ?? "",
    pathPart: r.pathPart ?? "",
    path: r.path ?? "",
    resourceMethods: Object.keys(r.resourceMethods ?? {}),
  };
}

function mapIntegration(r: RawIntegration | undefined): Integration | null {
  if (!r) return null;
  return {
    type: r.type ?? "",
    httpMethod: r.httpMethod ?? "",
    uri: r.uri ?? "",
    connectionType: r.connectionType ?? "",
    passthroughBehavior: r.passthroughBehavior ?? "",
    timeoutInMillis: r.timeoutInMillis ?? 0,
    cacheNamespace: r.cacheNamespace ?? "",
  };
}

function mapMethod(r: RawMethod): Method {
  return {
    httpMethod: r.httpMethod ?? "",
    authorizationType: r.authorizationType ?? "NONE",
    authorizerId: r.authorizerId ?? "",
    apiKeyRequired: r.apiKeyRequired ?? false,
    requestParameters: r.requestParameters ?? {},
    methodIntegration: mapIntegration(r.methodIntegration),
  };
}

function mapStage(r: RawStage): Stage {
  return {
    stageName: r.stageName ?? "",
    deploymentId: r.deploymentId ?? "",
    description: r.description ?? "",
    cacheClusterEnabled: r.cacheClusterEnabled ?? false,
    createdDate: isoDate(r.createdDate),
    lastUpdatedDate: isoDate(r.lastUpdatedDate),
    variables: r.variables ?? {},
  };
}

function mapDeployment(r: RawDeployment): Deployment {
  return {
    id: r.id ?? "",
    description: r.description ?? "",
    createdDate: isoDate(r.createdDate),
  };
}

function mapAuthorizer(r: RawAuthorizer): Authorizer {
  return {
    id: r.id ?? "",
    name: r.name ?? "",
    type: r.type ?? "",
    authType: r.authType ?? "",
    authorizerUri: r.authorizerUri ?? "",
    identitySource: r.identitySource ?? "",
  };
}

// ---- Operations ----

export async function getRestApis(): Promise<RestApi[]> {
  const data = await apigwFetch<RawListRestApis>("GET", "/restapis");
  return (data.items ?? []).map(mapRestApi);
}

export async function getRestApi(id: string): Promise<RestApi> {
  const data = await apigwFetch<RawRestApi>(
    "GET",
    `/restapis/${encodeURIComponent(id)}`,
  );
  return mapRestApi(data);
}

export async function createRestApi(input: {
  name: string;
  description?: string;
}): Promise<RestApi> {
  const body: Record<string, unknown> = { name: input.name };
  if (input.description) body["description"] = input.description;
  const data = await apigwFetch<RawRestApi>("POST", "/restapis", body);
  return mapRestApi(data);
}

export async function deleteRestApi(id: string): Promise<void> {
  await apigwFetch<unknown>("DELETE", `/restapis/${encodeURIComponent(id)}`);
}

export async function getResources(restApiId: string): Promise<Resource[]> {
  const data = await apigwFetch<RawListResources>(
    "GET",
    `/restapis/${encodeURIComponent(restApiId)}/resources?embed=methods`,
  );
  return (data.items ?? []).map(mapResource);
}

export async function getMethod(
  restApiId: string,
  resourceId: string,
  httpMethod: string,
): Promise<Method> {
  const data = await apigwFetch<RawMethod>(
    "GET",
    `/restapis/${encodeURIComponent(restApiId)}/resources/${encodeURIComponent(resourceId)}/methods/${encodeURIComponent(httpMethod)}`,
  );
  return mapMethod(data);
}

export async function getIntegration(
  restApiId: string,
  resourceId: string,
  httpMethod: string,
): Promise<Integration | null> {
  try {
    const data = await apigwFetch<RawIntegration>(
      "GET",
      `/restapis/${encodeURIComponent(restApiId)}/resources/${encodeURIComponent(resourceId)}/methods/${encodeURIComponent(httpMethod)}/integration`,
    );
    return mapIntegration(data);
  } catch {
    return null;
  }
}

export async function getStages(restApiId: string): Promise<Stage[]> {
  const data = await apigwFetch<RawListStages>(
    "GET",
    `/restapis/${encodeURIComponent(restApiId)}/stages`,
  );
  return (data.item ?? []).map(mapStage);
}

export async function getDeployments(restApiId: string): Promise<Deployment[]> {
  const data = await apigwFetch<RawListDeployments>(
    "GET",
    `/restapis/${encodeURIComponent(restApiId)}/deployments`,
  );
  return (data.items ?? []).map(mapDeployment);
}

export async function createDeployment(
  restApiId: string,
  input: { stageName?: string; description?: string },
): Promise<Deployment> {
  const body: Record<string, unknown> = {};
  if (input.stageName) body["stageName"] = input.stageName;
  if (input.description) body["description"] = input.description;
  const data = await apigwFetch<RawDeployment>(
    "POST",
    `/restapis/${encodeURIComponent(restApiId)}/deployments`,
    body,
  );
  return mapDeployment(data);
}

export async function getAuthorizers(restApiId: string): Promise<Authorizer[]> {
  const data = await apigwFetch<RawListAuthorizers>(
    "GET",
    `/restapis/${encodeURIComponent(restApiId)}/authorizers`,
  );
  return (data.items ?? []).map(mapAuthorizer);
}

/**
 * Build the public stage invoke URL for a REST API on this AWSim host.
 * AWS standard form: https://{id}.execute-api.{region}.amazonaws.com/{stage}
 * AWSim form:   {endpoint}/restapis/{id}/{stage}/_user_request_/
 */
export function stageInvokeUrl(restApiId: string, stage: string): string {
  return `${ENDPOINT}/restapis/${restApiId}/${stage}/_user_request_`;
}
