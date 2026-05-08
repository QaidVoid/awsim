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

export interface TagMap {
  [key: string]: string;
}

export async function listTagsForResource(arn: string): Promise<TagMap> {
  const data = (await idpRequest("ListTagsForResource", {
    ResourceArn: arn,
  })) as { Tags?: TagMap };
  return data.Tags ?? {};
}

export async function tagResource(arn: string, tags: TagMap): Promise<void> {
  await idpRequest("TagResource", { ResourceArn: arn, Tags: tags });
}

export async function untagResource(
  arn: string,
  tagKeys: string[],
): Promise<void> {
  await idpRequest("UntagResource", { ResourceArn: arn, TagKeys: tagKeys });
}

export interface PasswordPolicy {
  minimumLength?: number;
  requireUppercase?: boolean;
  requireLowercase?: boolean;
  requireNumbers?: boolean;
  requireSymbols?: boolean;
  temporaryPasswordValidityDays?: number;
}

export interface SchemaAttribute {
  name: string;
  type: string;
  required: boolean;
  mutable: boolean;
  stringConstraints?: { minLength?: number; maxLength?: number };
  numberConstraints?: { minValue?: number; maxValue?: number };
}

export interface UserPoolDetail extends UserPool {
  arn?: string;
  mfaConfiguration?: string;
  estimatedNumberOfUsers?: number;
  lambdaConfig?: Record<string, string>;
  schemaAttributes?: SchemaAttribute[];
  /** Attributes (`email` / `phone_number`) used as the canonical
   * Username. When set, AdminCreateUser / SignUp must pass the
   * matching attribute value as the Username. */
  usernameAttributes?: string[];
  /** Attributes accepted as sign-in aliases. */
  aliasAttributes?: string[];
  autoVerifiedAttributes?: string[];
  domain?: string;
  passwordPolicy?: PasswordPolicy;
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
  logoutURLs: string[];
  allowedOAuthFlows: string[];
  allowedOAuthScopes: string[];
  allowedOAuthFlowsUserPoolClient?: boolean;
  supportedIdentityProviders: string[];
  refreshTokenValidity?: number;
  accessTokenValidity?: number;
  idTokenValidity?: number;
}

export interface CognitoDomain {
  domain: string;
  status?: string;
}

