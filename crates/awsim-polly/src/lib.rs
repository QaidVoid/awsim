mod operations;
mod state;

pub use state::PollyState;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

pub struct PollyService {
    store: AccountRegionStore<PollyState>,
}

impl PollyService {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }
}

impl Default for PollyService {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for PollyService {
    fn service_name(&self) -> &str {
        "polly"
    }

    fn signing_name(&self) -> &str {
        "polly"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestJson1
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/voices",
                operation: "DescribeVoices",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/speech",
                operation: "SynthesizeSpeech",
                required_query_param: None,
            },
            RouteDefinition {
                method: "PUT",
                path_pattern: "/v1/lexicons/{LexiconName}",
                operation: "PutLexicon",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/lexicons/{LexiconName}",
                operation: "GetLexicon",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/lexicons",
                operation: "ListLexicons",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/v1/lexicons/{LexiconName}",
                operation: "DeleteLexicon",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/v1/synthesisTasks",
                operation: "StartSpeechSynthesisTask",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/synthesisTasks/{TaskId}",
                operation: "GetSpeechSynthesisTask",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/v1/synthesisTasks",
                operation: "ListSpeechSynthesisTasks",
                required_query_param: None,
            },
        ]
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation, "Polly request");
        let state = self.store.get(&ctx.account_id, &ctx.region);

        let mut input = input;
        if let Value::Object(map) = &mut input
            && let Some(name) = map.remove("LexiconName") {
                map.insert("Name".to_string(), name);
            }

        match operation {
            "ListVoices" | "DescribeVoices" => operations::voices::list_voices(&input, ctx),
            "SynthesizeSpeech" => operations::speech::synthesize_speech(&state, &input, ctx),
            "PutLexicon" => operations::lexicons::put_lexicon(&state, &input, ctx),
            "GetLexicon" => operations::lexicons::get_lexicon(&state, &input, ctx),
            "ListLexicons" => operations::lexicons::list_lexicons(&state, &input, ctx),
            "DeleteLexicon" => operations::lexicons::delete_lexicon(&state, &input, ctx),
            "StartSpeechSynthesisTask" => {
                operations::speech::start_speech_synthesis_task(&state, &input, ctx)
            }
            "GetSpeechSynthesisTask" => {
                operations::speech::get_speech_synthesis_task(&state, &input, ctx)
            }
            "ListSpeechSynthesisTasks" => {
                operations::speech::list_speech_synthesis_tasks(&state, &input, ctx)
            }
            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
