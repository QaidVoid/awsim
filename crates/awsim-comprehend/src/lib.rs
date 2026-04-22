//! AWS Comprehend NLP emulator for AWSim.
//!
//! Provides heuristic entity detection, key phrase extraction,
//! sentiment analysis, and language detection for local development.

mod operations;

use awsim_core::{AwsError, Protocol, RequestContext, ServiceHandler};
use serde_json::Value;

/// Amazon Comprehend service emulator.
pub struct ComprehendService;

impl ComprehendService {
    pub fn new() -> Self {
        Self
    }
}

impl Default for ComprehendService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ServiceHandler for ComprehendService {
    fn service_name(&self) -> &str {
        "comprehend"
    }

    fn protocol(&self) -> Protocol {
        Protocol::AwsJson1_1
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        _ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        match operation {
            "DetectEntities" => operations::detect::detect_entities(&input),
            "DetectKeyPhrases" => operations::detect::detect_key_phrases(&input),
            "DetectSentiment" => operations::detect::detect_sentiment(&input),
            "DetectDominantLanguage" => operations::detect::detect_dominant_language(&input),
            "BatchDetectEntities" => operations::detect::batch_detect_entities(&input),
            "BatchDetectKeyPhrases" => operations::detect::batch_detect_key_phrases(&input),
            _ => Err(AwsError::not_implemented(operation)),
        }
    }
}
