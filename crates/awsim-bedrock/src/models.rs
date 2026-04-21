/// Hardcoded foundation models available in the emulator.
pub struct FoundationModel {
    pub id: &'static str,
    pub provider: &'static str,
    pub name: &'static str,
    pub modalities: &'static [&'static str],
}

pub static FOUNDATION_MODELS: &[FoundationModel] = &[
    FoundationModel {
        id: "anthropic.claude-3-5-sonnet-20241022-v2:0",
        provider: "Anthropic",
        name: "Claude 3.5 Sonnet v2",
        modalities: &["TEXT", "IMAGE"],
    },
    FoundationModel {
        id: "anthropic.claude-3-haiku-20240307-v1:0",
        provider: "Anthropic",
        name: "Claude 3 Haiku",
        modalities: &["TEXT", "IMAGE"],
    },
    FoundationModel {
        id: "anthropic.claude-v2:1",
        provider: "Anthropic",
        name: "Claude v2.1",
        modalities: &["TEXT"],
    },
    FoundationModel {
        id: "meta.llama3-1-70b-instruct-v1:0",
        provider: "Meta",
        name: "Llama 3.1 70B Instruct",
        modalities: &["TEXT"],
    },
    FoundationModel {
        id: "amazon.titan-text-express-v1",
        provider: "Amazon",
        name: "Titan Text Express",
        modalities: &["TEXT"],
    },
    FoundationModel {
        id: "amazon.titan-embed-text-v2:0",
        provider: "Amazon",
        name: "Titan Text Embeddings V2",
        modalities: &["EMBEDDING"],
    },
    FoundationModel {
        id: "cohere.command-r-plus-v1:0",
        provider: "Cohere",
        name: "Command R+",
        modalities: &["TEXT"],
    },
    FoundationModel {
        id: "stability.stable-diffusion-xl-v1",
        provider: "Stability AI",
        name: "SDXL 1.0",
        modalities: &["IMAGE"],
    },
];

pub fn model_to_json(m: &FoundationModel) -> serde_json::Value {
    serde_json::json!({
        "modelId": m.id,
        "modelName": m.name,
        "providerName": m.provider,
        "inputModalities": m.modalities,
        "outputModalities": m.modalities,
        "modelLifecycle": { "status": "ACTIVE" },
    })
}
