/**
 * Model Gateway admin API. Phase 0 surfaces the bundled provider
 * catalog used to power the "Add backend" picker; further phases
 * extend this with credentials, aliases, health, and metrics.
 */

export type ProviderKind = "local" | "hosted" | "aws" | "custom";
export type AuthKind = "none" | "bearer";

export interface CatalogModel {
  id: string;
  context: number;
  modalities: string[];
  /** "chat" for completion / Converse targets, "embed" for embeddings. */
  kind: "chat" | "embed";
}

export interface CatalogProvider {
  key: string;
  name: string;
  /** Lucide icon name; see iconFor() in the gateway page for mapping. */
  icon: string;
  kind: ProviderKind;
  endpoint_template: string;
  auth: AuthKind;
  env_hint: string | null;
  docs_url: string | null;
  notes: string | null;
  models: CatalogModel[];
}

export interface ProviderCatalog {
  schema_version: number;
  providers: CatalogProvider[];
}

export async function getGatewayCatalog(): Promise<ProviderCatalog> {
  const res = await fetch("/_awsim/gateway/catalog");
  if (!res.ok) {
    throw new Error(`gateway/catalog failed (HTTP ${res.status})`);
  }
  return (await res.json()) as ProviderCatalog;
}
