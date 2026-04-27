/**
 * Typed Polly API client.
 *
 * Polly uses a REST-style API: voices, lexicons, and speech synthesis are
 * all hit at `/v1/...` paths.
 */

import { ENDPOINT, amzDate, authHeader, loggedFetch } from "$lib/aws";

const SERVICE = "polly";

// ---------- Types ----------

export interface Voice {
  id: string;
  name: string;
  gender: string;
  languageCode: string;
  languageName: string;
  supportedEngines: string[];
  additionalLanguageCodes: string[];
}

export interface Lexicon {
  name: string;
  alphabet: string | null;
  languageCode: string | null;
  lexemesCount?: number | null;
  size?: number | null;
  lastModified?: string | null;
}

export interface LexiconDetail extends Lexicon {
  content: string;
}

export interface SpeechSynthesisTask {
  taskId: string;
  taskStatus: string;
  voiceId: string;
  outputFormat: string;
  outputUri: string | null;
  creationTime: string | null;
  requestCharacters?: number;
  taskStatusReason?: string | null;
}

export interface SynthesisResult {
  audio: Blob;
  contentType: string;
  requestCharacters: number | null;
}

// ---------- Internal helpers ----------

interface RawVoice {
  Id?: string;
  Name?: string;
  Gender?: string;
  LanguageCode?: string;
  LanguageName?: string;
  SupportedEngines?: string[];
  AdditionalLanguageCodes?: string[];
}

interface RawLexiconAttributes {
  Alphabet?: string | null;
  LanguageCode?: string | null;
  LexemesCount?: number | null;
  Size?: number | null;
  LastModified?: string | null;
}

interface RawLexicon {
  Name?: string;
  Attributes?: RawLexiconAttributes | null;
}

interface RawLexiconResponse {
  Lexicon?: { Name?: string; Content?: string };
  LexiconAttributes?: RawLexiconAttributes;
}

interface RawSynthesisTask {
  TaskId?: string;
  TaskStatus?: string;
  VoiceId?: string;
  OutputFormat?: string;
  OutputUri?: string | null;
  CreationTime?: string | null;
  RequestCharacters?: number;
  TaskStatusReason?: string | null;
}

async function pollyFetchJson<T>(
  method: string,
  path: string,
  body?: unknown,
): Promise<T> {
  const operation = `${method} ${path}`;
  const headers: Record<string, string> = {
    Authorization: authHeader(SERVICE),
    "X-Amz-Date": amzDate(),
  };
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const res = await loggedFetch(
    SERVICE,
    operation,
    method,
    `${ENDPOINT}${path}`,
    {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    },
  );
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Polly ${operation} failed (HTTP ${res.status}): ${text}`);
  }
  const text = await res.text();
  return (text ? JSON.parse(text) : {}) as T;
}

function mapVoice(raw: RawVoice): Voice {
  return {
    id: raw.Id ?? "",
    name: raw.Name ?? "",
    gender: raw.Gender ?? "",
    languageCode: raw.LanguageCode ?? "",
    languageName: raw.LanguageName ?? "",
    supportedEngines: raw.SupportedEngines ?? [],
    additionalLanguageCodes: raw.AdditionalLanguageCodes ?? [],
  };
}

function mapLexicon(raw: RawLexicon): Lexicon {
  const a = raw.Attributes ?? {};
  return {
    name: raw.Name ?? "",
    alphabet: a.Alphabet ?? null,
    languageCode: a.LanguageCode ?? null,
    lexemesCount: a.LexemesCount ?? null,
    size: a.Size ?? null,
    lastModified: a.LastModified ?? null,
  };
}

function mapTask(raw: RawSynthesisTask): SpeechSynthesisTask {
  return {
    taskId: raw.TaskId ?? "",
    taskStatus: raw.TaskStatus ?? "",
    voiceId: raw.VoiceId ?? "",
    outputFormat: raw.OutputFormat ?? "",
    outputUri: raw.OutputUri ?? null,
    creationTime: raw.CreationTime ?? null,
    requestCharacters: raw.RequestCharacters,
    taskStatusReason: raw.TaskStatusReason ?? null,
  };
}

// ---------- Operations ----------

export async function describeVoices(languageCode?: string): Promise<Voice[]> {
  const qs = languageCode
    ? `?LanguageCode=${encodeURIComponent(languageCode)}`
    : "";
  const res = await pollyFetchJson<{ Voices?: RawVoice[] }>(
    "GET",
    `/v1/voices${qs}`,
  );
  return (res.Voices ?? []).map(mapVoice);
}

export async function listLexicons(): Promise<Lexicon[]> {
  const res = await pollyFetchJson<{ Lexicons?: RawLexicon[] }>(
    "GET",
    "/v1/lexicons",
  );
  return (res.Lexicons ?? []).map(mapLexicon);
}

export async function getLexicon(name: string): Promise<LexiconDetail> {
  const res = await pollyFetchJson<RawLexiconResponse>(
    "GET",
    `/v1/lexicons/${encodeURIComponent(name)}`,
  );
  const a = res.LexiconAttributes ?? {};
  return {
    name: res.Lexicon?.Name ?? name,
    content: res.Lexicon?.Content ?? "",
    alphabet: a.Alphabet ?? null,
    languageCode: a.LanguageCode ?? null,
    lexemesCount: a.LexemesCount ?? null,
    size: a.Size ?? null,
    lastModified: a.LastModified ?? null,
  };
}

export async function listSpeechSynthesisTasks(): Promise<
  SpeechSynthesisTask[]
> {
  const res = await pollyFetchJson<{
    SynthesisTasks?: RawSynthesisTask[];
  }>("GET", "/v1/synthesisTasks");
  return (res.SynthesisTasks ?? []).map(mapTask);
}

export interface SynthesizeOptions {
  text: string;
  voiceId: string;
  outputFormat?: "mp3" | "ogg_vorbis" | "pcm" | "json";
  engine?: "standard" | "neural";
  languageCode?: string;
  textType?: "text" | "ssml";
  sampleRate?: string;
}

/**
 * Calls SynthesizeSpeech (POST /v1/speech) and returns the audio bytes as a
 * Blob suitable for `URL.createObjectURL`.
 */
export async function synthesizeSpeech(
  opts: SynthesizeOptions,
): Promise<SynthesisResult> {
  const body: Record<string, unknown> = {
    Text: opts.text,
    VoiceId: opts.voiceId,
    OutputFormat: opts.outputFormat ?? "mp3",
  };
  if (opts.engine) body.Engine = opts.engine;
  if (opts.languageCode) body.LanguageCode = opts.languageCode;
  if (opts.textType) body.TextType = opts.textType;
  if (opts.sampleRate) body.SampleRate = opts.sampleRate;
  const res = await loggedFetch(
    SERVICE,
    "SynthesizeSpeech",
    "POST",
    `${ENDPOINT}/v1/speech`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: authHeader(SERVICE),
        "X-Amz-Date": amzDate(),
      },
      body: JSON.stringify(body),
    },
  );
  if (!res.ok) {
    const text = await res.text();
    throw new Error(
      `Polly SynthesizeSpeech failed (HTTP ${res.status}): ${text}`,
    );
  }
  const audio = await res.blob();
  const ct = res.headers.get("content-type") ?? "audio/mpeg";
  const reqCharsHeader = res.headers.get("x-amzn-requestcharacters");
  const requestCharacters = reqCharsHeader ? parseInt(reqCharsHeader, 10) : null;
  return { audio, contentType: ct, requestCharacters };
}
