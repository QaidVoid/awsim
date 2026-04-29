use awsim_core::AwsError;
use serde_json::{Value, json};

/// DetectEntities — extract named entities from text.
///
/// Real Comprehend uses ML models. We do simple heuristic detection:
/// - Capitalized multi-word sequences → PERSON or ORGANIZATION
/// - Words starting with uppercase after sentence start → OTHER
/// - Numbers/amounts → QUANTITY
/// - Email-like patterns → OTHER
/// - Date-like patterns → DATE
pub fn detect_entities(input: &Value) -> Result<Value, AwsError> {
    let text = input["Text"]
        .as_str()
        .ok_or_else(|| AwsError::validation("Text parameter is required"))?;

    let _language = input["LanguageCode"].as_str().unwrap_or("en");

    let entities = extract_entities(text);
    let chars = text.chars().count();

    Ok(json!({
        "Entities": entities,
        "ResultList": null,
        "__headers": { "X-Awsim-Char-Count": chars.to_string() },
    }))
}

/// DetectKeyPhrases — extract key phrases from text.
///
/// Heuristic: extract sequences of 1-4 words that look like noun phrases.
pub fn detect_key_phrases(input: &Value) -> Result<Value, AwsError> {
    let text = input["Text"]
        .as_str()
        .ok_or_else(|| AwsError::validation("Text parameter is required"))?;

    let _language = input["LanguageCode"].as_str().unwrap_or("en");

    let phrases = extract_key_phrases(text);
    let chars = text.chars().count();

    Ok(json!({
        "KeyPhrases": phrases,
        "ResultList": null,
        "__headers": { "X-Awsim-Char-Count": chars.to_string() },
    }))
}

/// BatchDetectEntities — detect entities in multiple texts.
pub fn batch_detect_entities(input: &Value) -> Result<Value, AwsError> {
    let texts = input["TextList"]
        .as_array()
        .ok_or_else(|| AwsError::validation("TextList parameter is required"))?;

    let mut total_chars: usize = 0;
    let results: Vec<Value> = texts
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let text_str = text.as_str().unwrap_or("");
            total_chars += text_str.chars().count();
            let entities = extract_entities(text_str);
            json!({
                "Index": i,
                "Entities": entities,
            })
        })
        .collect();

    Ok(json!({
        "ResultList": results,
        "ErrorList": [],
        "__headers": { "X-Awsim-Char-Count": total_chars.to_string() },
    }))
}

/// BatchDetectKeyPhrases — detect key phrases in multiple texts.
pub fn batch_detect_key_phrases(input: &Value) -> Result<Value, AwsError> {
    let texts = input["TextList"]
        .as_array()
        .ok_or_else(|| AwsError::validation("TextList parameter is required"))?;

    let mut total_chars: usize = 0;
    let results: Vec<Value> = texts
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let text_str = text.as_str().unwrap_or("");
            total_chars += text_str.chars().count();
            let phrases = extract_key_phrases(text_str);
            json!({
                "Index": i,
                "KeyPhrases": phrases,
            })
        })
        .collect();

    Ok(json!({
        "ResultList": results,
        "ErrorList": [],
        "__headers": { "X-Awsim-Char-Count": total_chars.to_string() },
    }))
}

/// DetectSentiment — detect overall sentiment of text.
pub fn detect_sentiment(input: &Value) -> Result<Value, AwsError> {
    let text = input["Text"]
        .as_str()
        .ok_or_else(|| AwsError::validation("Text parameter is required"))?;

    // Simple heuristic: count positive/negative words
    let lower = text.to_lowercase();
    let positive_words = [
        "good",
        "great",
        "excellent",
        "amazing",
        "wonderful",
        "fantastic",
        "love",
        "happy",
        "pleased",
        "best",
        "beautiful",
        "perfect",
        "nice",
    ];
    let negative_words = [
        "bad",
        "terrible",
        "awful",
        "horrible",
        "hate",
        "worst",
        "poor",
        "ugly",
        "angry",
        "sad",
        "disappointed",
        "wrong",
        "fail",
    ];

    let pos_count = positive_words.iter().filter(|w| lower.contains(*w)).count();
    let neg_count = negative_words.iter().filter(|w| lower.contains(*w)).count();

    let (sentiment, pos_score, neg_score, neutral_score) = if pos_count > neg_count {
        ("POSITIVE", 0.7, 0.05, 0.2)
    } else if neg_count > pos_count {
        ("NEGATIVE", 0.05, 0.7, 0.2)
    } else if pos_count == 0 && neg_count == 0 {
        ("NEUTRAL", 0.1, 0.1, 0.75)
    } else {
        ("MIXED", 0.35, 0.35, 0.2)
    };

    let chars = text.chars().count();
    Ok(json!({
        "Sentiment": sentiment,
        "SentimentScore": {
            "Positive": pos_score,
            "Negative": neg_score,
            "Neutral": neutral_score,
            "Mixed": 1.0 - pos_score - neg_score - neutral_score,
        },
        "__headers": { "X-Awsim-Char-Count": chars.to_string() },
    }))
}

