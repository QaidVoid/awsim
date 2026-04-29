/**
 * Typed Amazon Pinpoint API client. RestJson1.
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "mobiletargeting";

export interface App {
  id: string;
  name: string;
  arn: string;
  creationDate: string;
}

export interface Endpoint {
  id: string;
  applicationId: string;
  address?: string;
  channelType?: string;
  endpointStatus: string;
  effectiveDate: string;
  optOut: string;
  attributes?: Record<string, unknown>;
}

export interface Segment {
  id: string;
  applicationId: string;
  name: string;
  segmentType: string;
  version: number;
  creationDate: string;
}

export interface Campaign {
  id: string;
  applicationId: string;
  name: string;
  state: string;
  segmentId: string;
  segmentVersion: number;
  creationDate: string;
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
    throw new Error(`Pinpoint ${action} failed (HTTP ${res.status}): ${msg}`);
  }
  return (text ? JSON.parse(text) : {}) as T;
}

interface RawApp {
  Id: string;
  Name: string;
  Arn: string;
  CreationDate: string;
}

interface RawEndpoint {
  Id: string;
  ApplicationId: string;
  Address?: string;
  ChannelType?: string;
  EndpointStatus: string;
  EffectiveDate: string;
  OptOut: string;
  Attributes?: Record<string, unknown>;
}

interface RawSegment {
  Id: string;
  ApplicationId: string;
  Name: string;
  SegmentType: string;
  Version: number;
  CreationDate: string;
}

interface RawCampaign {
  Id: string;
  ApplicationId: string;
  Name: string;
  State: { CampaignStatus: string };
  SegmentId: string;
  SegmentVersion: number;
  CreationDate: string;
}

const fromApp = (r: RawApp): App => ({
  id: r.Id,
  name: r.Name,
  arn: r.Arn,
  creationDate: r.CreationDate,
});

const fromEndpoint = (r: RawEndpoint): Endpoint => ({
  id: r.Id,
  applicationId: r.ApplicationId,
  address: r.Address,
  channelType: r.ChannelType,
  endpointStatus: r.EndpointStatus,
  effectiveDate: r.EffectiveDate,
  optOut: r.OptOut,
  attributes: r.Attributes,
});

const fromSegment = (r: RawSegment): Segment => ({
  id: r.Id,
  applicationId: r.ApplicationId,
  name: r.Name,
  segmentType: r.SegmentType,
  version: r.Version,
  creationDate: r.CreationDate,
});

const fromCampaign = (r: RawCampaign): Campaign => ({
  id: r.Id,
  applicationId: r.ApplicationId,
  name: r.Name,
  state: r.State.CampaignStatus,
  segmentId: r.SegmentId,
  segmentVersion: r.SegmentVersion,
  creationDate: r.CreationDate,
});

export async function listApps(): Promise<App[]> {
  const data = await request<{ ApplicationsResponse?: { Item?: RawApp[] } }>(
    "GetApps",
    "GET",
    "/v1/apps",
  );
  return (data.ApplicationsResponse?.Item ?? []).map(fromApp);
}

export async function createApp(name: string): Promise<App> {
  const data = await request<{ ApplicationResponse: RawApp }>(
    "CreateApp",
    "POST",
    "/v1/apps",
    { CreateApplicationRequest: { Name: name } },
  );
  return fromApp(data.ApplicationResponse);
}

export async function deleteApp(appId: string): Promise<void> {
  await request<unknown>(
    "DeleteApp",
    "DELETE",
    `/v1/apps/${encodeURIComponent(appId)}`,
  );
}

export async function getEndpoint(
  appId: string,
  endpointId: string,
): Promise<Endpoint> {
  const data = await request<{ EndpointResponse: RawEndpoint }>(
    "GetEndpoint",
    "GET",
    `/v1/apps/${encodeURIComponent(appId)}/endpoints/${encodeURIComponent(endpointId)}`,
  );
  return fromEndpoint(data.EndpointResponse);
}

export async function updateEndpoint(input: {
  appId: string;
  endpointId: string;
  channelType: string;
  address: string;
}): Promise<void> {
  await request<unknown>(
    "UpdateEndpoint",
    "PUT",
    `/v1/apps/${encodeURIComponent(input.appId)}/endpoints/${encodeURIComponent(input.endpointId)}`,
    {
      EndpointRequest: {
        ChannelType: input.channelType,
        Address: input.address,
        EndpointStatus: "ACTIVE",
      },
    },
  );
}

export async function deleteEndpoint(
  appId: string,
  endpointId: string,
): Promise<void> {
  await request<unknown>(
    "DeleteEndpoint",
    "DELETE",
    `/v1/apps/${encodeURIComponent(appId)}/endpoints/${encodeURIComponent(endpointId)}`,
  );
}

export async function listSegments(appId: string): Promise<Segment[]> {
  const data = await request<{ SegmentsResponse?: { Item?: RawSegment[] } }>(
    "GetSegments",
    "GET",
    `/v1/apps/${encodeURIComponent(appId)}/segments`,
  );
  return (data.SegmentsResponse?.Item ?? []).map(fromSegment);
}

export async function createSegment(
  appId: string,
  name: string,
): Promise<Segment> {
  const data = await request<{ SegmentResponse: RawSegment }>(
    "CreateSegment",
    "POST",
    `/v1/apps/${encodeURIComponent(appId)}/segments`,
    { WriteSegmentRequest: { Name: name } },
  );
  return fromSegment(data.SegmentResponse);
}

export async function deleteSegment(
  appId: string,
  segmentId: string,
): Promise<void> {
  await request<unknown>(
    "DeleteSegment",
    "DELETE",
    `/v1/apps/${encodeURIComponent(appId)}/segments/${encodeURIComponent(segmentId)}`,
  );
}

export async function listCampaigns(appId: string): Promise<Campaign[]> {
  const data = await request<{ CampaignsResponse?: { Item?: RawCampaign[] } }>(
    "GetCampaigns",
    "GET",
    `/v1/apps/${encodeURIComponent(appId)}/campaigns`,
  );
  return (data.CampaignsResponse?.Item ?? []).map(fromCampaign);
}

export async function createCampaign(input: {
  appId: string;
  name: string;
  segmentId: string;
}): Promise<Campaign> {
  const data = await request<{ CampaignResponse: RawCampaign }>(
    "CreateCampaign",
    "POST",
    `/v1/apps/${encodeURIComponent(input.appId)}/campaigns`,
    {
      WriteCampaignRequest: {
        Name: input.name,
        SegmentId: input.segmentId,
        SegmentVersion: 1,
        MessageConfiguration: {
          DefaultMessage: { Body: "Hello from AWSim Pinpoint UI" },
        },
      },
    },
  );
  return fromCampaign(data.CampaignResponse);
}

export async function deleteCampaign(
  appId: string,
  campaignId: string,
): Promise<void> {
  await request<unknown>(
    "DeleteCampaign",
    "DELETE",
    `/v1/apps/${encodeURIComponent(appId)}/campaigns/${encodeURIComponent(campaignId)}`,
  );
}
