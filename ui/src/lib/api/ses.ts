/**
 * Typed SES v2 API client.
 *
 * Uses the REST v2 API at `/v2/email/...`. Names map directly to the AWS
 * SDK SES v2 operations (ListEmailIdentities, CreateEmailTemplate, etc.).
 */

import { ENDPOINT, authHeader, amzDate, loggedFetch } from "$lib/aws";

const SERVICE = "ses";

// ---------- Types ----------

export type IdentityType = "EMAIL_ADDRESS" | "DOMAIN" | "MANAGED_DOMAIN";
export type VerificationStatus =
  | "PENDING"
  | "SUCCESS"
  | "FAILED"
  | "TEMPORARY_FAILURE"
  | "NOT_STARTED"
  | "VERIFIED";

export interface Identity {
  name: string;
  type: IdentityType;
  verificationStatus: VerificationStatus;
  sendingEnabled: boolean;
}

export interface ConfigurationSet {
  name: string;
  sendingEnabled?: boolean;
  reputationOptions?: {
    sendingEnabled?: boolean;
    reputationMetricsEnabled?: boolean;
  };
  trackingOptions?: { customRedirectDomain?: string };
}

export interface Template {
  name: string;
  createdTimestamp?: string;
  subject?: string;
  html?: string;
  text?: string;
}

export interface ContactList {
  name: string;
  description?: string;
  createdTimestamp?: string;
  lastUpdatedTimestamp?: string;
}

export interface Contact {
  emailAddress: string;
  topicPreferences?: { topicName: string; subscriptionStatus: string }[];
  unsubscribeAll?: boolean;
  lastUpdatedTimestamp?: string;
}

export type SuppressionReason = "BOUNCE" | "COMPLAINT";

export interface SuppressedDestination {
  emailAddress: string;
  reason: SuppressionReason;
  lastUpdateTime?: string;
}

export interface EmailMessage {
  fromEmailAddress: string;
  toAddresses: string[];
  ccAddresses?: string[];
  bccAddresses?: string[];
  subject: string;
  html?: string;
  text?: string;
  replyToAddresses?: string[];
  configurationSetName?: string;
}

export interface SendEmailResult {
  messageId: string;
}

// ---------- Internal request ----------

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
    throw new Error(`SES ${action} failed (HTTP ${res.status}): ${msg}`);
  }
  return (text ? JSON.parse(text) : {}) as T;
}

// ---------- Identities ----------

interface RawIdentity {
  IdentityName: string;
  IdentityType?: IdentityType;
  VerificationStatus?: VerificationStatus;
  SendingEnabled?: boolean;
}

export async function listIdentities(): Promise<Identity[]> {
  const data = await request<{ EmailIdentities?: RawIdentity[] }>(
    "ListEmailIdentities",
    "GET",
    "/v2/email/identities",
  );
  return (data.EmailIdentities ?? []).map((i) => ({
    name: i.IdentityName,
    type: i.IdentityType ?? "EMAIL_ADDRESS",
    verificationStatus: i.VerificationStatus ?? "VERIFIED",
    sendingEnabled: i.SendingEnabled ?? true,
  }));
}

export async function createIdentity(emailIdentity: string): Promise<void> {
  await request<unknown>("CreateEmailIdentity", "POST", "/v2/email/identities", {
    EmailIdentity: emailIdentity,
  });
}

export async function deleteIdentity(emailIdentity: string): Promise<void> {
  await request<unknown>(
    "DeleteEmailIdentity",
    "DELETE",
    `/v2/email/identities/${encodeURIComponent(emailIdentity)}`,
  );
}

// ---------- Configuration sets ----------

export async function listConfigurationSets(): Promise<ConfigurationSet[]> {
  const data = await request<{ ConfigurationSets?: string[] }>(
    "ListConfigurationSets",
    "GET",
    "/v2/email/configuration-sets",
  );
  return (data.ConfigurationSets ?? []).map((name) => ({ name }));
}

