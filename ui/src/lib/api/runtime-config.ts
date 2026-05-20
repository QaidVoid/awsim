/**
 * Runtime config admin API.
 *
 * The runtime config is the live, hot-reloadable subset of awsim's
 * settings — Bedrock proxy backends, SES retention, etc. Persisted
 * to `<data_dir>/runtime-config.json` when --data-dir is set;
 * in-memory only otherwise. The UI Settings page is the primary
 * editor.
 */

export interface BedrockBackendSpec {
  endpoint: string;
  /**
   * Catalog key the backend was wired from (e.g. "ollama", "openai",
   * "groq", "custom"). Pure metadata; the runtime never branches on
   * it. The UI uses it to render the right logo / notes / curated
   * model list.
   */
  provider?: string | null;
  /**
   * Reference into the top-level `credentials` map. When set, the
   * legacy `api_key` / `api_key_env` fields on this backend must be
   * absent; the resolved credential's key is used at request time.
   */
  credential?: string | null;
  api_key?: string | null;
  api_key_env?: string | null;
}

/**
 * Reusable API-key credential. Same `(api_key | api_key_env)` shape
 * as the legacy per-backend fields, just lifted into a named table
 * so multiple backends can share one secret.
 */
export interface CredentialSpec {
  api_key?: string | null;
  api_key_env?: string | null;
}

export type ModelMapEntry = string | { backend: string; tag: string };

/** Which call side this alias serves. */
export type AliasKind = "chat" | "embed";

/** Selection strategy across the alias's ordered targets. */
export type AliasStrategy = "first";

export interface AliasTarget {
  backend: string;
  tag: string;
  /**
   * Per-request upstream timeout in milliseconds. Honoured by
   * both chat and embed paths; absent = the backend's default.
   */
  timeout_ms?: number | null;
  /** Chat-only. Caps the upstream's max_tokens for this target. */
  max_tokens?: number | null;
  /** Chat-only. Pins the upstream sampling temperature. */
  temperature?: number | null;
}

export interface AliasSpec {
  kind?: AliasKind;
  strategy?: AliasStrategy;
  targets: AliasTarget[];
}

export interface BedrockSpec {
  default_backend?: string | null;
  credentials?: Record<string, CredentialSpec>;
  backends: Record<string, BedrockBackendSpec>;
  /**
   * Multi-target alias groups keyed by Bedrock id. The runtime checks
   * these before the legacy `invoke` / `embed` tables, so they can
   * layer a primary + fallback ordering on top of an existing
   * single-target mapping.
   */
  aliases?: Record<string, AliasSpec>;
  invoke: Record<string, ModelMapEntry>;
  embed: Record<string, ModelMapEntry>;
}

export interface BedrockSection {
  enabled: boolean;
  spec: BedrockSpec;
}

export interface SesSection {
  retention_hours: number;
}

export interface IamSection {
  enforce: boolean;
}

export interface LoggingSection {
  level: string;
}

export interface RuntimeConfig {
  bedrock: BedrockSection;
  ses: SesSection;
  iam: IamSection;
  logging: LoggingSection;
}

export interface RuntimeConfigEnvelope {
  config: RuntimeConfig;
  persistent: boolean;
  configPath: string | null;
}

const ENDPOINT = "/_awsim/runtime-config";

export async function getRuntimeConfig(): Promise<RuntimeConfigEnvelope> {
  const res = await fetch(ENDPOINT);
  if (!res.ok) {
    throw new Error(`runtime-config GET failed (HTTP ${res.status})`);
  }
  return (await res.json()) as RuntimeConfigEnvelope;
}

export async function getRuntimeConfigDefaults(): Promise<RuntimeConfig> {
  const res = await fetch(`${ENDPOINT}/defaults`);
  if (!res.ok) {
    throw new Error(`runtime-config defaults GET failed (HTTP ${res.status})`);
  }
  return (await res.json()) as RuntimeConfig;
}

export async function putRuntimeConfig(
  cfg: RuntimeConfig,
): Promise<RuntimeConfigEnvelope> {
  const res = await fetch(ENDPOINT, {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(cfg),
  });
  if (!res.ok) {
    let message = `runtime-config PUT failed (HTTP ${res.status})`;
    try {
      const err = (await res.json()) as { message?: string };
      if (err.message) {
        message = err.message;
      }
    } catch {
      /* fall through with default message */
    }
    throw new Error(message);
  }
  return (await res.json()) as RuntimeConfigEnvelope;
}
