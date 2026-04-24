use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

pub fn list_voices(_input: &Value, _ctx: &RequestContext) -> Result<Value, AwsError> {
    Ok(json!({
        "Voices": [
            {
                "Gender": "Female",
                "Id": "Joanna",
                "LanguageCode": "en-US",
                "LanguageName": "US English",
                "Name": "Joanna",
                "SupportedEngines": ["standard", "neural"],
            },
            {
                "Gender": "Male",
                "Id": "Matthew",
                "LanguageCode": "en-US",
                "LanguageName": "US English",
                "Name": "Matthew",
                "SupportedEngines": ["standard", "neural"],
            },
            {
                "Gender": "Female",
                "Id": "Amy",
                "LanguageCode": "en-GB",
                "LanguageName": "British English",
                "Name": "Amy",
                "SupportedEngines": ["standard", "neural"],
            }
        ]
    }))
}