export async function getConfigurationSet(name: string): Promise<ConfigurationSet> {
  const data = await request<{
    ConfigurationSetName: string;
    SendingOptions?: { SendingEnabled?: boolean };
    ReputationOptions?: {
      ReputationMetricsEnabled?: boolean;
      LastFreshStart?: string;
    };
    TrackingOptions?: { CustomRedirectDomain?: string };
  }>(
    "GetConfigurationSet",
    "GET",
    `/v2/email/configuration-sets/${encodeURIComponent(name)}`,
  );
  return {
    name: data.ConfigurationSetName,
    sendingEnabled: data.SendingOptions?.SendingEnabled,
    reputationOptions: {
      sendingEnabled: data.SendingOptions?.SendingEnabled,
      reputationMetricsEnabled: data.ReputationOptions?.ReputationMetricsEnabled,
    },
    trackingOptions: {
      customRedirectDomain: data.TrackingOptions?.CustomRedirectDomain,
    },
  };
}

// ---------- Templates ----------

export async function listTemplates(): Promise<Template[]> {
  const data = await request<{
    TemplatesMetadata?: { TemplateName: string; CreatedTimestamp?: string }[];
  }>("ListEmailTemplates", "GET", "/v2/email/templates");
  return (data.TemplatesMetadata ?? []).map((t) => ({
    name: t.TemplateName,
    createdTimestamp: t.CreatedTimestamp,
  }));
}

export async function getTemplate(name: string): Promise<Template> {
  const data = await request<{
    TemplateName: string;
    TemplateContent?: { Subject?: string; Html?: string; Text?: string };
  }>(
    "GetEmailTemplate",
    "GET",
    `/v2/email/templates/${encodeURIComponent(name)}`,
  );
  return {
    name: data.TemplateName,
    subject: data.TemplateContent?.Subject,
    html: data.TemplateContent?.Html,
    text: data.TemplateContent?.Text,
  };
}

export async function createTemplate(input: {
  name: string;
  subject: string;
  html?: string;
  text?: string;
}): Promise<void> {
  await request<unknown>("CreateEmailTemplate", "POST", "/v2/email/templates", {
    TemplateName: input.name,
    TemplateContent: {
      Subject: input.subject,
      ...(input.html ? { Html: input.html } : {}),
      ...(input.text ? { Text: input.text } : {}),
    },
  });
}

export async function deleteTemplate(name: string): Promise<void> {
  await request<unknown>(
    "DeleteEmailTemplate",
    "DELETE",
    `/v2/email/templates/${encodeURIComponent(name)}`,
  );
}

// ---------- Contact lists ----------

export async function listContactLists(): Promise<ContactList[]> {
  const data = await request<{
    ContactLists?: {
      ContactListName: string;
      LastUpdatedTimestamp?: string;
    }[];
  }>("ListContactLists", "GET", "/v2/email/contact-lists");
  return (data.ContactLists ?? []).map((c) => ({
    name: c.ContactListName,
    lastUpdatedTimestamp: c.LastUpdatedTimestamp,
  }));
}

export async function listContacts(listName: string): Promise<Contact[]> {
  const data = await request<{
    Contacts?: {
      EmailAddress: string;
      UnsubscribeAll?: boolean;
      LastUpdatedTimestamp?: string;
      TopicPreferences?: { TopicName: string; SubscriptionStatus: string }[];
    }[];
  }>(
    "ListContacts",
    "GET",
    `/v2/email/contact-lists/${encodeURIComponent(listName)}/contacts`,
  );
  return (data.Contacts ?? []).map((c) => ({
    emailAddress: c.EmailAddress,
    unsubscribeAll: c.UnsubscribeAll,
    lastUpdatedTimestamp: c.LastUpdatedTimestamp,
    topicPreferences: (c.TopicPreferences ?? []).map((t) => ({
      topicName: t.TopicName,
      subscriptionStatus: t.SubscriptionStatus,
    })),
  }));
}

