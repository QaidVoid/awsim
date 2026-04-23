use dashmap::DashMap;

#[derive(Debug, Clone)]
pub struct Lexicon {
    pub name: String,
    pub content: String,
    pub last_modified: u64,
}

#[derive(Debug, Clone)]
pub struct SpeechSynthesisTask {
    pub task_id: String,
    pub status: String,
    pub output_uri: String,
    pub text: String,
    pub voice_id: String,
    pub output_format: String,
    pub created_at: u64,
}

#[derive(Debug, Default)]
pub struct PollyState {
    pub lexicons: DashMap<String, Lexicon>,
    pub tasks: DashMap<String, SpeechSynthesisTask>,
}
