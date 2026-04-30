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
  api_key?: string | null;
  api_key_env?: string | null;
}

export type ModelMapEntry = string | { backend: string; tag: string };

export interface BedrockSpec {
  default_backend?: string | null;
  backends: Record<string, BedrockBackendSpec>;
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
