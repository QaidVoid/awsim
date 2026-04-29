/**
 * Typed AppConfig (control plane) API client. RestJson1.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "appconfig";

export interface Application {
  id: string;
  name: string;
  description?: string;
}

export interface Environment {
  id: string;
  applicationId: string;
  name: string;
  description?: string;
  state: string;
}

export interface ConfigProfile {
  id: string;
  applicationId: string;
  name: string;
  locationUri: string;
  type: string;
  description?: string;
}

export interface HostedVersion {
  applicationId: string;
  configurationProfileId: string;
  versionNumber: number;
  description?: string;
  contentType: string;
  contentBase64: string;
  versionLabel?: string;
}

export interface Deployment {
  applicationId: string;
  environmentId: string;
  deploymentNumber: number;
  configurationProfileId: string;
  deploymentStrategyId: string;
  configurationVersion: string;
  state: string;
  percentageComplete: number;
}

export interface DeploymentStrategy {
  id: string;
  name: string;
  deploymentDurationInMinutes: number;
  growthFactor: number;
  finalBakeTimeInMinutes: number;
  growthType: string;
  replicateTo: string;
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
  method: "GET" | "POST" | "PUT" | "DELETE" | "PATCH",
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
    throw new Error(`AppConfig ${action} failed (HTTP ${res.status}): ${msg}`);
  }
  return (text ? JSON.parse(text) : {}) as T;
}

interface RawApp {
  Id: string;
  Name: string;
  Description?: string;
}

interface RawEnv {
  Id: string;
  ApplicationId: string;
  Name: string;
  Description?: string;
  State: string;
}

interface RawProfile {
  Id: string;
  ApplicationId: string;
  Name: string;
  LocationUri: string;
  Type: string;
  Description?: string;
}

interface RawVersion {
  ApplicationId: string;
  ConfigurationProfileId: string;
  VersionNumber: number;
  Description?: string;
  ContentType: string;
  Content: string;
  VersionLabel?: string;
}

interface RawDeployment {
  ApplicationId: string;
  EnvironmentId: string;
  DeploymentNumber: number;
  ConfigurationProfileId: string;
  DeploymentStrategyId: string;
  ConfigurationVersion: string;
  State: string;
  PercentageComplete: number;
}

interface RawStrategy {
  Id: string;
  Name: string;
  DeploymentDurationInMinutes: number;
  GrowthFactor: number;
  FinalBakeTimeInMinutes: number;
  GrowthType: string;
  ReplicateTo: string;
}

const fromApp = (r: RawApp): Application => ({
  id: r.Id,
  name: r.Name,
  description: r.Description,
});
const fromEnv = (r: RawEnv): Environment => ({
  id: r.Id,
  applicationId: r.ApplicationId,
  name: r.Name,
  description: r.Description,
  state: r.State,
});
const fromProfile = (r: RawProfile): ConfigProfile => ({
  id: r.Id,
  applicationId: r.ApplicationId,
  name: r.Name,
  locationUri: r.LocationUri,
  type: r.Type,
  description: r.Description,
});
const fromVersion = (r: RawVersion): HostedVersion => ({
  applicationId: r.ApplicationId,
  configurationProfileId: r.ConfigurationProfileId,
  versionNumber: r.VersionNumber,
  description: r.Description,
  contentType: r.ContentType,
  contentBase64: r.Content,
  versionLabel: r.VersionLabel,
});
const fromDeployment = (r: RawDeployment): Deployment => ({
  applicationId: r.ApplicationId,
  environmentId: r.EnvironmentId,
  deploymentNumber: r.DeploymentNumber,
  configurationProfileId: r.ConfigurationProfileId,
  deploymentStrategyId: r.DeploymentStrategyId,
  configurationVersion: r.ConfigurationVersion,
  state: r.State,
  percentageComplete: r.PercentageComplete,
});
const fromStrategy = (r: RawStrategy): DeploymentStrategy => ({
  id: r.Id,
  name: r.Name,
  deploymentDurationInMinutes: r.DeploymentDurationInMinutes,
  growthFactor: r.GrowthFactor,
  finalBakeTimeInMinutes: r.FinalBakeTimeInMinutes,
  growthType: r.GrowthType,
  replicateTo: r.ReplicateTo,
});

// ---------- Applications ----------
export async function listApplications(): Promise<Application[]> {
  const data = await request<{ Items?: RawApp[] }>(
    "ListApplications",
    "GET",
    "/applications",
  );
  return (data.Items ?? []).map(fromApp);
}

export async function createApplication(
  name: string,
  description?: string,
): Promise<Application> {
  const body: Record<string, unknown> = { Name: name };
  if (description) body.Description = description;
  const r = await request<RawApp>(
    "CreateApplication",
    "POST",
    "/applications",
    body,
  );
  return fromApp(r);
}

export async function deleteApplication(id: string): Promise<void> {
  await request<unknown>(
    "DeleteApplication",
    "DELETE",
    `/applications/${encodeURIComponent(id)}`,
  );
}

// ---------- Environments ----------
export async function listEnvironments(appId: string): Promise<Environment[]> {
  const data = await request<{ Items?: RawEnv[] }>(
    "ListEnvironments",
    "GET",
    `/applications/${encodeURIComponent(appId)}/environments`,
  );
  return (data.Items ?? []).map(fromEnv);
}

export async function createEnvironment(
  appId: string,
  name: string,
): Promise<Environment> {
  const r = await request<RawEnv>(
    "CreateEnvironment",
    "POST",
    `/applications/${encodeURIComponent(appId)}/environments`,
    { Name: name },
  );
  return fromEnv(r);
}

export async function deleteEnvironment(
  appId: string,
  envId: string,
): Promise<void> {
  await request<unknown>(
    "DeleteEnvironment",
    "DELETE",
    `/applications/${encodeURIComponent(appId)}/environments/${encodeURIComponent(envId)}`,
  );
}

// ---------- Profiles ----------
export async function listProfiles(appId: string): Promise<ConfigProfile[]> {
  const data = await request<{ Items?: RawProfile[] }>(
    "ListConfigurationProfiles",
    "GET",
    `/applications/${encodeURIComponent(appId)}/configurationprofiles`,
  );
  return (data.Items ?? []).map(fromProfile);
}

export async function createProfile(
  appId: string,
  name: string,
  locationUri = "hosted",
): Promise<ConfigProfile> {
  const r = await request<RawProfile>(
    "CreateConfigurationProfile",
    "POST",
    `/applications/${encodeURIComponent(appId)}/configurationprofiles`,
    { Name: name, LocationUri: locationUri },
  );
  return fromProfile(r);
}

export async function deleteProfile(
  appId: string,
  profileId: string,
): Promise<void> {
  await request<unknown>(
    "DeleteConfigurationProfile",
    "DELETE",
    `/applications/${encodeURIComponent(appId)}/configurationprofiles/${encodeURIComponent(profileId)}`,
  );
}

// ---------- Hosted versions ----------
export async function createHostedVersion(input: {
  appId: string;
  profileId: string;
  contentBase64: string;
  contentType?: string;
  description?: string;
  versionLabel?: string;
}): Promise<HostedVersion> {
  const body: Record<string, unknown> = {
    Content: input.contentBase64,
    ContentType: input.contentType ?? "application/json",
  };
  if (input.description) body.Description = input.description;
  if (input.versionLabel) body.VersionLabel = input.versionLabel;
  const r = await request<RawVersion>(
    "CreateHostedConfigurationVersion",
    "POST",
    `/applications/${encodeURIComponent(input.appId)}/configurationprofiles/${encodeURIComponent(input.profileId)}/hostedconfigurationversions`,
    body,
  );
  return fromVersion(r);
}

// ---------- Deployments ----------
export async function listDeployments(
  appId: string,
  envId: string,
): Promise<Deployment[]> {
  const data = await request<{ Items?: RawDeployment[] }>(
    "ListDeployments",
    "GET",
    `/applications/${encodeURIComponent(appId)}/environments/${encodeURIComponent(envId)}/deployments`,
  );
  return (data.Items ?? []).map(fromDeployment);
}

export async function startDeployment(input: {
  appId: string;
  envId: string;
  profileId: string;
  strategyId: string;
  configurationVersion: string;
}): Promise<Deployment> {
  const r = await request<RawDeployment>(
    "StartDeployment",
    "POST",
    `/applications/${encodeURIComponent(input.appId)}/environments/${encodeURIComponent(input.envId)}/deployments`,
    {
      ConfigurationProfileId: input.profileId,
      DeploymentStrategyId: input.strategyId,
      ConfigurationVersion: input.configurationVersion,
    },
  );
  return fromDeployment(r);
}

// ---------- Strategies ----------
export async function listStrategies(): Promise<DeploymentStrategy[]> {
  const data = await request<{ Items?: RawStrategy[] }>(
    "ListDeploymentStrategies",
    "GET",
    "/deploymentstrategies",
  );
  return (data.Items ?? []).map(fromStrategy);
}
