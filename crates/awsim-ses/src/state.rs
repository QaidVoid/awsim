use dashmap::DashMap;

/// A sent email stored locally for debugging/admin.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct SentEmail {
    pub message_id: String,
    pub from: String,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub subject: Option<String>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub raw: Option<String>,
    pub sent_at: u64,
}

/// An email identity (address or domain).
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct EmailIdentity {
    pub identity: String,
    pub verified: bool,
    pub identity_type: String, // "EMAIL_ADDRESS" or "DOMAIN"
    pub created_at: u64,
}

/// An email template.
#[derive(Debug, Clone)]
pub struct EmailTemplate {
    pub name: String,
    pub subject: Option<String>,
    pub html: Option<String>,
    pub text: Option<String>,
    pub created_at: u64,
}

/// Per-account/region SES state.
#[derive(Debug, Default)]
pub struct SesState {
    /// MessageId → SentEmail
    pub sent_emails: DashMap<String, SentEmail>,
    /// identity → EmailIdentity
    pub identities: DashMap<String, EmailIdentity>,
    /// template name → EmailTemplate
    pub templates: DashMap<String, EmailTemplate>,
}
