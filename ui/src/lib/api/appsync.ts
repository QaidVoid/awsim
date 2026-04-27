/**
 * Typed AppSync API client.
 *
 * AppSync exposes a REST-style control-plane (`/v1/apis/...`) rather than
 * the AWS JSON-RPC envelope used by most services. This module wraps the
 * relevant operations behind typed helpers.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "appsync";

// ---------- Types ----------

export interface GraphqlApi {
  apiId: string;
  name: string;
  arn: string;
  authenticationType: string;
  schemaStatus?: string;
  createdAt?: string;
  uris: Record<string, string>;
}

export interface ApiKey {
  id: string;
  description: string | null;
  expires: number;
}

export interface DataSource {
  name: string;
  type: string;
  description: string | null;
  serviceRoleArn?: string | null;
  dynamodbConfig?: { tableName: string; awsRegion: string } | null;
  lambdaConfig?: { lambdaFunctionArn: string } | null;
  httpConfig?: { endpoint: string } | null;
}

export interface Resolver {
  typeName: string;
  fieldName: string;
  dataSourceName: string | null;
  kind: string;
  resolverArn?: string | null;
  requestMappingTemplate?: string | null;
  responseMappingTemplate?: string | null;
}

export interface AppsyncFunction {
  functionId: string;
  name: string;
  description: string | null;
  dataSourceName: string;
  functionArn: string;
}

export interface ApiType {
  name: string;
  description: string | null;
  arn?: string | null;
  format: string;
  definition: string;
}

// ---------- Internal request helpers ----------

interface RawApi {
  apiId?: string;
  name?: string;
  arn?: string;
  authenticationType?: string;
  schemaStatus?: string;
  createdAt?: string;
  uris?: Record<string, string>;
}

interface RawDataSource {
  name?: string;
  type?: string;
  description?: string | null;
  serviceRoleArn?: string | null;
  dynamodbConfig?: { tableName?: string; awsRegion?: string } | null;
  lambdaConfig?: { lambdaFunctionArn?: string } | null;
  httpConfig?: { endpoint?: string } | null;
}

interface RawResolver {
  typeName?: string;
  fieldName?: string;
  dataSourceName?: string | null;
  kind?: string;
  resolverArn?: string | null;
  requestMappingTemplate?: string | null;
  responseMappingTemplate?: string | null;
}

interface RawFunction {
  functionId?: string;
  name?: string;
  description?: string | null;
  dataSourceName?: string;
  functionArn?: string;
}

interface RawType {
  name?: string;
  description?: string | null;
  arn?: string | null;
  format?: string;
  definition?: string;
}

async function appsyncFetch<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  const operation = `${method} ${path}`;
  const headers: Record<string, string> = {
    Authorization: authHeader(SERVICE),
    "X-Amz-Date": amzDate(),
  };
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const res = await loggedFetch(
    SERVICE,
    operation,
    method,
    `${ENDPOINT}${path}`,
    {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    },
  );
  if (!res.ok) {
    const text = await res.text();
    throw new Error(
      `AppSync ${operation} failed (HTTP ${res.status}): ${text}`,
    );
  }
  const text = await res.text();
  return (text ? JSON.parse(text) : {}) as T;
}

function mapApi(raw: RawApi): GraphqlApi {
  return {
    apiId: raw.apiId ?? "",
    name: raw.name ?? "",
    arn: raw.arn ?? "",
    authenticationType: raw.authenticationType ?? "",
    schemaStatus: raw.schemaStatus,
    createdAt: raw.createdAt,
    uris: raw.uris ?? {},
  };
}

function mapDataSource(raw: RawDataSource): DataSource {
  return {
    name: raw.name ?? "",
    type: raw.type ?? "",
    description: raw.description ?? null,
    serviceRoleArn: raw.serviceRoleArn ?? null,
    dynamodbConfig: raw.dynamodbConfig
      ? {
          tableName: raw.dynamodbConfig.tableName ?? "",
          awsRegion: raw.dynamodbConfig.awsRegion ?? "",
        }
      : null,
    lambdaConfig: raw.lambdaConfig
      ? { lambdaFunctionArn: raw.lambdaConfig.lambdaFunctionArn ?? "" }
      : null,
    httpConfig: raw.httpConfig
      ? { endpoint: raw.httpConfig.endpoint ?? "" }
      : null,
  };
}

function mapResolver(raw: RawResolver): Resolver {
  return {
    typeName: raw.typeName ?? "",
    fieldName: raw.fieldName ?? "",
    dataSourceName: raw.dataSourceName ?? null,
    kind: raw.kind ?? "UNIT",
    resolverArn: raw.resolverArn ?? null,
    requestMappingTemplate: raw.requestMappingTemplate ?? null,
    responseMappingTemplate: raw.responseMappingTemplate ?? null,
  };
}

function mapFunction(raw: RawFunction): AppsyncFunction {
  return {
    functionId: raw.functionId ?? "",
    name: raw.name ?? "",
    description: raw.description ?? null,
    dataSourceName: raw.dataSourceName ?? "",
    functionArn: raw.functionArn ?? "",
  };
}

function mapType(raw: RawType): ApiType {
  return {
    name: raw.name ?? "",
    description: raw.description ?? null,
    arn: raw.arn ?? null,
    format: raw.format ?? "SDL",
    definition: raw.definition ?? "",
  };
}

// ---------- Operations ----------

export async function listGraphqlApis(): Promise<GraphqlApi[]> {
  const res = await appsyncFetch<{ graphqlApis?: RawApi[] }>("GET", "/v1/apis");
  return (res.graphqlApis ?? []).map(mapApi);
}

export async function getGraphqlApi(apiId: string): Promise<GraphqlApi> {
  const res = await appsyncFetch<{ graphqlApi?: RawApi }>(
    "GET",
    `/v1/apis/${apiId}`,
  );
  return mapApi(res.graphqlApi ?? {});
}

export async function createGraphqlApi(input: {
  name: string;
  authenticationType: string;
}): Promise<GraphqlApi> {
  const res = await appsyncFetch<{ graphqlApi?: RawApi }>(
    "POST",
    "/v1/apis",
    input,
  );
  return mapApi(res.graphqlApi ?? {});
}

export async function deleteGraphqlApi(apiId: string): Promise<void> {
  await appsyncFetch<unknown>("DELETE", `/v1/apis/${apiId}`);
}

export async function listApiKeys(apiId: string): Promise<ApiKey[]> {
  const res = await appsyncFetch<{ apiKeys?: ApiKey[] }>(
    "GET",
    `/v1/apis/${apiId}/apikeys`,
  );
  return (res.apiKeys ?? []).map((k) => ({
    id: k.id,
    description: k.description ?? null,
    expires: k.expires ?? 0,
  }));
}

export async function listDataSources(apiId: string): Promise<DataSource[]> {
  const res = await appsyncFetch<{ dataSources?: RawDataSource[] }>(
    "GET",
    `/v1/apis/${apiId}/datasources`,
  );
  return (res.dataSources ?? []).map(mapDataSource);
}

export async function getDataSource(
  apiId: string,
  name: string,
): Promise<DataSource> {
  const res = await appsyncFetch<{ dataSource?: RawDataSource }>(
    "GET",
    `/v1/apis/${apiId}/datasources/${name}`,
  );
  return mapDataSource(res.dataSource ?? {});
}

export async function listFunctions(apiId: string): Promise<AppsyncFunction[]> {
  const res = await appsyncFetch<{ functions?: RawFunction[] }>(
    "GET",
    `/v1/apis/${apiId}/functions`,
  );
  return (res.functions ?? []).map(mapFunction);
}

export async function getFunction(
  apiId: string,
  functionId: string,
): Promise<AppsyncFunction> {
  const res = await appsyncFetch<{ functionConfiguration?: RawFunction }>(
    "GET",
    `/v1/apis/${apiId}/functions/${functionId}`,
  );
  return mapFunction(res.functionConfiguration ?? {});
}

export async function listTypes(apiId: string): Promise<ApiType[]> {
  const res = await appsyncFetch<{ types?: RawType[] }>(
    "GET",
    `/v1/apis/${apiId}/types?format=SDL`,
  );
  return (res.types ?? []).map(mapType);
}

export async function getType(
  apiId: string,
  typeName: string,
): Promise<ApiType> {
  const res = await appsyncFetch<{ type?: RawType }>(
    "GET",
    `/v1/apis/${apiId}/types/${typeName}?format=SDL`,
  );
  return mapType(res.type ?? {});
}

export async function listResolvers(
  apiId: string,
  typeName: string,
): Promise<Resolver[]> {
  const res = await appsyncFetch<{ resolvers?: RawResolver[] }>(
    "GET",
    `/v1/apis/${apiId}/types/${typeName}/resolvers`,
  );
  return (res.resolvers ?? []).map(mapResolver);
}

export async function getResolver(
  apiId: string,
  typeName: string,
  fieldName: string,
): Promise<Resolver> {
  const res = await appsyncFetch<{ resolver?: RawResolver }>(
    "GET",
    `/v1/apis/${apiId}/types/${typeName}/resolvers/${fieldName}`,
  );
  return mapResolver(res.resolver ?? {});
}

/** Pulls all resolvers across the canonical Query/Mutation/Subscription roots. */
export async function listAllRootResolvers(apiId: string): Promise<Resolver[]> {
  const out: Resolver[] = [];
  for (const t of ["Query", "Mutation", "Subscription"]) {
    try {
      const items = await listResolvers(apiId, t);
      out.push(...items);
    } catch {
      // ignore — type may not exist
    }
  }
  return out;
}
