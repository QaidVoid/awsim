/**
 * Typed ACM (Certificate Manager) API client.
 */

const ENDPOINT = "http://localhost:4566";
const FAKE_DATE = new Date().toISOString().slice(0, 10).replace(/-/g, "");

function authHeader(): string {
  return `AWS4-HMAC-SHA256 Credential=test/${FAKE_DATE}/us-east-1/acm/aws4_request, SignedHeaders=host;x-amz-date, Signature=fakesignature`;
}

function amzDate(): string {
  return new Date().toISOString().replace(/[:-]/g, "").slice(0, 15) + "Z";
}

async function acmRequest(
  action: string,
  body: unknown = {},
): Promise<unknown> {
  const res = await fetch(ENDPOINT, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-amz-json-1.1",
      "X-Amz-Target": `CertificateManager.${action}`,
      Authorization: authHeader(),
      "X-Amz-Date": amzDate(),
    },
    body: JSON.stringify(body),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`ACM ${action} failed: ${res.status} ${text}`);
  return text ? JSON.parse(text) : {};
}

// ---- Types ----

export type CertificateStatus =
  | "PENDING_VALIDATION"
  | "ISSUED"
  | "INACTIVE"
  | "EXPIRED"
  | "VALIDATION_TIMED_OUT"
  | "REVOKED"
  | "FAILED";

export interface Certificate {
  arn: string;
  domainName: string;
  status: CertificateStatus | string;
}

export interface CertificateDetail extends Certificate {
  subjectAlternativeNames?: string[];
  issuer?: string;
  type?: string;
  keyAlgorithm?: string;
  signatureAlgorithm?: string;
  notBefore?: string;
  notAfter?: string;
  inUseBy?: string[];
  createdAt?: string;
}

// ---- Operations ----

export async function listCertificates(): Promise<Certificate[]> {
  const data = (await acmRequest("ListCertificates")) as {
    CertificateSummaryList?: {
      CertificateArn: string;
      DomainName: string;
      Status?: string;
    }[];
  };
  return (data.CertificateSummaryList ?? []).map((c) => ({
    arn: c.CertificateArn,
    domainName: c.DomainName,
    status: (c.Status as CertificateStatus | undefined) ?? "PENDING_VALIDATION",
  }));
}

export async function describeCertificate(
  arn: string,
): Promise<CertificateDetail> {
  const data = (await acmRequest("DescribeCertificate", {
    CertificateArn: arn,
  })) as {
    Certificate?: {
      CertificateArn: string;
      DomainName: string;
      Status?: string;
      SubjectAlternativeNames?: string[];
      Issuer?: string;
      Type?: string;
      KeyAlgorithm?: string;
      SignatureAlgorithm?: string;
      NotBefore?: number;
      NotAfter?: number;
      CreatedAt?: number;
      InUseBy?: string[];
    };
  };
  const c = data.Certificate ?? ({} as NonNullable<typeof data.Certificate>);
  return {
    arn: c.CertificateArn ?? arn,
    domainName: c.DomainName ?? "",
    status: c.Status ?? "PENDING_VALIDATION",
    subjectAlternativeNames: c.SubjectAlternativeNames,
    issuer: c.Issuer,
    type: c.Type,
    keyAlgorithm: c.KeyAlgorithm,
    signatureAlgorithm: c.SignatureAlgorithm,
    notBefore: c.NotBefore
      ? new Date(c.NotBefore * 1000).toISOString()
      : undefined,
    notAfter: c.NotAfter
      ? new Date(c.NotAfter * 1000).toISOString()
      : undefined,
    createdAt: c.CreatedAt
      ? new Date(c.CreatedAt * 1000).toISOString()
      : undefined,
    inUseBy: c.InUseBy,
  };
}

export async function requestCertificate(
  domainName: string,
  options?: { sans?: string[]; validationMethod?: "DNS" | "EMAIL" },
): Promise<{ arn: string }> {
  const body: Record<string, unknown> = {
    DomainName: domainName,
    ValidationMethod: options?.validationMethod ?? "DNS",
  };
  if (options?.sans?.length) body.SubjectAlternativeNames = options.sans;
  const data = (await acmRequest("RequestCertificate", body)) as {
    CertificateArn?: string;
  };
  return { arn: data.CertificateArn ?? "" };
}

export async function deleteCertificate(arn: string): Promise<void> {
  await acmRequest("DeleteCertificate", { CertificateArn: arn });
}
