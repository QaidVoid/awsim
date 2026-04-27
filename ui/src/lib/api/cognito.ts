/**
 * Typed Cognito API client.
 *
 * Speaks both AWSCognitoIdentityProviderService (user pools) and
 * AWSCognitoIdentityService (identity pools). Returns parsed records.
 */

const ENDPOINT = "http://localhost:4566";
const FAKE_DATE = new Date().toISOString().slice(0, 10).replace(/-/g, "");

function authHeader(service: string): string {
  return `AWS4-HMAC-SHA256 Credential=test/${FAKE_DATE}/us-east-1/${service}/aws4_request, SignedHeaders=host;x-amz-date, Signature=fakesignature`;
}

function amzDate(): string {
  return new Date().toISOString().replace(/[:-]/g, "").slice(0, 15) + "Z";
}

async function idpRequest(action: string, body: unknown): Promise<unknown> {
  const res = await fetch(ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.1",
      "X-Amz-Target": `AWSCognitoIdentityProviderService.${action}`,
      Authorization: authHeader("cognito-idp"),
      "X-Amz-Date": amzDate(),
    },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok)
    throw new Error(`Cognito ${action} failed: ${res.status} ${text}`);
  return text ? JSON.parse(text) : {};
}

async function identityRequest(
  action: string,
  body: unknown,
): Promise<unknown> {
  const res = await fetch(ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.1",
      "X-Amz-Target": `AWSCognitoIdentityService.${action}`,
      Authorization: authHeader("cognito-identity"),
      "X-Amz-Date": amzDate(),
    },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok)
    throw new Error(`Cognito-Identity ${action} failed: ${res.status} ${text}`);
  return text ? JSON.parse(text) : {};
}

// ---- Types ----

export interface UserPool {
  id: string;
  name: string;
  status: string;
  creationDate: string;
}

export interface UserPoolDetail extends UserPool {
  mfaConfiguration?: string;
  estimatedNumberOfUsers?: number;
  lambdaConfig?: Record<string, string>;
  schemaAttributes?: { name: string; type: string; required: boolean }[];
}

export interface CognitoUser {
  username: string;
  status: string;
  enabled: boolean;
  createDate: string;
  attributes: { name: string; value: string }[];
}

export interface CognitoUserSummary {
  username: string;
  status: string;
  enabled: boolean;
  createDate: string;
}

export interface CognitoGroup {
  name: string;
  description: string;
  roleArn: string;
  precedence?: number;
}

export interface CognitoAppClient {
  clientId: string;
  clientName: string;
  generateSecret?: boolean;
}

export interface CognitoAppClientDetail extends CognitoAppClient {
  clientSecret?: string;
  explicitAuthFlows: string[];
  callbackURLs: string[];
  allowedOAuthScopes: string[];
}

export interface CognitoDomain {
  domain: string;
  status?: string;
}

export interface IdentityPool {
  id: string;
  name: string;
  allowUnauthenticated: boolean;
}

export interface IdentityPoolDetail extends IdentityPool {
  cognitoIdentityProviders?: { providerName: string; clientId: string }[];
  developerProviderName?: string;
}

// ---- User Pools ----

export async function listUserPools(): Promise<UserPool[]> {
  const data = (await idpRequest("ListUserPools", { MaxResults: 60 })) as {
    UserPools?: {
      Id: string;
      Name: string;
      Status?: string;
      CreationDate?: number;
    }[];
  };
  return (data.UserPools ?? []).map((p) => ({
    id: p.Id,
    name: p.Name,
    status: p.Status ?? "ACTIVE",
    creationDate: p.CreationDate
      ? new Date(p.CreationDate * 1000).toISOString()
      : "",
  }));
}

export async function describeUserPool(id: string): Promise<UserPoolDetail> {
  const data = (await idpRequest("DescribeUserPool", { UserPoolId: id })) as {
    UserPool?: {
      Id: string;
      Name: string;
      Status?: string;
      CreationDate?: number;
      MfaConfiguration?: string;
      EstimatedNumberOfUsers?: number;
      LambdaConfig?: Record<string, string>;
      SchemaAttributes?: {
        Name: string;
        AttributeDataType: string;
        Required: boolean;
      }[];
    };
  };
  const p = data.UserPool ?? ({} as NonNullable<typeof data.UserPool>);
  return {
    id: p.Id ?? id,
    name: p.Name ?? "",
    status: p.Status ?? "",
    creationDate: p.CreationDate
      ? new Date(p.CreationDate * 1000).toISOString()
      : "",
    mfaConfiguration: p.MfaConfiguration,
    estimatedNumberOfUsers: p.EstimatedNumberOfUsers,
    lambdaConfig: p.LambdaConfig,
    schemaAttributes: (p.SchemaAttributes ?? []).map((a) => ({
      name: a.Name,
      type: a.AttributeDataType,
      required: a.Required,
    })),
  };
}

// ---- Users in pool ----

export async function listPoolUsers(
  poolId: string,
): Promise<CognitoUserSummary[]> {
  const data = (await idpRequest("ListUsers", { UserPoolId: poolId })) as {
    Users?: {
      Username: string;
      UserStatus?: string;
      Enabled?: boolean;
      UserCreateDate?: number;
    }[];
  };
  return (data.Users ?? []).map((u) => ({
    username: u.Username,
    status: u.UserStatus ?? "",
    enabled: u.Enabled ?? true,
    createDate: u.UserCreateDate
      ? new Date(u.UserCreateDate * 1000).toISOString()
      : "",
  }));
}

