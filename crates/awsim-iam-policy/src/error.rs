use std::fmt;

#[derive(Debug)]
pub enum ParseError {
    InvalidJson(serde_json::Error),
    MissingField(String),
    InvalidEffect(String),
    InvalidConditionOperator(String),
    InvalidArn(String),
    UnknownVersion(String),
    InvalidShape(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::InvalidJson(e) => write!(f, "invalid json: {e}"),
            ParseError::MissingField(s) => write!(f, "missing field: {s}"),
            ParseError::InvalidEffect(s) => write!(f, "invalid effect: {s}"),
            ParseError::InvalidConditionOperator(s) => {
                write!(f, "invalid condition operator: {s}")
            }
            ParseError::InvalidArn(s) => write!(f, "invalid arn: {s}"),
            ParseError::UnknownVersion(s) => write!(f, "unknown policy version: {s}"),
            ParseError::InvalidShape(s) => write!(f, "invalid shape: {s}"),
        }
    }
}

impl std::error::Error for ParseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ParseError::InvalidJson(e) => Some(e),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for ParseError {
    fn from(value: serde_json::Error) -> Self {
        ParseError::InvalidJson(value)
    }
}
