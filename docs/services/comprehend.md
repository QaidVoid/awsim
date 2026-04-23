# Comprehend

Amazon Comprehend NLP service for extracting insights from text using natural language processing.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `comprehend` |
| Target Prefix | `Comprehend_20171127` |
| Persistence | No |

## Quick Start

Detect sentiment and entities in a piece of text:

```bash
# Detect sentiment
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Comprehend_20171127.DetectSentiment" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/comprehend/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Text":"I absolutely love using AWSim for local development! It saves so much time.","LanguageCode":"en"}'

# Detect entities
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Comprehend_20171127.DetectEntities" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/comprehend/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Text":"Jeff Bezos founded Amazon in Seattle in 1994.","LanguageCode":"en"}'
```

## Operations

- `DetectEntities` — identify named entities in text
  - Input: `Text` (required), `LanguageCode` (e.g., `en`, `es`, `fr`, `de`, `it`, `pt`, `ar`, `hi`, `ja`, `ko`, `zh`)
  - Returns: `Entities` list, each with `Text`, `Type` (`PERSON`, `LOCATION`, `ORGANIZATION`, `DATE`, `QUANTITY`, `EVENT`, `TITLE`, `OTHER`), `Score` (confidence 0.0–1.0), `BeginOffset`, `EndOffset`

- `DetectKeyPhrases` — extract key phrases from a block of text
  - Input: `Text`, `LanguageCode`
  - Returns: `KeyPhrases` list, each with `Text`, `Score`, `BeginOffset`, `EndOffset`

- `DetectSentiment` — determine the overall sentiment of text
  - Input: `Text`, `LanguageCode`
  - Returns: `Sentiment` (`POSITIVE`, `NEGATIVE`, `NEUTRAL`, `MIXED`), `SentimentScore` with `Positive`, `Negative`, `Neutral`, `Mixed` float values summing to 1.0

- `DetectDominantLanguage` — detect the dominant language of a document
  - Input: `Text` (no `LanguageCode` needed)
  - Returns: `Languages` list, each with `LanguageCode` (BCP-47 code, e.g., `en`) and `Score` (confidence)

- `BatchDetectEntities` — run entity detection on multiple documents in one call
  - Input: `TextList` (list of strings, max 25), `LanguageCode`
  - Returns: `ResultList` (list of entity results by index), `ErrorList` (any failed items)

- `BatchDetectKeyPhrases` — run key phrase extraction on multiple documents
  - Input: `TextList` (max 25), `LanguageCode`
  - Returns: `ResultList`, `ErrorList`

## Curl Examples

```bash
# 1. Detect key phrases
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Comprehend_20171127.DetectKeyPhrases" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/comprehend/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Text":"The quick brown fox jumps over the lazy dog near the river bank.","LanguageCode":"en"}'

# 2. Detect dominant language
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Comprehend_20171127.DetectDominantLanguage" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/comprehend/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"Text":"Bonjour le monde, comment allez-vous?"}'

# 3. Batch entity detection across multiple documents
curl -s http://localhost:4566 \
  -H "Content-Type: application/x-amz-json-1.1" \
  -H "X-Amz-Target: Comprehend_20171127.BatchDetectEntities" \
  -H "Authorization: AWS4-HMAC-SHA256 Credential=test/20260421/us-east-1/comprehend/aws4_request, SignedHeaders=host, Signature=fake" \
  -d '{"TextList":["Apple was founded by Steve Jobs.","Microsoft is headquartered in Redmond, Washington."],"LanguageCode":"en"}'
```

## SDK Example

```typescript
import {
  ComprehendClient,
  DetectSentimentCommand,
  DetectEntitiesCommand,
  DetectKeyPhrasesCommand,
  DetectDominantLanguageCommand,
} from '@aws-sdk/client-comprehend';

const comprehend = new ComprehendClient({
  region: 'us-east-1',
  endpoint: 'http://localhost:4566',
  credentials: { accessKeyId: 'test', secretAccessKey: 'test' },
});

const text = 'I love using AWS services in Seattle! The pricing is great.';

// Detect sentiment
const { Sentiment, SentimentScore } = await comprehend.send(new DetectSentimentCommand({
  Text: text,
  LanguageCode: 'en',
}));
console.log('Sentiment:', Sentiment); // POSITIVE
console.log('Scores:', SentimentScore);

// Detect entities
const { Entities } = await comprehend.send(new DetectEntitiesCommand({
  Text: text,
  LanguageCode: 'en',
}));
console.log('Entities:', Entities?.map(e => `${e.Text} (${e.Type})`));
// e.g., ["Seattle (LOCATION)", "AWS (ORGANIZATION)"]

// Detect language
const { Languages } = await comprehend.send(new DetectDominantLanguageCommand({
  Text: text,
}));
console.log('Language:', Languages?.[0]?.LanguageCode); // en
```

## Behavior Notes

- AWSim's Comprehend uses **heuristic algorithms** rather than real ML models:
  - **Entity detection** uses pattern matching for common types: capitalized words become `PERSON` or `ORGANIZATION`, dates match `DATE`, numbers match `QUANTITY`.
  - **Sentiment analysis** uses positive/negative word lists — texts with words like "love", "great", "excellent" score `POSITIVE`.
  - **Language detection** uses simple n-gram character frequency analysis to pick the closest language.
  - **Key phrase extraction** identifies noun phrase patterns using basic POS tagging.
- Results are approximate and intended for integration testing of your NLP pipeline, not production NLP use.
- All operations are fully stateless — no data is retained between calls.
- Confidence scores (`Score`) are simulated and do not reflect real model confidence.
