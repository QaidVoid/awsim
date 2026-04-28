/**
 * Decode/sniff captured request bodies for the inspect drawer.
 *
 * The server captures bodies as base64 because S3, ECR and other binary
 * services can ship arbitrary payloads. The UI tries to interpret each
 * body using the captured Content-Type header, falling back to:
 *   1. UTF-8 string + JSON pretty-print if it parses
 *   2. UTF-8 string verbatim if it parses
 *   3. Hex dump of the first 256 bytes
 */

import type { CapturedBody, CapturedHeader } from "./events";

export type DecodedKind = "json" | "xml" | "form" | "text" | "binary" | "empty";

export interface DecodedBody {
  kind: DecodedKind;
  text: string;
  contentType: string | null;
  size: number;
  truncated: boolean;
}

export function findContentType(headers: CapturedHeader[]): string | null {
  const h = headers.find((x) => x.name.toLowerCase() === "content-type");
  return h ? h.value : null;
}

function decodeBase64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
  return out;
}

function tryUtf8(bytes: Uint8Array): string | null {
  try {
    // `fatal: true` bails on invalid UTF-8 instead of replacing chars
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return null;
  }
}

function hexDump(bytes: Uint8Array, max = 256): string {
  const slice = bytes.subarray(0, Math.min(bytes.length, max));
  const lines: string[] = [];
  for (let off = 0; off < slice.length; off += 16) {
    const row = slice.subarray(off, off + 16);
    const hex: string[] = [];
    const ascii: string[] = [];
    for (let i = 0; i < 16; i++) {
      if (i < row.length) {
        hex.push(row[i].toString(16).padStart(2, "0"));
        const c = row[i];
        ascii.push(c >= 0x20 && c < 0x7f ? String.fromCharCode(c) : ".");
      } else {
        hex.push("  ");
        ascii.push(" ");
      }
    }
    lines.push(
      `${off.toString(16).padStart(8, "0")}  ${hex.slice(0, 8).join(" ")}  ${hex
        .slice(8)
        .join(" ")}  ${ascii.join("")}`,
    );
  }
  return lines.join("\n");
}

function sniffKind(contentType: string | null, text: string): DecodedKind {
  const ct = (contentType ?? "").toLowerCase();
  if (ct.includes("json")) return "json";
  if (ct.includes("xml")) return "xml";
  if (ct.includes("x-www-form-urlencoded")) return "form";
  // Heuristic fallbacks for AWS responses that omit Content-Type
  const trimmed = text.trimStart();
  if (trimmed.startsWith("{") || trimmed.startsWith("[")) return "json";
  if (trimmed.startsWith("<")) return "xml";
  return "text";
}

function prettyJson(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

export function decodeBody(
  body: CapturedBody,
  headers: CapturedHeader[],
): DecodedBody {
  const contentType = findContentType(headers);
  const size = body.size;
  const truncated = body.truncated;

  if (!body.data_b64) {
    return { kind: "empty", text: "", contentType, size, truncated };
  }

  const bytes = decodeBase64ToBytes(body.data_b64);
  const text = tryUtf8(bytes);

  if (text === null) {
    return {
      kind: "binary",
      text: hexDump(bytes),
      contentType,
      size,
      truncated,
    };
  }

  const kind = sniffKind(contentType, text);
  const out = kind === "json" ? prettyJson(text) : text;
  return { kind, text: out, contentType, size, truncated };
}

/**
 * Synthesize a `curl` command that reproduces the captured request, for
 * easy copy-paste replay outside the UI.
 */
export function toCurl(
  method: string,
  url: string,
  headers: CapturedHeader[],
  body: CapturedBody,
): string {
  const parts: string[] = [`curl -X ${method.toUpperCase()}`];
  for (const h of headers) {
    if (h.name.toLowerCase() === "host") continue;
    parts.push(`-H ${shellQuote(`${h.name}: ${h.value}`)}`);
  }
  if (body.data_b64) {
    const bytes = decodeBase64ToBytes(body.data_b64);
    const text = tryUtf8(bytes);
    if (text !== null) {
      parts.push(`--data-raw ${shellQuote(text)}`);
    } else {
      // Binary — surface a placeholder rather than a multi-MB base64 blob
      parts.push(`--data-binary @<binary-${body.size}-bytes>`);
    }
  }
  parts.push(shellQuote(url));
  return parts.join(" \\\n  ");
}

function shellQuote(s: string): string {
  return `'${s.replace(/'/g, "'\\''")}'`;
}