export interface AppClientUpdateInput {
  clientName?: string;
  callbackURLs?: string[];
  logoutURLs?: string[];
  allowedOAuthFlows?: string[];
  allowedOAuthScopes?: string[];
  allowedOAuthFlowsUserPoolClient?: boolean;
  supportedIdentityProviders?: string[];
  explicitAuthFlows?: string[];
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

export interface ListUserPoolsPage {
  pools: UserPool[];
  nextToken?: string;
}

export async function listUserPools(opts?: {
  maxResults?: number;
  nextToken?: string;
}): Promise<ListUserPoolsPage> {
  const body: Record<string, unknown> = {
    MaxResults: opts?.maxResults ?? 60,
  };
  if (opts?.nextToken) body.NextToken = opts.nextToken;
  const data = (await idpRequest("ListUserPools", body)) as {
    UserPools?: {
      Id: string;
      Name: string;
      Status?: string;
      CreationDate?: number;
    }[];
    NextToken?: string;
  };
  return {
    pools: (data.UserPools ?? []).map((p) => ({
      id: p.Id,
      name: p.Name,
      status: p.Status ?? "ACTIVE",
      creationDate: p.CreationDate
        ? new Date(p.CreationDate * 1000).toISOString()
        : "",
    })),
    nextToken: data.NextToken,
  };
}

export async function describeUserPool(id: string): Promise<UserPoolDetail> {
  const data = (await idpRequest("DescribeUserPool", { UserPoolId: id })) as {
    UserPool?: {
      Id: string;
      Name: string;
      Status?: string;
      CreationDate?: number;
      Arn?: string;
      MfaConfiguration?: string;
      EstimatedNumberOfUsers?: number;
      LambdaConfig?: Record<string, string>;
      SchemaAttributes?: {
        Name: string;
        AttributeDataType: string;
        Required: boolean;
        Mutable?: boolean;
        StringAttributeConstraints?: {
          MinLength?: string;
          MaxLength?: string;
        };
        NumberAttributeConstraints?: {
          MinValue?: string;
          MaxValue?: string;
        };
      }[];
      UsernameAttributes?: string[];
      AliasAttributes?: string[];
      AutoVerifiedAttributes?: string[];
      Domain?: string;
      Policies?: {
        PasswordPolicy?: {
          MinimumLength?: number;
          RequireUppercase?: boolean;
          RequireLowercase?: boolean;
          RequireNumbers?: boolean;
          RequireSymbols?: boolean;
          TemporaryPasswordValidityDays?: number;
        };
      };
    };
  };
  const p = data.UserPool ?? ({} as NonNullable<typeof data.UserPool>);
  const pp = p.Policies?.PasswordPolicy;
  const parseInt10 = (s: string | undefined) =>
    s !== undefined && s !== "" ? Number.parseInt(s, 10) : undefined;
  return {
    id: p.Id ?? id,
    name: p.Name ?? "",
    status: p.Status ?? "",
    creationDate: p.CreationDate
      ? new Date(p.CreationDate * 1000).toISOString()
      : "",
    arn: p.Arn,
    mfaConfiguration: p.MfaConfiguration,
    estimatedNumberOfUsers: p.EstimatedNumberOfUsers,
    lambdaConfig: p.LambdaConfig,
    schemaAttributes: (p.SchemaAttributes ?? []).map((a) => ({
      name: a.Name,
      type: a.AttributeDataType,
      required: a.Required,
      mutable: a.Mutable ?? true,
      stringConstraints: a.StringAttributeConstraints
        ? {
            minLength: parseInt10(a.StringAttributeConstraints.MinLength),
            maxLength: parseInt10(a.StringAttributeConstraints.MaxLength),
          }
        : undefined,
      numberConstraints: a.NumberAttributeConstraints
        ? {
            minValue: parseInt10(a.NumberAttributeConstraints.MinValue),
            maxValue: parseInt10(a.NumberAttributeConstraints.MaxValue),
          }
        : undefined,
    })),
    usernameAttributes: p.UsernameAttributes ?? [],
    aliasAttributes: p.AliasAttributes ?? [],
    autoVerifiedAttributes: p.AutoVerifiedAttributes ?? [],
    domain: p.Domain,
    passwordPolicy: pp
      ? {
          minimumLength: pp.MinimumLength,
          requireUppercase: pp.RequireUppercase,
          requireLowercase: pp.RequireLowercase,
          requireNumbers: pp.RequireNumbers,
          requireSymbols: pp.RequireSymbols,
          temporaryPasswordValidityDays: pp.TemporaryPasswordValidityDays,
        }
      : undefined,
  };
}

export interface UpdateUserPoolInput {
  lambdaConfig?: Record<string, string>;
  mfaConfiguration?: "OFF" | "ON" | "OPTIONAL";
  autoVerifiedAttributes?: string[];
  passwordPolicy?: {
    minimumLength?: number;
    requireUppercase?: boolean;
    requireLowercase?: boolean;
    requireNumbers?: boolean;
    requireSymbols?: boolean;
    temporaryPasswordValidityDays?: number;
  };
}

export interface MfaConfig {
  mfaConfiguration: "OFF" | "ON" | "OPTIONAL";
  softwareTokenEnabled: boolean;
}

export async function getUserPoolMfaConfig(poolId: string): Promise<MfaConfig> {
  const data = (await idpRequest("GetUserPoolMfaConfig", {
    UserPoolId: poolId,
  })) as {
    MfaConfiguration?: string;
    SoftwareTokenMfaConfiguration?: { Enabled?: boolean };
  };
  const mfa = (data.MfaConfiguration ?? "OFF") as MfaConfig["mfaConfiguration"];
  return {
    mfaConfiguration: mfa,
    softwareTokenEnabled: data.SoftwareTokenMfaConfiguration?.Enabled ?? false,
  };
}

export async function setUserPoolMfaConfig(
  poolId: string,
  cfg: MfaConfig,
): Promise<void> {
  await idpRequest("SetUserPoolMfaConfig", {
    UserPoolId: poolId,
    MfaConfiguration: cfg.mfaConfiguration,
    SoftwareTokenMfaConfiguration: { Enabled: cfg.softwareTokenEnabled },
  });
}

export async function updateUserPool(
  poolId: string,
  patch: UpdateUserPoolInput,
): Promise<void> {
  const body: Record<string, unknown> = { UserPoolId: poolId };
  if (patch.lambdaConfig) body.LambdaConfig = patch.lambdaConfig;
  if (patch.mfaConfiguration) body.MfaConfiguration = patch.mfaConfiguration;
  if (patch.autoVerifiedAttributes)
    body.AutoVerifiedAttributes = patch.autoVerifiedAttributes;
  if (patch.passwordPolicy) {
    const p = patch.passwordPolicy;
    const policy: Record<string, unknown> = {};
    if (p.minimumLength !== undefined) policy.MinimumLength = p.minimumLength;
    if (p.requireUppercase !== undefined)
      policy.RequireUppercase = p.requireUppercase;
    if (p.requireLowercase !== undefined)
      policy.RequireLowercase = p.requireLowercase;
    if (p.requireNumbers !== undefined)
      policy.RequireNumbers = p.requireNumbers;
    if (p.requireSymbols !== undefined)
      policy.RequireSymbols = p.requireSymbols;
    if (p.temporaryPasswordValidityDays !== undefined)
      policy.TemporaryPasswordValidityDays = p.temporaryPasswordValidityDays;
    body.Policies = { PasswordPolicy: policy };
  }
  await idpRequest("UpdateUserPool", body);
}

// ---- Users in pool ----

export interface ListUsersOptions {
  limit?: number;
  paginationToken?: string;
  /** Cognito filter expression, e.g. `username ^= "alice"` */
  filter?: string;
}

export interface ListUsersPage {
  users: CognitoUserSummary[];
  /** Present when more pages exist; pass back as `paginationToken`. */
  nextToken?: string;
}

export async function listPoolUsers(
  poolId: string,
  opts: ListUsersOptions = {},
): Promise<ListUsersPage> {
  const body: Record<string, unknown> = { UserPoolId: poolId };
  if (opts.limit !== undefined) body.Limit = opts.limit;
  if (opts.paginationToken) body.PaginationToken = opts.paginationToken;
  if (opts.filter) body.Filter = opts.filter;
  const data = (await idpRequest("ListUsers", body)) as {
    Users?: {
      Username: string;
      UserStatus?: string;
      Enabled?: boolean;
      UserCreateDate?: number;
    }[];
    PaginationToken?: string;
  };
  return {
    users: (data.Users ?? []).map((u) => ({
      username: u.Username,
      status: u.UserStatus ?? "",
      enabled: u.Enabled ?? true,
      createDate: u.UserCreateDate
        ? new Date(u.UserCreateDate * 1000).toISOString()
        : "",
    })),
    nextToken: data.PaginationToken,
  };
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

export async function adminCreateUser(input: {
  poolId: string;
  username: string;
  temporaryPassword?: string;
  attributes?: { name: string; value: string }[];
  messageAction?: "SUPPRESS" | "RESEND";
}): Promise<void> {
  const body: Record<string, unknown> = {
    UserPoolId: input.poolId,
    Username: input.username,
  };
  if (input.temporaryPassword) body.TemporaryPassword = input.temporaryPassword;
  if (input.messageAction) body.MessageAction = input.messageAction;
  if (input.attributes && input.attributes.length > 0) {
    body.UserAttributes = input.attributes.map((a) => ({
      Name: a.name,
      Value: a.value,
    }));
  }
  await idpRequest("AdminCreateUser", body);
}

export async function adminDeleteUser(
  poolId: string,
  username: string,
): Promise<void> {
  await idpRequest("AdminDeleteUser", {
    UserPoolId: poolId,
    Username: username,
  });
}

export async function adminSetUserPassword(input: {
  poolId: string;
  username: string;
  password: string;
  permanent: boolean;
}): Promise<void> {
  await idpRequest("AdminSetUserPassword", {
    UserPoolId: input.poolId,
    Username: input.username,
    Password: input.password,
    Permanent: input.permanent,
  });
}

/// Declare new custom attributes on a user pool. The `name` is sent
/// without the `custom:` prefix (Cognito always prepends it). Real
/// AWS rejects duplicate names, type changes, or going past the
/// 50-attr per-pool cap.
export async function addCustomAttributes(
  poolId: string,
  attrs: {
    name: string;
    type: 'String' | 'Number' | 'DateTime' | 'Boolean';
    mutable: boolean;
    required?: boolean;
    stringConstraints?: { minLength?: number; maxLength?: number };
    numberConstraints?: { minValue?: number; maxValue?: number };
  }[]
): Promise<void> {
  const customAttributes = attrs.map((a) => {
    const entry: Record<string, unknown> = {
      Name: a.name,
      AttributeDataType: a.type,
      Mutable: a.mutable,
      Required: a.required ?? false,
    };
    if (a.type === 'String' && a.stringConstraints) {
      const sc: Record<string, string> = {};
      if (a.stringConstraints.minLength !== undefined)
        sc.MinLength = String(a.stringConstraints.minLength);
      if (a.stringConstraints.maxLength !== undefined)
        sc.MaxLength = String(a.stringConstraints.maxLength);
      if (Object.keys(sc).length > 0) entry.StringAttributeConstraints = sc;
    }
    if (a.type === 'Number' && a.numberConstraints) {
      const nc: Record<string, string> = {};
      if (a.numberConstraints.minValue !== undefined)
        nc.MinValue = String(a.numberConstraints.minValue);
      if (a.numberConstraints.maxValue !== undefined)
        nc.MaxValue = String(a.numberConstraints.maxValue);
      if (Object.keys(nc).length > 0) entry.NumberAttributeConstraints = nc;
    }
    return entry;
  });
  await idpRequest('AddCustomAttributes', {
    UserPoolId: poolId,
    CustomAttributes: customAttributes,
  });
}

export async function adminUpdateUserAttributes(input: {
  poolId: string;
  username: string;
  attributes: { name: string; value: string }[];
}): Promise<void> {
  await idpRequest("AdminUpdateUserAttributes", {
    UserPoolId: input.poolId,
    Username: input.username,
    UserAttributes: input.attributes.map((a) => ({
      Name: a.name,
      Value: a.value,
    })),
  });
}

export interface AuthEvent {
  eventId: string;
  eventType: string;
  creationDate: string;
  eventResponse: string;
  riskDecision?: string;
  riskLevel?: string;
  compromised?: boolean;
  ipAddress?: string;
  deviceName?: string;
  city?: string;
  country?: string;
}

export async function adminListUserAuthEvents(
  poolId: string,
  username: string,
  opts?: { maxResults?: number; nextToken?: string },
): Promise<{ events: AuthEvent[]; nextToken?: string }> {
  const body: Record<string, unknown> = {
    UserPoolId: poolId,
    Username: username,
    MaxResults: opts?.maxResults ?? 60,
  };
  if (opts?.nextToken) body.NextToken = opts.nextToken;
  const data = (await idpRequest("AdminListUserAuthEvents", body)) as {
    AuthEvents?: {
      EventId: string;
      EventType?: string;
      CreationDate?: number;
      EventResponse?: string;
      EventRisk?: {
        RiskDecision?: string;
        RiskLevel?: string;
        CompromisedCredentialsDetected?: boolean;
      };
      EventContextData?: {
        IpAddress?: string;
        DeviceName?: string;
        City?: string;
        Country?: string;
      };
    }[];
    NextToken?: string;
  };
  return {
    events: (data.AuthEvents ?? []).map((e) => ({
      eventId: e.EventId,
      eventType: e.EventType ?? "",
      creationDate: e.CreationDate
        ? new Date(e.CreationDate * 1000).toISOString()
        : "",
      eventResponse: e.EventResponse ?? "",
      riskDecision: e.EventRisk?.RiskDecision,
      riskLevel: e.EventRisk?.RiskLevel,
      compromised: e.EventRisk?.CompromisedCredentialsDetected,
      ipAddress: e.EventContextData?.IpAddress,
      deviceName: e.EventContextData?.DeviceName,
      city: e.EventContextData?.City,
      country: e.EventContextData?.Country,
    })),
    nextToken: data.NextToken,
  };
}

export async function adminListGroupsForUser(
  poolId: string,
  username: string,
): Promise<CognitoGroup[]> {
  const data = (await idpRequest("AdminListGroupsForUser", {
    UserPoolId: poolId,
    Username: username,
  })) as {
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

export async function adminAddUserToGroup(
  poolId: string,
  username: string,
  groupName: string,
): Promise<void> {
  await idpRequest("AdminAddUserToGroup", {
    UserPoolId: poolId,
    Username: username,
    GroupName: groupName,
  });
}

export async function adminRemoveUserFromGroup(
  poolId: string,
  username: string,
  groupName: string,
): Promise<void> {
  await idpRequest("AdminRemoveUserFromGroup", {
    UserPoolId: poolId,
    Username: username,
    GroupName: groupName,
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

export async function createGroup(input: {
  poolId: string;
  name: string;
  description?: string;
  roleArn?: string;
  precedence?: number;
}): Promise<void> {
  const body: Record<string, unknown> = {
    UserPoolId: input.poolId,
    GroupName: input.name,
  };
  if (input.description) body.Description = input.description;
  if (input.roleArn) body.RoleArn = input.roleArn;
  if (input.precedence !== undefined) body.Precedence = input.precedence;
  await idpRequest("CreateGroup", body);
}

export async function updateGroup(input: {
  poolId: string;
  name: string;
  description?: string;
  roleArn?: string;
  precedence?: number;
}): Promise<void> {
  const body: Record<string, unknown> = {
    UserPoolId: input.poolId,
    GroupName: input.name,
  };
  if (input.description !== undefined) body.Description = input.description;
  if (input.roleArn !== undefined) body.RoleArn = input.roleArn;
  if (input.precedence !== undefined) body.Precedence = input.precedence;
  await idpRequest("UpdateGroup", body);
}

export async function deleteGroup(
  poolId: string,
  groupName: string,
): Promise<void> {
  await idpRequest("DeleteGroup", {
    UserPoolId: poolId,
    GroupName: groupName,
  });
}

export async function listUsersInGroup(
  poolId: string,
  groupName: string,
): Promise<CognitoUserSummary[]> {
  const data = (await idpRequest("ListUsersInGroup", {
    UserPoolId: poolId,
    GroupName: groupName,
  })) as {
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

// ---- App clients ----

export interface ListAppClientsPage {
  clients: CognitoAppClient[];
  nextToken?: string;
}

export async function listAppClients(
  poolId: string,
  opts?: { maxResults?: number; nextToken?: string },
): Promise<ListAppClientsPage> {
  const body: Record<string, unknown> = {
    UserPoolId: poolId,
    MaxResults: opts?.maxResults ?? 60,
  };
  if (opts?.nextToken) body.NextToken = opts.nextToken;
  const data = (await idpRequest("ListUserPoolClients", body)) as {
    UserPoolClients?: {
      ClientId: string;
      ClientName: string;
      UserPoolId: string;
    }[];
    NextToken?: string;
  };
  return {
    clients: (data.UserPoolClients ?? []).map((c) => ({
      clientId: c.ClientId,
      clientName: c.ClientName,
    })),
    nextToken: data.NextToken,
  };
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
      LogoutURLs?: string[];
      AllowedOAuthFlows?: string[];
      AllowedOAuthScopes?: string[];
      AllowedOAuthFlowsUserPoolClient?: boolean;
      SupportedIdentityProviders?: string[];
      RefreshTokenValidity?: number;
      AccessTokenValidity?: number;
      IdTokenValidity?: number;
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
    logoutURLs: c.LogoutURLs ?? [],
    allowedOAuthFlows: c.AllowedOAuthFlows ?? [],
    allowedOAuthScopes: c.AllowedOAuthScopes ?? [],
    allowedOAuthFlowsUserPoolClient: c.AllowedOAuthFlowsUserPoolClient,
    supportedIdentityProviders: c.SupportedIdentityProviders ?? [],
    refreshTokenValidity: c.RefreshTokenValidity,
    accessTokenValidity: c.AccessTokenValidity,
    idTokenValidity: c.IdTokenValidity,
  };
}

export async function createAppClient(input: {
  poolId: string;
  clientName: string;
  generateSecret?: boolean;
  explicitAuthFlows?: string[];
  callbackURLs?: string[];
  logoutURLs?: string[];
  allowedOAuthFlows?: string[];
  allowedOAuthScopes?: string[];
  allowedOAuthFlowsUserPoolClient?: boolean;
  supportedIdentityProviders?: string[];
}): Promise<CognitoAppClientDetail> {
  const body: Record<string, unknown> = {
    UserPoolId: input.poolId,
    ClientName: input.clientName,
  };
  if (input.generateSecret) body.GenerateSecret = true;
  if (input.explicitAuthFlows?.length)
    body.ExplicitAuthFlows = input.explicitAuthFlows;
  if (input.callbackURLs?.length) body.CallbackURLs = input.callbackURLs;
  if (input.logoutURLs?.length) body.LogoutURLs = input.logoutURLs;
  if (input.allowedOAuthFlows?.length)
    body.AllowedOAuthFlows = input.allowedOAuthFlows;
  if (input.allowedOAuthScopes?.length)
    body.AllowedOAuthScopes = input.allowedOAuthScopes;
  if (input.allowedOAuthFlowsUserPoolClient !== undefined)
    body.AllowedOAuthFlowsUserPoolClient = input.allowedOAuthFlowsUserPoolClient;
  if (input.supportedIdentityProviders?.length)
    body.SupportedIdentityProviders = input.supportedIdentityProviders;
  const data = (await idpRequest("CreateUserPoolClient", body)) as {
    UserPoolClient?: {
      ClientId?: string;
      ClientName?: string;
      ClientSecret?: string;
    };
  };
  const c = data.UserPoolClient ?? {};
  return {
    clientId: c.ClientId ?? "",
    clientName: c.ClientName ?? input.clientName,
    clientSecret: c.ClientSecret,
    explicitAuthFlows: input.explicitAuthFlows ?? [],
    callbackURLs: input.callbackURLs ?? [],
    logoutURLs: input.logoutURLs ?? [],
    allowedOAuthFlows: input.allowedOAuthFlows ?? [],
    allowedOAuthScopes: input.allowedOAuthScopes ?? [],
    allowedOAuthFlowsUserPoolClient: input.allowedOAuthFlowsUserPoolClient,
    supportedIdentityProviders: input.supportedIdentityProviders ?? [],
  };
}

export async function updateAppClient(input: {
  poolId: string;
  clientId: string;
  patch: AppClientUpdateInput;
}): Promise<void> {
  const body: Record<string, unknown> = {
    UserPoolId: input.poolId,
    ClientId: input.clientId,
  };
  const p = input.patch;
  if (p.clientName !== undefined) body.ClientName = p.clientName;
  if (p.callbackURLs !== undefined) body.CallbackURLs = p.callbackURLs;
  if (p.logoutURLs !== undefined) body.LogoutURLs = p.logoutURLs;
  if (p.allowedOAuthFlows !== undefined)
    body.AllowedOAuthFlows = p.allowedOAuthFlows;
  if (p.allowedOAuthScopes !== undefined)
    body.AllowedOAuthScopes = p.allowedOAuthScopes;
  if (p.allowedOAuthFlowsUserPoolClient !== undefined)
    body.AllowedOAuthFlowsUserPoolClient = p.allowedOAuthFlowsUserPoolClient;
  if (p.supportedIdentityProviders !== undefined)
    body.SupportedIdentityProviders = p.supportedIdentityProviders;
  if (p.explicitAuthFlows !== undefined)
    body.ExplicitAuthFlows = p.explicitAuthFlows;
  await idpRequest("UpdateUserPoolClient", body);
}

export async function deleteAppClient(
  poolId: string,
  clientId: string,
): Promise<void> {
  await idpRequest("DeleteUserPoolClient", {
    UserPoolId: poolId,
    ClientId: clientId,
  });
}

// ---- UI customization (hosted UI branding) ----

export interface UiCustomization {
  /** `null` when scope is the pool default ("ALL" clients). */
  clientId: string | null;
  css: string | null;
  imageUrl: string | null;
  creationDate?: string;
  lastModifiedDate?: string;
}

export async function getUiCustomization(
  poolId: string,
  clientId?: string,
): Promise<UiCustomization> {
  const body: Record<string, unknown> = { UserPoolId: poolId };
  if (clientId) body.ClientId = clientId;
  const data = (await idpRequest("GetUICustomization", body)) as {
    UICustomization?: {
      ClientId?: string | null;
      CSS?: string | null;
      ImageUrl?: string | null;
      CreationDate?: number;
      LastModifiedDate?: number;
    };
  };
  const u = data.UICustomization ?? {};
  return {
    clientId: u.ClientId ?? null,
    css: u.CSS ?? null,
    imageUrl: u.ImageUrl ?? null,
    creationDate: u.CreationDate
      ? new Date(u.CreationDate * 1000).toISOString()
      : undefined,
    lastModifiedDate: u.LastModifiedDate
      ? new Date(u.LastModifiedDate * 1000).toISOString()
      : undefined,
  };
}

export async function setUiCustomization(input: {
  poolId: string;
  clientId?: string;
  css?: string;
  imageUrl?: string;
}): Promise<void> {
  const body: Record<string, unknown> = { UserPoolId: input.poolId };
  if (input.clientId) body.ClientId = input.clientId;
  if (input.css !== undefined) body.CSS = input.css;
  if (input.imageUrl !== undefined) body.ImageFile = input.imageUrl;
  await idpRequest("SetUICustomization", body);
}

// ---- Identity providers ----

export type IdpType =
  | "SAML"
  | "OIDC"
  | "Google"
  | "Facebook"
  | "SignInWithApple"
  | "LoginWithAmazon";

export interface IdentityProvider {
  name: string;
  type: IdpType;
}

export interface IdentityProviderDetail extends IdentityProvider {
  providerDetails: Record<string, string>;
  attributeMapping: Record<string, string>;
  idpIdentifiers: string[];
}

export async function listIdentityProviders(
  poolId: string,
  opts?: { maxResults?: number; nextToken?: string },
): Promise<{ providers: IdentityProvider[]; nextToken?: string }> {
  const body: Record<string, unknown> = {
    UserPoolId: poolId,
    MaxResults: opts?.maxResults ?? 60,
  };
  if (opts?.nextToken) body.NextToken = opts.nextToken;
  const data = (await idpRequest("ListIdentityProviders", body)) as {
    Providers?: { ProviderName: string; ProviderType: string }[];
    NextToken?: string;
  };
  return {
    providers: (data.Providers ?? []).map((p) => ({
      name: p.ProviderName,
      type: p.ProviderType as IdpType,
    })),
    nextToken: data.NextToken,
  };
}

export async function describeIdentityProvider(
  poolId: string,
  name: string,
): Promise<IdentityProviderDetail> {
  const data = (await idpRequest("DescribeIdentityProvider", {
    UserPoolId: poolId,
    ProviderName: name,
  })) as {
    IdentityProvider?: {
      ProviderName: string;
      ProviderType: string;
      ProviderDetails?: Record<string, string>;
      AttributeMapping?: Record<string, string>;
      IdpIdentifiers?: string[];
    };
  };
  const i = data.IdentityProvider;
  return {
    name: i?.ProviderName ?? name,
    type: (i?.ProviderType ?? "OIDC") as IdpType,
    providerDetails: i?.ProviderDetails ?? {},
    attributeMapping: i?.AttributeMapping ?? {},
    idpIdentifiers: i?.IdpIdentifiers ?? [],
  };
}

export async function createIdentityProvider(input: {
  poolId: string;
  name: string;
  type: IdpType;
  providerDetails: Record<string, string>;
  attributeMapping?: Record<string, string>;
  idpIdentifiers?: string[];
}): Promise<void> {
  const body: Record<string, unknown> = {
    UserPoolId: input.poolId,
    ProviderName: input.name,
    ProviderType: input.type,
    ProviderDetails: input.providerDetails,
  };
  if (input.attributeMapping && Object.keys(input.attributeMapping).length > 0)
    body.AttributeMapping = input.attributeMapping;
  if (input.idpIdentifiers?.length) body.IdpIdentifiers = input.idpIdentifiers;
  await idpRequest("CreateIdentityProvider", body);
}

export async function updateIdentityProvider(input: {
  poolId: string;
  name: string;
  providerDetails?: Record<string, string>;
  attributeMapping?: Record<string, string>;
  idpIdentifiers?: string[];
}): Promise<void> {
  const body: Record<string, unknown> = {
    UserPoolId: input.poolId,
    ProviderName: input.name,
  };
  if (input.providerDetails) body.ProviderDetails = input.providerDetails;
  if (input.attributeMapping) body.AttributeMapping = input.attributeMapping;
  if (input.idpIdentifiers !== undefined)
    body.IdpIdentifiers = input.idpIdentifiers;
  await idpRequest("UpdateIdentityProvider", body);
}

export async function deleteIdentityProvider(
  poolId: string,
  name: string,
): Promise<void> {
  await idpRequest("DeleteIdentityProvider", {
    UserPoolId: poolId,
    ProviderName: name,
  });
}

// ---- Resource servers ----

export interface ResourceScope {
  name: string;
  description: string;
}

export interface ResourceServer {
  identifier: string;
  name: string;
  scopes: ResourceScope[];
}

export async function listResourceServers(
  poolId: string,
  opts?: { maxResults?: number; nextToken?: string },
): Promise<{ servers: ResourceServer[]; nextToken?: string }> {
  const body: Record<string, unknown> = {
    UserPoolId: poolId,
    MaxResults: opts?.maxResults ?? 60,
  };
  if (opts?.nextToken) body.NextToken = opts.nextToken;
  const data = (await idpRequest("ListResourceServers", body)) as {
    ResourceServers?: {
      Identifier: string;
      Name: string;
      Scopes?: { ScopeName: string; ScopeDescription: string }[];
    }[];
    NextToken?: string;
  };
  return {
    servers: (data.ResourceServers ?? []).map((s) => ({
      identifier: s.Identifier,
      name: s.Name,
      scopes: (s.Scopes ?? []).map((sc) => ({
        name: sc.ScopeName,
        description: sc.ScopeDescription,
      })),
    })),
    nextToken: data.NextToken,
  };
}

export async function createResourceServer(input: {
  poolId: string;
  identifier: string;
  name: string;
  scopes: ResourceScope[];
}): Promise<void> {
  await idpRequest("CreateResourceServer", {
    UserPoolId: input.poolId,
    Identifier: input.identifier,
    Name: input.name,
    Scopes: input.scopes.map((s) => ({
      ScopeName: s.name,
      ScopeDescription: s.description,
    })),
  });
}

export async function updateResourceServer(input: {
  poolId: string;
  identifier: string;
  name: string;
  scopes: ResourceScope[];
}): Promise<void> {
  await idpRequest("UpdateResourceServer", {
    UserPoolId: input.poolId,
    Identifier: input.identifier,
    Name: input.name,
    Scopes: input.scopes.map((s) => ({
      ScopeName: s.name,
      ScopeDescription: s.description,
    })),
  });
}

export async function deleteResourceServer(
  poolId: string,
  identifier: string,
): Promise<void> {
  await idpRequest("DeleteResourceServer", {
    UserPoolId: poolId,
    Identifier: identifier,
  });
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

export async function createDomain(
  poolId: string,
  domain: string,
): Promise<void> {
  await idpRequest("CreateUserPoolDomain", {
    UserPoolId: poolId,
    Domain: domain,
  });
}

export async function deleteDomain(
  poolId: string,
  domain: string,
): Promise<void> {
  await idpRequest("DeleteUserPoolDomain", {
    UserPoolId: poolId,
    Domain: domain,
  });
}

// ---- Pool create/delete ----

export async function createUserPool(input: {
  name: string;
  autoVerifyEmail?: boolean;
  passwordMinLength?: number;
}): Promise<UserPool> {
  const body: Record<string, unknown> = { PoolName: input.name };
  if (input.autoVerifyEmail) body.AutoVerifiedAttributes = ["email"];
  if (input.passwordMinLength !== undefined) {
    body.Policies = {
      PasswordPolicy: {
        MinimumLength: input.passwordMinLength,
        RequireUppercase: false,
        RequireLowercase: false,
        RequireNumbers: false,
        RequireSymbols: false,
      },
    };
  }
  const data = (await idpRequest("CreateUserPool", body)) as {
    UserPool?: {
      Id?: string;
      Name?: string;
      Status?: string;
      CreationDate?: number;
    };
  };
  const p = data.UserPool ?? {};
  return {
    id: p.Id ?? "",
    name: p.Name ?? input.name,
    status: p.Status ?? "ACTIVE",
    creationDate: p.CreationDate
      ? new Date(p.CreationDate * 1000).toISOString()
      : "",
  };
}

export async function deleteUserPool(poolId: string): Promise<void> {
  await idpRequest("DeleteUserPool", { UserPoolId: poolId });
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

export interface IdentityPoolIdentity {
  identityId: string;
  logins: Record<string, string>;
  creationDate: string;
  lastModifiedDate: string;
}

export async function listIdentities(
  poolId: string,
  maxResults = 60,
): Promise<IdentityPoolIdentity[]> {
  const data = (await identityRequest("ListIdentities", {
    IdentityPoolId: poolId,
    MaxResults: maxResults,
  })) as {
    Identities?: {
      IdentityId?: string;
      Logins?: Record<string, string>;
      CreationDate?: string;
      LastModifiedDate?: string;
    }[];
  };
  return (data.Identities ?? []).map((i) => ({
    identityId: i.IdentityId ?? "",
    logins: i.Logins ?? {},
    creationDate: i.CreationDate ?? "",
    lastModifiedDate: i.LastModifiedDate ?? "",
  }));
}

export interface IdentityPoolRoles {
  authenticatedRoleArn?: string;
  unauthenticatedRoleArn?: string;
  roleMappingRules?: {
    type: string;
    ambiguousRoleResolution: string;
    rules: { claim: string; matchType: string; value: string; roleArn: string }[];
  }[];
}

export async function getIdentityPoolRoles(
  poolId: string,
): Promise<IdentityPoolRoles> {
  const data = (await identityRequest("GetIdentityPoolRoles", {
    IdentityPoolId: poolId,
  })) as {
    Roles?: {
      AuthenticatedRoleArn?: string;
      UnauthenticatedRoleArn?: string;
    };
    RoleMappings?: Record<
      string,
      {
        Type?: string;
        AmbiguousRoleResolution?: string;
        RulesConfiguration?: {
          Rules?: {
            Claim?: string;
            MatchType?: string;
            Value?: string;
            RoleARN?: string;
          }[];
        };
      }
    >;
  };
  const roleMappings = data.RoleMappings ?? {};
  const roleMappingRules = Object.entries(roleMappings).map(
    ([, v]) => ({
      type: v.Type ?? "",
      ambiguousRoleResolution: v.AmbiguousRoleResolution ?? "",
      rules: (v.RulesConfiguration?.Rules ?? []).map((r) => ({
        claim: r.Claim ?? "",
        matchType: r.MatchType ?? "",
        value: r.Value ?? "",
        roleArn: r.RoleARN ?? "",
      })),
    }),
  );
  return {
    authenticatedRoleArn: data.Roles?.AuthenticatedRoleArn,
    unauthenticatedRoleArn: data.Roles?.UnauthenticatedRoleArn,
    roleMappingRules: roleMappingRules.length ? roleMappingRules : undefined,
  };
}

export async function deleteIdentityPool(poolId: string): Promise<void> {
  await identityRequest("DeleteIdentityPool", { IdentityPoolId: poolId });
}
