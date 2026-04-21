use std::collections::HashMap;

use dashmap::DashMap;

/// A Cognito User Pool.
#[derive(Debug, Clone)]
pub struct UserPool {
    pub id: String,
    pub name: String,
    pub arn: String,
    pub clients: HashMap<String, UserPoolClient>,
    pub users: HashMap<String, CognitoUser>,
    pub groups: HashMap<String, CognitoGroup>,
    pub created_date: u64,
}

/// A Cognito User Pool App Client.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct UserPoolClient {
    pub client_id: String,
    pub client_name: String,
    pub user_pool_id: String,
    pub explicit_auth_flows: Vec<String>,
    pub created_date: u64,
}

/// A Cognito user.
#[derive(Debug, Clone)]
pub struct CognitoUser {
    pub username: String,
    pub sub: String,
    /// Store plaintext for dev/emulator use.
    pub password: String,
    pub attributes: HashMap<String, String>,
    /// CONFIRMED | UNCONFIRMED | FORCE_CHANGE_PASSWORD
    pub status: String,
    pub enabled: bool,
    pub groups: Vec<String>,
    pub created_date: u64,
}

/// A Cognito User Pool group.
#[derive(Debug, Clone)]
pub struct CognitoGroup {
    pub group_name: String,
    pub description: Option<String>,
    pub role_arn: Option<String>,
    pub user_pool_id: String,
    pub created_date: u64,
}

/// A simple revocation store for invalidated tokens.
#[derive(Debug, Default, Clone)]
pub struct TokenRevocationStore {
    /// Set of access token strings that have been signed out.
    pub revoked: DashMap<String, ()>,
}

/// Per-account/region Cognito state.
#[derive(Debug, Default, Clone)]
pub struct CognitoState {
    /// PoolId → UserPool
    pub user_pools: DashMap<String, UserPool>,
    /// Revoked tokens (GlobalSignOut).
    pub revoked_tokens: TokenRevocationStore,
}
