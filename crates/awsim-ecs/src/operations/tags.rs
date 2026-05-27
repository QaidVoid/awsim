use serde_json::Value;

/// Parse an ECS `tags` input shape: `[{"key": "...", "value": "..."}]`.
/// Entries missing either field are dropped. Returns an empty vector
/// when input is absent or empty.
pub fn parse_tags(value: Option<&Value>) -> Vec<(String, String)> {
    let Some(arr) = value.and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|t| {
            let key = t.get("key").and_then(|v| v.as_str())?.to_string();
            let value = t.get("value").and_then(|v| v.as_str())?.to_string();
            Some((key, value))
        })
        .collect()
}

/// Merge two tag lists, overlaying `overlay` on top of `base`. AWS
/// resolves key collisions in favour of the later source, which for
/// ECS means caller-supplied tags win over service / task-definition
/// inherited tags.
pub fn merge_tags(
    base: &[(String, String)],
    overlay: &[(String, String)],
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = base.to_vec();
    for (k, v) in overlay {
        if let Some(existing) = out.iter_mut().find(|(name, _)| name == k) {
            existing.1 = v.clone();
        } else {
            out.push((k.clone(), v.clone()));
        }
    }
    out
}

/// AWS-managed tags ECS attaches to tasks when `enableECSManagedTags`
/// is true on a service. The keys are documented as `aws:ecs:clusterName`
/// and `aws:ecs:serviceName`.
pub fn ecs_managed_tags(cluster_name: &str, service_name: Option<&str>) -> Vec<(String, String)> {
    let mut tags = vec![("aws:ecs:clusterName".to_string(), cluster_name.to_string())];
    if let Some(name) = service_name {
        tags.push(("aws:ecs:serviceName".to_string(), name.to_string()));
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_tags_skips_malformed_entries() {
        let input = json!([
            { "key": "env", "value": "prod" },
            { "key": "missing-value" },
            { "value": "missing-key" },
            { "key": "team", "value": "data" }
        ]);
        let tags = parse_tags(Some(&input));
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0], ("env".to_string(), "prod".to_string()));
        assert_eq!(tags[1], ("team".to_string(), "data".to_string()));
    }

    #[test]
    fn merge_tags_overlay_wins_on_key_collision() {
        let base = vec![
            ("env".into(), "prod".into()),
            ("team".into(), "data".into()),
        ];
        let overlay = vec![
            ("env".into(), "canary".into()),
            ("owner".into(), "alice".into()),
        ];
        let merged = merge_tags(&base, &overlay);
        assert_eq!(merged.len(), 3);
        let env = merged.iter().find(|(k, _)| k == "env").unwrap();
        assert_eq!(env.1, "canary");
        assert!(merged.iter().any(|(k, _)| k == "owner"));
    }

    #[test]
    fn ecs_managed_tags_emits_cluster_and_optional_service() {
        let with_service = ecs_managed_tags("default", Some("api"));
        assert!(
            with_service
                .iter()
                .any(|(k, v)| k == "aws:ecs:clusterName" && v == "default")
        );
        assert!(
            with_service
                .iter()
                .any(|(k, v)| k == "aws:ecs:serviceName" && v == "api")
        );

        let no_service = ecs_managed_tags("default", None);
        assert!(no_service.iter().all(|(k, _)| k != "aws:ecs:serviceName"));
    }
}
