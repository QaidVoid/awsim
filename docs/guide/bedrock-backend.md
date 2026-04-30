# Bedrock Local LLM Backend

awsim's Bedrock service can proxy to any OpenAI-compatible local LLM server. This means SDK code that targets Bedrock — `InvokeModel`, `InvokeModelWithResponseStream`, `Converse`, `ConverseStream`, plus Titan / Cohere embeddings — runs against actual model inference instead of canned responses, all while staying fully offline.

## How it works

```text
SDK → awsim gateway → awsim-bedrock translators
                        ├── Anthropic Messages
                        ├── Amazon Titan
                        ├── Meta Llama
                        ├── Mistral
                        ├── Cohere Command
                        ├── Titan / Cohere embed
                        └── reqwest → backend /chat/completions or /embeddings
```

awsim hosts the AWS-shaped front door (Bedrock wire format, per-vendor request/response shapes). Real inference happens in your local LLM. When the backend is unreachable awsim falls back to canned responses so CI keeps working.

## Backend setup

Any server that speaks OpenAI's `/v1/chat/completions` + `/v1/embeddings` works. Picks for the most common ones:

### Ollama

```bash
# Install + pull the default models the built-in map points at:
ollama pull llama3.1:8b nomic-embed-text

# Then run awsim:
awsim --bedrock-backend http://localhost:11434/v1
```

### LM Studio

In the GUI: Developer → Start Server. Note the port (default `1234`).

```bash
awsim --bedrock-backend http://localhost:1234/v1
```

LM Studio uses the model name shown in its UI as the tag. Override the model map (see below) so awsim's defaults match.

### llama.cpp server

```bash
./server -m my-model.gguf -c 4096 --port 8080
awsim --bedrock-backend http://localhost:8080/v1
```

### vLLM

```bash
vllm serve meta-llama/Meta-Llama-3-8B-Instruct --port 8000
awsim --bedrock-backend http://localhost:8000/v1
```

### Hosted (OpenAI / Anthropic / etc.)

The translators target OpenAI's contract, so any hosted OpenAI-compatible endpoint works. Pass the API key:

```bash
awsim --bedrock-backend https://api.openai.com/v1 \
      --bedrock-api-key "$OPENAI_API_KEY"
```

This breaks the offline promise; only use when you've explicitly opted in.

## Configuration

| Flag | Env var | Default | What |
|------|---------|---------|------|
| `--bedrock-backend` | `AWSIM_BEDROCK_BACKEND` | _(unset)_ | Base URL ending in `/v1`. Unset = canned responses. |
| `--bedrock-api-key` | `AWSIM_BEDROCK_API_KEY` | _(unset)_ | Sent as `Authorization: Bearer <key>`. Most local servers don't need this. |
| `--bedrock-model-map` | `AWSIM_BEDROCK_MODEL_MAP` | _(built-in)_ | Path to a TOML file overriding the model id → backend tag map. |

## Model map

Maps AWS-style ids (`anthropic.claude-3-5-sonnet-20241022-v2:0`) to the tag your backend understands (`llama3.1:8b`, `qwen2.5:32b`, etc.). The built-in defaults skew toward Llama-family on Ollama:

```toml
[invoke]
"anthropic.claude-3-5-sonnet-20241022-v2:0" = "llama3.1:8b"
"anthropic.claude-3-haiku-20240307-v1:0"    = "llama3.1:8b"
"meta.llama3-1-70b-instruct-v1:0"           = "llama3.1:70b"
"meta.llama3-1-8b-instruct-v1:0"            = "llama3.1:8b"
"amazon.titan-text-express-v1"              = "llama3.1:8b"
"cohere.command-r-v1:0"                     = "llama3.1:8b"
"mistral.mistral-7b-instruct-v0:2"          = "mistral:7b"

[embed]
"amazon.titan-embed-text-v1"   = "nomic-embed-text"
"amazon.titan-embed-text-v2:0" = "nomic-embed-text"
"cohere.embed-english-v3"      = "nomic-embed-text"
"cohere.embed-multilingual-v3" = "nomic-embed-text"
```

Override per-id via your own TOML. User keys merge on top — defaults stay for unmentioned ids.

```toml
# my-models.toml — point Sonnet at a heavier local model
[invoke]
"anthropic.claude-3-5-sonnet-20241022-v2:0" = "qwen2.5:32b"

[embed]
"amazon.titan-embed-text-v2:0" = "mxbai-embed-large"
```

```bash
awsim --bedrock-backend http://localhost:11434/v1 \
      --bedrock-model-map ./my-models.toml
```

Unknown ids return `ResourceNotFoundException` from the proxy (matching real Bedrock for unsupported models) — so add an entry for any custom model id you call.

## What's translated

| Bedrock op | Bedrock format | OpenAI path |
|------------|---------------|-------------|
| `InvokeModel` (Anthropic) | Messages API | `/chat/completions` |
| `InvokeModel` (Titan / Llama / Mistral / Cohere Command) | per-vendor | `/chat/completions` |
| `InvokeModel` (Titan / Cohere embed) | per-vendor | `/embeddings` |
| `InvokeModelWithResponseStream` | per-vendor stream chunks | `/chat/completions` with `stream:true` |
| `Converse` | unified | `/chat/completions` |
| `ConverseStream` | unified stream chunks | `/chat/completions` with `stream:true` |

System prompts, message history, max tokens, temperature, top_p, and stop sequences all translate. Tool / image content blocks aren't proxied yet (text only).

## Limitations

- **Streaming framing**: awsim accumulates the full response and emits the full content in a JSON envelope. Real Bedrock streams use `vnd.amazon.eventstream` binary framing — adding that codec is on the roadmap. Inspection (curl, admin UI) sees real content; SDK clients that parse the binary frame format will see one big chunk rather than incremental tokens.
- **Tool use**: Anthropic and Converse tool-use content blocks are dropped on the way in. Round-trip tool calls aren't supported yet.
- **Image input**: Multi-modal blocks are dropped — the proxy is text-only.
- **Guardrails / customization jobs / knowledge bases**: management-side state only. Inference doesn't apply guardrails.

## Falling back to canned

When `--bedrock-backend` is unset or the backend errors out, awsim returns a deterministic canned response. The response is structurally valid (SDKs deserialize it correctly) so CI tests that just check wiring still pass.