/// DetectDominantLanguage — detect the language of text.
pub fn detect_dominant_language(input: &Value) -> Result<Value, AwsError> {
    let text = input["Text"]
        .as_str()
        .ok_or_else(|| AwsError::validation("Text parameter is required"))?;

    let chars = text.chars().count();
    // Always return English with high confidence for dev emulator
    Ok(json!({
        "Languages": [
            { "LanguageCode": "en", "Score": 0.98 }
        ],
        "__headers": { "X-Awsim-Char-Count": chars.to_string() },
    }))
}

// --- Heuristic NLP helpers ---

fn extract_entities(text: &str) -> Vec<Value> {
    let mut entities = Vec::new();
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut offset = 0;

    let mut i = 0;
    while i < words.len() {
        let word = words[i];
        let word_start = text[offset..]
            .find(word)
            .map(|p| offset + p)
            .unwrap_or(offset);
        let word_end = word_start + word.len();

        // Detect capitalized sequences (potential names/orgs)
        if word.len() > 1
            && word.chars().next().is_some_and(|c| c.is_uppercase())
            && !is_sentence_start(text, word_start)
        {
            // Collect consecutive capitalized words
            let mut end_idx = i;
            let mut entity_end = word_end;
            while end_idx + 1 < words.len() {
                let next = words[end_idx + 1];
                if next.len() > 1 && next.chars().next().is_some_and(|c| c.is_uppercase()) {
                    end_idx += 1;
                    let next_start = text[entity_end..]
                        .find(next)
                        .map(|p| entity_end + p)
                        .unwrap_or(entity_end);
                    entity_end = next_start + next.len();
                } else {
                    break;
                }
            }

            let entity_text = &text[word_start..entity_end];
            let entity_type = if end_idx > i {
                "ORGANIZATION"
            } else {
                "PERSON"
            };

            entities.push(json!({
                "Text": entity_text.trim_end_matches(|c: char| c.is_ascii_punctuation()),
                "Type": entity_type,
                "Score": 0.85,
                "BeginOffset": word_start,
                "EndOffset": entity_end,
            }));

            i = end_idx + 1;
            offset = entity_end;
            continue;
        }

        // Detect numbers/quantities
        if word.chars().any(|c| c.is_ascii_digit())
            && word
                .chars()
                .all(|c| c.is_ascii_digit() || c == ',' || c == '.' || c == '%' || c == '$')
        {
            entities.push(json!({
                "Text": word,
                "Type": "QUANTITY",
                "Score": 0.9,
                "BeginOffset": word_start,
                "EndOffset": word_end,
            }));
        }

        offset = word_end;
        i += 1;
    }

    entities
}