// ---------- Suppression list ----------

export async function listSuppressedDestinations(): Promise<SuppressedDestination[]> {
  const data = await request<{
    SuppressedDestinationSummaries?: {
      EmailAddress: string;
      Reason: SuppressionReason;
      LastUpdateTime?: string;
    }[];
  }>("ListSuppressedDestinations", "GET", "/v2/email/suppression/addresses");
  return (data.SuppressedDestinationSummaries ?? []).map((s) => ({
    emailAddress: s.EmailAddress,
    reason: s.Reason,
    lastUpdateTime: s.LastUpdateTime,
  }));
}

export async function putSuppressedDestination(
  emailAddress: string,
  reason: SuppressionReason,
): Promise<void> {
  await request<unknown>(
    "PutSuppressedDestination",
    "PUT",
    "/v2/email/suppression/addresses",
    { EmailAddress: emailAddress, Reason: reason },
  );
}

export async function deleteSuppressedDestination(emailAddress: string): Promise<void> {
  await request<unknown>(
    "DeleteSuppressedDestination",
    "DELETE",
    `/v2/email/suppression/addresses/${encodeURIComponent(emailAddress)}`,
  );
}

// ---------- Send ----------

export async function sendEmail(msg: EmailMessage): Promise<SendEmailResult> {
  const body: Record<string, unknown> = {
    FromEmailAddress: msg.fromEmailAddress,
    Destination: {
      ToAddresses: msg.toAddresses,
      ...(msg.ccAddresses?.length ? { CcAddresses: msg.ccAddresses } : {}),
      ...(msg.bccAddresses?.length ? { BccAddresses: msg.bccAddresses } : {}),
    },
    Content: {
      Simple: {
        Subject: { Data: msg.subject, Charset: "UTF-8" },
        Body: {
          ...(msg.html ? { Html: { Data: msg.html, Charset: "UTF-8" } } : {}),
          ...(msg.text ? { Text: { Data: msg.text, Charset: "UTF-8" } } : {}),
        },
      },
    },
  };
  if (msg.replyToAddresses?.length) body["ReplyToAddresses"] = msg.replyToAddresses;
  if (msg.configurationSetName) body["ConfigurationSetName"] = msg.configurationSetName;
  const data = await request<{ MessageId?: string }>(
    "SendEmail",
    "POST",
    "/v2/email/outbound-emails",
    body,
  );
  return { messageId: data.MessageId ?? "" };
}

export async function sendBulkEmail(input: {
  fromEmailAddress: string;
  templateName: string;
  defaultTemplateData?: string;
  destinations: { toAddresses: string[]; replacementTemplateData?: string }[];
  configurationSetName?: string;
}): Promise<{ messageIds: string[] }> {
  const body: Record<string, unknown> = {
    FromEmailAddress: input.fromEmailAddress,
    DefaultContent: {
      Template: {
        TemplateName: input.templateName,
        ...(input.defaultTemplateData
          ? { TemplateData: input.defaultTemplateData }
          : {}),
      },
    },
    BulkEmailEntries: input.destinations.map((d) => ({
      Destination: { ToAddresses: d.toAddresses },
      ...(d.replacementTemplateData
        ? {
            ReplacementEmailContent: {
              ReplacementTemplate: { ReplacementTemplateData: d.replacementTemplateData },
            },
          }
        : {}),
    })),
  };
  if (input.configurationSetName)
    body["ConfigurationSetName"] = input.configurationSetName;
  const data = await request<{
    BulkEmailEntryResults?: { MessageId?: string }[];
  }>("SendBulkEmail", "POST", "/v2/email/outbound-bulk-emails", body);
  return {
    messageIds: (data.BulkEmailEntryResults ?? []).map((r) => r.MessageId ?? ""),
  };
}