export async function adminGetUser(
  poolId: string,
  username: string,
): Promise<CognitoUser> {
  const data = (await idpRequest("AdminGetUser", {
    UserPoolId: poolId,
    Username: username,
  })) as {
    Username?: string;
    UserStatus?: string;
    Enabled?: boolean;
    UserCreateDate?: number;
    UserAttributes?: { Name: string; Value: string }[];
  };
  return {
    username: data.Username ?? username,
    status: data.UserStatus ?? "",
    enabled: data.Enabled ?? true,
    createDate: data.UserCreateDate
      ? new Date(data.UserCreateDate * 1000).toISOString()
      : "",
    attributes: (data.UserAttributes ?? []).map((a) => ({
      name: a.Name,
      value: a.Value,
    })),
  };
}

export async function adminConfirmSignUp(
  poolId: string,
  username: string,
): Promise<void> {
  await idpRequest("AdminConfirmSignUp", {
    UserPoolId: poolId,
    Username: username,
  });
}

export async function adminEnableUser(
  poolId: string,
  username: string,
): Promise<void> {
  await idpRequest("AdminEnableUser", {
    UserPoolId: poolId,
    Username: username,
  });
}

export async function adminDisableUser(
  poolId: string,
  username: string,
): Promise<void> {
  await idpRequest("AdminDisableUser", {
    UserPoolId: poolId,
    Username: username,
  });
}

export async function adminResetUserPassword(
  poolId: string,
  username: string,
): Promise<void> {
  await idpRequest("AdminResetUserPassword", {
    UserPoolId: poolId,
    Username: username,
  });
}

// ---- Groups ----

export async function listGroups(poolId: string): Promise<CognitoGroup[]> {
  const data = (await idpRequest("ListGroups", { UserPoolId: poolId })) as {
    Groups?: {
      GroupName: string;
      Description?: string;
      RoleArn?: string;
      Precedence?: number;
    }[];
  };
  return (data.Groups ?? []).map((g) => ({
    name: g.GroupName,
    description: g.Description ?? "",
    roleArn: g.RoleArn ?? "",
    precedence: g.Precedence,
  }));
}

// ---- App clients ----

export async function listAppClients(
  poolId: string,
): Promise<CognitoAppClient[]> {
  const data = (await idpRequest("ListUserPoolClients", {
    UserPoolId: poolId,
    MaxResults: 60,
  })) as {
    UserPoolClients?: {
      ClientId: string;
      ClientName: string;
      UserPoolId: string;
    }[];
  };
  return (data.UserPoolClients ?? []).map((c) => ({
    clientId: c.ClientId,
    clientName: c.ClientName,
  }));
}

export async function describeAppClient(
  poolId: string,
  clientId: string,
): Promise<CognitoAppClientDetail> {
  const data = (await idpRequest("DescribeUserPoolClient", {
    UserPoolId: poolId,
    ClientId: clientId,
  })) as {
    UserPoolClient?: {
      ClientId: string;
      ClientName: string;
      ClientSecret?: string;
      ExplicitAuthFlows?: string[];
      CallbackURLs?: string[];
      AllowedOAuthScopes?: string[];
    };
  };
  const c =
    data.UserPoolClient ?? ({} as NonNullable<typeof data.UserPoolClient>);
  return {
    clientId: c.ClientId ?? clientId,
    clientName: c.ClientName ?? "",
    clientSecret: c.ClientSecret,
    explicitAuthFlows: c.ExplicitAuthFlows ?? [],
    callbackURLs: c.CallbackURLs ?? [],
    allowedOAuthScopes: c.AllowedOAuthScopes ?? [],
  };
}

// ---- Domain ----

export async function describeDomain(
  domain: string,
): Promise<CognitoDomain | null> {
  try {
    const data = (await idpRequest("DescribeUserPoolDomain", {
      Domain: domain,
    })) as {
      DomainDescription?: { Domain?: string; Status?: string };
    };
    if (!data.DomainDescription?.Domain) return null;
    return {
      domain: data.DomainDescription.Domain,
      status: data.DomainDescription.Status,
    };
  } catch {
    return null;
  }
}

// ---- Identity Pools ----

export async function listIdentityPools(): Promise<IdentityPool[]> {
  const data = (await identityRequest("ListIdentityPools", {
    MaxResults: 60,
  })) as {
    IdentityPools?: {
      IdentityPoolId: string;
      IdentityPoolName: string;
      AllowUnauthenticatedIdentities?: boolean;
    }[];
  };
  return (data.IdentityPools ?? []).map((p) => ({
    id: p.IdentityPoolId,
    name: p.IdentityPoolName,
    allowUnauthenticated: p.AllowUnauthenticatedIdentities ?? false,
  }));
}

export async function describeIdentityPool(
  id: string,
): Promise<IdentityPoolDetail> {
  const data = (await identityRequest("DescribeIdentityPool", {
    IdentityPoolId: id,
  })) as {
    IdentityPoolId?: string;
    IdentityPoolName?: string;
    AllowUnauthenticatedIdentities?: boolean;
    CognitoIdentityProviders?: { ProviderName: string; ClientId: string }[];
    DeveloperProviderName?: string;
  };
  return {
    id: data.IdentityPoolId ?? id,
    name: data.IdentityPoolName ?? "",
    allowUnauthenticated: data.AllowUnauthenticatedIdentities ?? false,
    cognitoIdentityProviders: (data.CognitoIdentityProviders ?? []).map(
      (p) => ({
        providerName: p.ProviderName,
        clientId: p.ClientId,
      }),
    ),
    developerProviderName: data.DeveloperProviderName,
  };
}