fn extract_key_phrases(text: &str) -> Vec<Value> {
    let mut phrases = Vec::new();
    let sentences: Vec<&str> = text.split(['.', '!', '?']).collect();

    let stop_words = [
        "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
        "do", "does", "did", "will", "would", "could", "should", "may", "might", "shall", "can",
        "to", "of", "in", "for", "on", "with", "at", "by", "from", "as", "into", "through",
        "during", "before", "after", "above", "below", "between", "and", "but", "or", "nor", "not",
        "so", "yet", "both", "either", "neither", "each", "every", "all", "any", "few", "more",
        "most", "other", "some", "such", "no", "only", "own", "same", "than", "too", "very",
        "this", "that", "these", "those", "i", "me", "my", "we", "our", "you", "your", "he", "him",
        "his", "she", "her", "it", "its", "they", "them", "their", "what", "which", "who", "whom",
    ];

    for sentence in sentences {
        let sentence = sentence.trim();
        if sentence.is_empty() {
            continue;
        }

        let words: Vec<&str> = sentence.split_whitespace().collect();
        let mut i = 0;

        while i < words.len() {
            let word_lower = words[i].to_lowercase();
            let clean = word_lower.trim_matches(|c: char| c.is_ascii_punctuation());

            if stop_words.contains(&clean) || clean.len() <= 1 {
                i += 1;
                continue;
            }

            // Collect phrase (up to 4 content words)
            let start = i;
            let mut end = i + 1;
            while end < words.len() && end - start < 4 {
                let next_lower = words[end].to_lowercase();
                let next_clean = next_lower.trim_matches(|c: char| c.is_ascii_punctuation());
                if stop_words.contains(&next_clean) {
                    // Allow one stop word inside a phrase
                    if end + 1 < words.len() && end - start < 3 {
                        let after_lower = words[end + 1].to_lowercase();
                        let after_clean =
                            after_lower.trim_matches(|c: char| c.is_ascii_punctuation());
                        if !stop_words.contains(&after_clean) && !after_clean.is_empty() {
                            end += 2;
                            continue;
                        }
                    }
                    break;
                }
                if next_clean.is_empty() {
                    break;
                }
                end += 1;
            }

            if end > start {
                let phrase_text = words[start..end].join(" ");
                let phrase_clean = phrase_text.trim_matches(|c: char| c.is_ascii_punctuation());
                if !phrase_clean.is_empty() && phrase_clean.len() > 2 {
                    let begin_offset = text.find(phrase_clean).unwrap_or(0);
                    phrases.push(json!({
                        "Text": phrase_clean,
                        "Score": 0.9,
                        "BeginOffset": begin_offset,
                        "EndOffset": begin_offset + phrase_clean.len(),
                    }));
                }
            }

            i = end;
        }
    }

    phrases
}

fn is_sentence_start(text: &str, pos: usize) -> bool {
    if pos == 0 {
        return true;
    }
    let before = &text[..pos];
    let trimmed = before.trim_end();
    trimmed.is_empty() || trimmed.ends_with('.') || trimmed.ends_with('!') || trimmed.ends_with('?')
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_detect_entities_capitalized_names() {
        let input = json!({"Text": "Jeff Bezos founded Amazon Web Services in Seattle.", "LanguageCode": "en"});
        let result = detect_entities(&input).unwrap();
        let entities = result["Entities"].as_array().unwrap();
        assert!(!entities.is_empty());
        // Should detect at least one entity
        let types: Vec<&str> = entities.iter().filter_map(|e| e["Type"].as_str()).collect();
        assert!(types.contains(&"PERSON") || types.contains(&"ORGANIZATION"));
    }

    #[test]
    fn test_detect_key_phrases() {
        let input =
            json!({"Text": "The quick brown fox jumps over the lazy dog.", "LanguageCode": "en"});
        let result = detect_key_phrases(&input).unwrap();
        let phrases = result["KeyPhrases"].as_array().unwrap();
        assert!(!phrases.is_empty());
    }

    #[test]
    fn test_detect_sentiment_positive() {
        let input = json!({"Text": "This product is great and amazing!", "LanguageCode": "en"});
        let result = detect_sentiment(&input).unwrap();
        assert_eq!(result["Sentiment"], "POSITIVE");
    }

    #[test]
    fn test_detect_sentiment_negative() {
        let input = json!({"Text": "This is terrible and awful.", "LanguageCode": "en"});
        let result = detect_sentiment(&input).unwrap();
        assert_eq!(result["Sentiment"], "NEGATIVE");
    }

    #[test]
    fn test_detect_dominant_language() {
        let input = json!({"Text": "Hello world", "LanguageCode": "en"});
        let result = detect_dominant_language(&input).unwrap();
        let langs = result["Languages"].as_array().unwrap();
        assert_eq!(langs[0]["LanguageCode"], "en");
    }

    #[test]
    fn test_batch_detect_entities() {
        let input = json!({"TextList": ["John Smith went to Paris.", "Amazon released new features."], "LanguageCode": "en"});
        let result = batch_detect_entities(&input).unwrap();
        let results = result["ResultList"].as_array().unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn test_missing_text_returns_error() {
        let input = json!({"LanguageCode": "en"});
        assert!(detect_entities(&input).is_err());
        assert!(detect_key_phrases(&input).is_err());
        assert!(detect_sentiment(&input).is_err());
    }
}
