/**
 * Typed Resource Groups Tagging API client.
 *
 * The service uses awsJson1.1 with the ResourceGroupsTaggingAPI_20170126
 * target prefix. We route through the generic localhost endpoint that
 * AWSim exposes at :4566.
 */

const ENDPOINT = "http://localhost:4566";
const FAKE_DATE = new Date().toISOString().slice(0, 10).replace(/-/g, "");

function authHeader(): string {
  return `AWS4-HMAC-SHA256 Credential=test/${FAKE_DATE}/us-east-1/tagging/aws4_request, SignedHeaders=host;x-amz-date, Signature=fakesignature`;
}

function amzDate(): string {
  return new Date().toISOString().replace(/[:-]/g, "").slice(0, 15) + "Z";
}

async function taggingRequest(
  action: string,
  body: unknown = {},
): Promise<unknown> {
  const res = await fetch(ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.1",
      "X-Amz-Target": `ResourceGroupsTaggingAPI_20170126.${action}`,
      Authorization: authHeader(),
      "X-Amz-Date": amzDate(),
    },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) {
    throw new Error(`Tagging ${action} failed: ${res.status} ${text}`);
  }
  return text ? JSON.parse(text) : {};
}

// ---- Types ----

export interface TagFilter {
  Key: string;
  Values?: string[];
}

export interface TaggedResource {
  ResourceARN: string;
  Tags: Array<{ Key: string; Value: string }>;
}

interface GetResourcesResponse {
  PaginationToken?: string;
  ResourceTagMappingList?: TaggedResource[];
}

interface GetTagKeysResponse {
  TagKeys?: string[];
}

interface GetTagValuesResponse {
  TagValues?: string[];
}

// ---- Operations ----

/**
 * List tagged resources, optionally filtered by tag key / value tuples or
 * service / resource type. Pagination is collapsed transparently.
 */
export async function getResources(opts: {
  tagFilters?: TagFilter[];
  resourceTypeFilters?: string[];
} = {}): Promise<TaggedResource[]> {
  const all: TaggedResource[] = [];
  let token: string | undefined;
  do {
    const resp = (await taggingRequest("GetResources", {
      TagFilters: opts.tagFilters,
      ResourceTypeFilters: opts.resourceTypeFilters,
      ResourcesPerPage: 100,
      PaginationToken: token,
    })) as GetResourcesResponse;
    if (resp.ResourceTagMappingList) {
      all.push(...resp.ResourceTagMappingList);
    }
    token = resp.PaginationToken && resp.PaginationToken.length > 0
      ? resp.PaginationToken
      : undefined;
  } while (token);
  return all;
}

export async function getTagKeys(): Promise<string[]> {
  const resp = (await taggingRequest("GetTagKeys")) as GetTagKeysResponse;
  return resp.TagKeys ?? [];
}

export async function getTagValues(key: string): Promise<string[]> {
  const resp = (await taggingRequest("GetTagValues", {
    Key: key,
  })) as GetTagValuesResponse;
  return resp.TagValues ?? [];
}

export async function tagResources(
  arns: string[],
  tags: Record<string, string>,
): Promise<void> {
  await taggingRequest("TagResources", {
    ResourceARNList: arns,
    Tags: tags,
  });
}

export async function untagResources(
  arns: string[],
  tagKeys: string[],
): Promise<void> {
  await taggingRequest("UntagResources", {
    ResourceARNList: arns,
    TagKeys: tagKeys,
  });
}
