# Comprehend

Amazon Comprehend NLP service for extracting insights from text using natural language processing.

## Configuration

| Property | Value |
|----------|-------|
| Protocol | `AwsJson1_1` |
| Signing Name | `comprehend` |
| Persistence | No |

## Operations

- `DetectEntities` — identify named entities (people, places, organizations, dates, etc.) in text
- `DetectKeyPhrases` — extract key phrases from a block of text
- `DetectSentiment` — determine the overall sentiment of text (POSITIVE, NEGATIVE, NEUTRAL, MIXED)
- `DetectDominantLanguage` — detect the dominant language of a document
- `BatchDetectEntities` — run entity detection on multiple documents in one call
- `BatchDetectKeyPhrases` — run key phrase extraction on multiple documents in one call

## Example

```bash
# Detect sentiment
aws --endpoint-url http://localhost:4567 \
  comprehend detect-sentiment \
  --text "I love using AWSim for local development!" \
  --language-code en

# Detect entities
aws --endpoint-url http://localhost:4567 \
  comprehend detect-entities \
  --text "Jeff Bezos founded Amazon in Seattle in 1994." \
  --language-code en

# Detect key phrases
aws --endpoint-url http://localhost:4567 \
  comprehend detect-key-phrases \
  --text "The quick brown fox jumps over the lazy dog." \
  --language-code en

# Detect dominant language
aws --endpoint-url http://localhost:4567 \
  comprehend detect-dominant-language \
  --text "Bonjour le monde"
```

## Notes

- AWSim's Comprehend uses **heuristic algorithms** rather than real ML models:
  - Entity detection uses pattern matching for common entity types (capitalized words, dates, numbers, email addresses).
  - Sentiment analysis uses keyword-based scoring (positive/negative word lists).
  - Language detection uses simple n-gram character frequency analysis.
  - Key phrase extraction identifies noun phrases using basic POS patterns.
- Results are approximate and intended for integration testing, not production NLP use.
- Comprehend has no persistent state — all operations are stateless text analysis.
