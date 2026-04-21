use std::collections::HashMap;

use dashmap::DashMap;

/// A single image stored in a repository.
#[derive(Debug, Clone)]
pub struct ContainerImage {
    pub image_digest: String,
    pub image_tag: Option<String>,
    pub image_manifest: String,
    pub pushed_at: String,
    pub image_size_in_bytes: u64,
}

/// An ECR repository.
#[derive(Debug)]
pub struct Repository {
    pub name: String,
    pub arn: String,
    pub registry_id: String,
    pub repository_uri: String,
    pub images: Vec<ContainerImage>,
    pub created_at: String,
    pub image_tag_mutability: String,
    pub tags: HashMap<String, String>,
}

/// Per-account/region ECR state.
#[derive(Debug, Default)]
pub struct EcrState {
    /// repositoryName → Repository
    pub repositories: DashMap<String, Repository>,
}
