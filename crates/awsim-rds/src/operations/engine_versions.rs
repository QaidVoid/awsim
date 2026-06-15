use awsim_core::{AwsError, RequestContext};
use serde_json::{Value, json};

use super::{opt_str, require_str};
use crate::error::invalid_parameter;
use crate::ids::{default_engine_version, is_aurora_engine, now_iso8601};
use crate::state::{DbCustomEngineVersion, RdsState};

/// `DescribeDBEngineVersions` returns the built-in engine versions for
/// postgres, mysql, mariadb, aurora-postgresql, and aurora-mysql.
///
/// Custom engine versions registered via [`create_custom_db_engine_version`]
/// are merged into the result so callers polling for a fresh CEV's
/// lifecycle see it transition into `available`.
pub fn describe_db_engine_versions(state: &RdsState, input: &Value) -> Result<Value, AwsError> {
    let filter_engine = opt_str(input, "Engine");
    let filter_version = opt_str(input, "EngineVersion");
    let include_all = input
        .get("IncludeAll")
        .and_then(super::coerce_bool)
        .unwrap_or(false);

    let all_versions: Vec<Value> = vec![
        // PostgreSQL
        json!({
            "Engine": "postgres",
            "EngineVersion": "16.1",
            "DBParameterGroupFamily": "postgres16",
            "DBEngineDescription": "PostgreSQL",
            "DBEngineVersionDescription": "PostgreSQL 16.1-R3",
            "ValidUpgradeTarget": { "member": [] },
            "SupportedFeatureNames": { "member": [] },
            "Status": "available",
            "SupportsParallelQuery": false,
            "SupportsGlobalDatabases": false,
            "SupportsBabelfish": false,
        }),
        json!({
            "Engine": "postgres",
            "EngineVersion": "15.4",
            "DBParameterGroupFamily": "postgres15",
            "DBEngineDescription": "PostgreSQL",
            "DBEngineVersionDescription": "PostgreSQL 15.4-R3",
            "ValidUpgradeTarget": { "member": [] },
            "SupportedFeatureNames": { "member": [] },
            "Status": "available",
            "SupportsParallelQuery": false,
            "SupportsGlobalDatabases": false,
            "SupportsBabelfish": false,
        }),
        json!({
            "Engine": "postgres",
            "EngineVersion": "14.9",
            "DBParameterGroupFamily": "postgres14",
            "DBEngineDescription": "PostgreSQL",
            "DBEngineVersionDescription": "PostgreSQL 14.9-R3",
            "ValidUpgradeTarget": { "member": [] },
            "SupportedFeatureNames": { "member": [] },
            "Status": "available",
            "SupportsParallelQuery": false,
            "SupportsGlobalDatabases": false,
            "SupportsBabelfish": false,
        }),
        // MySQL
        json!({
            "Engine": "mysql",
            "EngineVersion": "8.0.35",
            "DBParameterGroupFamily": "mysql8.0",
            "DBEngineDescription": "MySQL Community Edition",
            "DBEngineVersionDescription": "MySQL 8.0.35",
            "ValidUpgradeTarget": { "member": [] },
            "SupportedFeatureNames": { "member": [] },
            "Status": "available",
            "SupportsParallelQuery": false,
            "SupportsGlobalDatabases": false,
            "SupportsBabelfish": false,
        }),
        json!({
            "Engine": "mysql",
            "EngineVersion": "8.0.28",
            "DBParameterGroupFamily": "mysql8.0",
            "DBEngineDescription": "MySQL Community Edition",
            "DBEngineVersionDescription": "MySQL 8.0.28",
            "ValidUpgradeTarget": { "member": [] },
            "SupportedFeatureNames": { "member": [] },
            "Status": "available",
            "SupportsParallelQuery": false,
            "SupportsGlobalDatabases": false,
            "SupportsBabelfish": false,
        }),
        json!({
            "Engine": "mysql",
            "EngineVersion": "5.7.44",
            "DBParameterGroupFamily": "mysql5.7",
            "DBEngineDescription": "MySQL Community Edition",
            "DBEngineVersionDescription": "MySQL 5.7.44",
            "ValidUpgradeTarget": { "member": [] },
            "SupportedFeatureNames": { "member": [] },
            "Status": "available",
            "SupportsParallelQuery": false,
            "SupportsGlobalDatabases": false,
            "SupportsBabelfish": false,
        }),
        // MariaDB
        json!({
            "Engine": "mariadb",
            "EngineVersion": "10.11.6",
            "DBParameterGroupFamily": "mariadb10.11",
            "DBEngineDescription": "MariaDB Community Edition",
            "DBEngineVersionDescription": "MariaDB 10.11.6",
            "ValidUpgradeTarget": { "member": [] },
            "SupportedFeatureNames": { "member": [] },
            "Status": "available",
            "SupportsParallelQuery": false,
            "SupportsGlobalDatabases": false,
            "SupportsBabelfish": false,
        }),
        json!({
            "Engine": "mariadb",
            "EngineVersion": "10.6.14",
            "DBParameterGroupFamily": "mariadb10.6",
            "DBEngineDescription": "MariaDB Community Edition",
            "DBEngineVersionDescription": "MariaDB 10.6.14",
            "ValidUpgradeTarget": { "member": [] },
            "SupportedFeatureNames": { "member": [] },
            "Status": "available",
            "SupportsParallelQuery": false,
            "SupportsGlobalDatabases": false,
            "SupportsBabelfish": false,
        }),
        // Aurora PostgreSQL
        json!({
            "Engine": "aurora-postgresql",
            "EngineVersion": "16.1",
            "DBParameterGroupFamily": "aurora-postgresql16",
            "DBEngineDescription": "Aurora PostgreSQL",
            "DBEngineVersionDescription": "Aurora PostgreSQL 16.1",
            "ValidUpgradeTarget": { "member": [] },
            "SupportedFeatureNames": { "member": [] },
            "Status": "available",
            "SupportsParallelQuery": false,
            "SupportsGlobalDatabases": true,
            "SupportsBabelfish": false,
        }),
        json!({
            "Engine": "aurora-postgresql",
            "EngineVersion": "15.4",
            "DBParameterGroupFamily": "aurora-postgresql15",
            "DBEngineDescription": "Aurora PostgreSQL",
            "DBEngineVersionDescription": "Aurora PostgreSQL 15.4",
            "ValidUpgradeTarget": { "member": [] },
            "SupportedFeatureNames": { "member": [] },
            "Status": "available",
            "SupportsParallelQuery": false,
            "SupportsGlobalDatabases": true,
            "SupportsBabelfish": false,
        }),
        json!({
            "Engine": "aurora-postgresql",
            "EngineVersion": "14.9",
            "DBParameterGroupFamily": "aurora-postgresql14",
            "DBEngineDescription": "Aurora PostgreSQL",
            "DBEngineVersionDescription": "Aurora PostgreSQL 14.9",
            "ValidUpgradeTarget": { "member": [] },
            "SupportedFeatureNames": { "member": [] },
            "Status": "available",
            "SupportsParallelQuery": false,
            "SupportsGlobalDatabases": true,
            "SupportsBabelfish": false,
        }),
        // Aurora MySQL
        json!({
            "Engine": "aurora-mysql",
            "EngineVersion": "8.0.mysql_aurora.3.04.0",
            "DBParameterGroupFamily": "aurora-mysql8.0",
            "DBEngineDescription": "Aurora MySQL",
            "DBEngineVersionDescription": "Aurora MySQL 3.04.0 (compatible with MySQL 8.0.28)",
            "ValidUpgradeTarget": { "member": [] },
            "SupportedFeatureNames": { "member": [] },
            "Status": "available",
            "SupportsParallelQuery": true,
            "SupportsGlobalDatabases": true,
            "SupportsBabelfish": false,
        }),
        json!({
            "Engine": "aurora-mysql",
            "EngineVersion": "8.0.mysql_aurora.3.02.0",
            "DBParameterGroupFamily": "aurora-mysql8.0",
            "DBEngineDescription": "Aurora MySQL",
            "DBEngineVersionDescription": "Aurora MySQL 3.02.0 (compatible with MySQL 8.0.23)",
            "ValidUpgradeTarget": { "member": [] },
            "SupportedFeatureNames": { "member": [] },
            "Status": "available",
            "SupportsParallelQuery": true,
            "SupportsGlobalDatabases": true,
            "SupportsBabelfish": false,
        }),
    ];

    let versions: Vec<Value> = all_versions
        .into_iter()
        .filter(|v| {
            if let Some(eng) = filter_engine
                && v["Engine"].as_str().unwrap_or("") != eng
            {
                return false;
            }
            if let Some(ver) = filter_version
                && v["EngineVersion"].as_str().unwrap_or("") != ver
            {
                return false;
            }
            true
        })
        .collect();

    // Append every custom engine version we've registered, filtered the
    // same way as the built-ins. `IncludeAll=true` surfaces `inactive`
    // CEVs that AWS would otherwise hide.
    let custom: Vec<Value> = state
        .custom_engine_versions
        .iter()
        .map(|entry| custom_engine_version_to_value(entry.value()))
        .filter(|v| {
            if !include_all && v["Status"].as_str() == Some("inactive") {
                return false;
            }
            if let Some(eng) = filter_engine
                && v["Engine"].as_str().unwrap_or("") != eng
            {
                return false;
            }
            if let Some(ver) = filter_version
                && v["EngineVersion"].as_str().unwrap_or("") != ver
            {
                return false;
            }
            true
        })
        .collect();

    let mut merged = versions;
    merged.extend(custom);

    Ok(json!({
        "DBEngineVersions": { "DBEngineVersion": merged },
        "Marker": null,
    }))
}

/// `DescribeOrderableDBInstanceOptions` returns the available instance
/// classes for an engine.
///
/// Aurora engines advertise cluster-capable classes (including the
/// `db.serverless` class used by Serverless v2) backed by `aurora`
/// storage, and report `SupportsClusters` so SDK clients pick the
/// cluster creation path. Standalone engines keep the provisioned
/// instance classes and the `gp2`/`io1`/`standard` storage tiers.
pub fn describe_orderable_db_instance_options(input: &Value) -> Result<Value, AwsError> {
    let engine = opt_str(input, "Engine").unwrap_or("mysql");
    let aurora = is_aurora_engine(engine);
    let engine_version =
        opt_str(input, "EngineVersion").unwrap_or_else(|| default_engine_version(engine));

    let classes: &[&str] = if aurora {
        &[
            "db.serverless",
            "db.t3.medium",
            "db.t4g.medium",
            "db.r6g.large",
            "db.r6g.xlarge",
            "db.r6g.2xlarge",
            "db.r5.large",
            "db.r5.xlarge",
            "db.r5.2xlarge",
        ]
    } else {
        &[
            "db.t3.micro",
            "db.t3.small",
            "db.t3.medium",
            "db.t3.large",
            "db.t3.xlarge",
            "db.t3.2xlarge",
            "db.m5.large",
            "db.m5.xlarge",
            "db.m5.2xlarge",
            "db.m5.4xlarge",
            "db.r5.large",
            "db.r5.xlarge",
            "db.r5.2xlarge",
            "db.r5.4xlarge",
        ]
    };

    let storage_types: &[&str] = if aurora {
        &["aurora"]
    } else {
        &["gp2", "io1", "standard"]
    };

    let options: Vec<Value> = classes
        .iter()
        .flat_map(|class| {
            storage_types.iter().map(move |storage| {
                json!({
                    "Engine": engine,
                    "EngineVersion": engine_version,
                    "DBInstanceClass": class,
                    "LicenseModel": "general-public-license",
                    "StorageType": storage,
                    "MultiAZCapable": true,
                    // Aurora scales reads through cluster reader instances
                    // rather than RDS-style read replicas.
                    "ReadReplicaCapable": !aurora,
                    "Vpc": true,
                    "SupportsStorageEncryption": true,
                    "SupportsIops": !aurora && storage == &"io1",
                    "SupportsEnhancedMonitoring": true,
                    "SupportsIAMDatabaseAuthentication": true,
                    "SupportsPerformanceInsights": true,
                    "SupportsClusters": aurora,
                    "AvailabilityZones": { "member": [
                        { "Name": "us-east-1a" },
                        { "Name": "us-east-1b" },
                        { "Name": "us-east-1c" },
                    ] },
                    "OrderableDBInstanceOption": [],
                })
            })
        })
        .collect();

    Ok(json!({
        "OrderableDBInstanceOptions": { "OrderableDBInstanceOption": options },
        "Marker": null,
    }))
}

/// Customer-allowed engine families for a CEV. Real AWS supports
/// Oracle and SQL Server custom builds; we accept the same surface
/// at the API boundary so SDK clients exercise unchanged.
const CUSTOM_ENGINE_FAMILIES: &[&str] = &[
    "custom-oracle-ee",
    "custom-oracle-se2",
    "custom-sqlserver-ee",
    "custom-sqlserver-se",
    "custom-sqlserver-web",
];

fn custom_engine_version_arn(
    partition: &str,
    region: &str,
    account: &str,
    engine: &str,
    version: &str,
) -> String {
    format!("arn:{partition}:rds:{region}:{account}:engine-version:{engine}:{version}")
}

fn custom_engine_version_to_value(cev: &DbCustomEngineVersion) -> Value {
    let mut obj = json!({
        "Engine": cev.engine,
        "EngineVersion": cev.engine_version,
        "DBEngineVersionArn": cev.db_engine_version_arn,
        "DBEngineVersionDescription": cev.description,
        "DBParameterGroupFamily": format!("{}-{}", cev.engine, cev.engine_version),
        "Status": cev.status,
        "CreateTime": cev.created_at,
        "SupportsParallelQuery": false,
        "SupportsGlobalDatabases": false,
        "SupportsBabelfish": false,
        "ValidUpgradeTarget": { "member": [] },
        "SupportedFeatureNames": { "member": [] },
    });
    if let Some(ref b) = cev.database_installation_files_s3_bucket_name {
        obj["DatabaseInstallationFilesS3BucketName"] = json!(b);
    }
    if let Some(ref p) = cev.database_installation_files_s3_prefix {
        obj["DatabaseInstallationFilesS3Prefix"] = json!(p);
    }
    if let Some(ref k) = cev.kms_key_id {
        obj["KMSKeyId"] = json!(k);
    }
    obj
}

/// `CreateCustomDBEngineVersion`. AWS validates the underlying
/// installation media asynchronously and surfaces the lifecycle
/// (`pending-validation` -> `available`). AWSim collapses the
/// validation step (we have no AMI to inspect) and goes straight
/// to `available` so SDK callers polling `DescribeDBEngineVersions`
/// see a terminal status without simulating wall-clock delay.
pub fn create_custom_db_engine_version(
    state: &RdsState,
    input: &Value,
    ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let engine = require_str(input, "Engine")?;
    let engine_version = require_str(input, "EngineVersion")?;
    if !CUSTOM_ENGINE_FAMILIES.contains(&engine) {
        return Err(invalid_parameter(format!(
            "Engine `{engine}` is not a custom engine family. Use one of: {}.",
            CUSTOM_ENGINE_FAMILIES.join(", ")
        )));
    }
    let key = (engine.to_string(), engine_version.to_string());
    if state.custom_engine_versions.contains_key(&key) {
        return Err(AwsError::bad_request(
            "CustomDBEngineVersionAlreadyExistsFault",
            format!("Custom engine version `{engine}` `{engine_version}` already exists."),
        ));
    }

    let arn = custom_engine_version_arn(
        &ctx.partition,
        &ctx.region,
        &ctx.account_id,
        engine,
        engine_version,
    );
    let cev = DbCustomEngineVersion {
        engine: engine.to_string(),
        engine_version: engine_version.to_string(),
        db_engine_version_arn: arn,
        // AWS goes through `pending-validation` first. AWSim has
        // nothing to validate, so we record the steady-state value
        // directly; callers that poll see `available` on the first
        // describe.
        status: "available".to_string(),
        description: opt_str(input, "Description").unwrap_or("").to_string(),
        database_installation_files_s3_bucket_name: opt_str(
            input,
            "DatabaseInstallationFilesS3BucketName",
        )
        .map(str::to_string),
        database_installation_files_s3_prefix: opt_str(input, "DatabaseInstallationFilesS3Prefix")
            .map(str::to_string),
        kms_key_id: opt_str(input, "KMSKeyId").map(str::to_string),
        created_at: now_iso8601(),
    };
    let result = custom_engine_version_to_value(&cev);
    state.custom_engine_versions.insert(key, cev);
    Ok(result)
}

/// `ModifyCustomDBEngineVersion` flips `Status` between `available`
/// and `inactive`. AWS uses `inactive` to prevent further use without
/// deleting the registration outright.
pub fn modify_custom_db_engine_version(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let engine = require_str(input, "Engine")?;
    let engine_version = require_str(input, "EngineVersion")?;
    let status = require_str(input, "Status")?;
    if !matches!(status, "available" | "inactive") {
        return Err(invalid_parameter(
            "Status must be `available` or `inactive`.",
        ));
    }
    let key = (engine.to_string(), engine_version.to_string());
    let mut cev = state.custom_engine_versions.get_mut(&key).ok_or_else(|| {
        AwsError::not_found(
            "CustomDBEngineVersionNotFoundFault",
            format!("Custom engine version `{engine}` `{engine_version}` does not exist."),
        )
    })?;
    cev.status = status.to_string();
    if let Some(desc) = opt_str(input, "Description") {
        cev.description = desc.to_string();
    }
    Ok(custom_engine_version_to_value(&cev))
}

/// `DeleteCustomDBEngineVersion` drops the registration. Real AWS
/// requires the CEV to be in `inactive` first; we mirror that so
/// clients exercising the lifecycle hit the same gate.
pub fn delete_custom_db_engine_version(
    state: &RdsState,
    input: &Value,
    _ctx: &RequestContext,
) -> Result<Value, AwsError> {
    let engine = require_str(input, "Engine")?;
    let engine_version = require_str(input, "EngineVersion")?;
    let key = (engine.to_string(), engine_version.to_string());
    let existing = state.custom_engine_versions.get(&key).ok_or_else(|| {
        AwsError::not_found(
            "CustomDBEngineVersionNotFoundFault",
            format!("Custom engine version `{engine}` `{engine_version}` does not exist."),
        )
    })?;
    if existing.status != "inactive" {
        return Err(AwsError::bad_request(
            "InvalidCustomDBEngineVersionStateFault",
            format!(
                "Custom engine version `{engine}` `{engine_version}` must be in \
                 `inactive` status before deletion (current: `{}`).",
                existing.status
            ),
        ));
    }
    let result = custom_engine_version_to_value(&existing);
    drop(existing);
    state.custom_engine_versions.remove(&key);
    Ok(result)
}

#[cfg(test)]
mod custom_engine_version_tests {
    use super::*;

    fn ctx() -> RequestContext {
        RequestContext::new("rds", "us-east-1")
    }

    #[test]
    fn create_reaches_available_immediately() {
        let state = RdsState::default();
        let resp = create_custom_db_engine_version(
            &state,
            &json!({
                "Engine": "custom-oracle-ee",
                "EngineVersion": "19.cdb_cev1",
                "Description": "Custom Oracle 19c",
            }),
            &ctx(),
        )
        .unwrap();
        assert_eq!(resp["Status"], json!("available"));
        assert!(
            resp["DBEngineVersionArn"]
                .as_str()
                .unwrap()
                .contains("engine-version:custom-oracle-ee:19.cdb_cev1"),
            "ARN should reference the engine + version: {resp}"
        );
    }

    #[test]
    fn create_rejects_non_custom_engine() {
        let state = RdsState::default();
        let err = create_custom_db_engine_version(
            &state,
            &json!({
                "Engine": "postgres",
                "EngineVersion": "16.1",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterValue");
    }

    #[test]
    fn duplicate_cev_is_rejected() {
        let state = RdsState::default();
        create_custom_db_engine_version(
            &state,
            &json!({
                "Engine": "custom-sqlserver-se",
                "EngineVersion": "15.00.4322.2.cev1",
            }),
            &ctx(),
        )
        .unwrap();
        let err = create_custom_db_engine_version(
            &state,
            &json!({
                "Engine": "custom-sqlserver-se",
                "EngineVersion": "15.00.4322.2.cev1",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "CustomDBEngineVersionAlreadyExistsFault");
    }

    #[test]
    fn describe_includes_active_cev_by_default() {
        let state = RdsState::default();
        create_custom_db_engine_version(
            &state,
            &json!({
                "Engine": "custom-oracle-ee",
                "EngineVersion": "19.cdb_cev1",
            }),
            &ctx(),
        )
        .unwrap();
        let resp =
            describe_db_engine_versions(&state, &json!({ "Engine": "custom-oracle-ee" })).unwrap();
        let entries = resp["DBEngineVersions"]["DBEngineVersion"]
            .as_array()
            .unwrap();
        assert!(entries.iter().any(|v| v["EngineVersion"] == "19.cdb_cev1"));
    }

    #[test]
    fn describe_excludes_inactive_cev_unless_include_all_set() {
        let state = RdsState::default();
        create_custom_db_engine_version(
            &state,
            &json!({
                "Engine": "custom-oracle-ee",
                "EngineVersion": "19.cdb_cev1",
            }),
            &ctx(),
        )
        .unwrap();
        modify_custom_db_engine_version(
            &state,
            &json!({
                "Engine": "custom-oracle-ee",
                "EngineVersion": "19.cdb_cev1",
                "Status": "inactive",
            }),
            &ctx(),
        )
        .unwrap();

        let hidden =
            describe_db_engine_versions(&state, &json!({ "Engine": "custom-oracle-ee" })).unwrap();
        assert!(
            hidden["DBEngineVersions"]["DBEngineVersion"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        let shown = describe_db_engine_versions(
            &state,
            &json!({ "Engine": "custom-oracle-ee", "IncludeAll": true }),
        )
        .unwrap();
        assert_eq!(
            shown["DBEngineVersions"]["DBEngineVersion"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn delete_requires_inactive_status() {
        let state = RdsState::default();
        create_custom_db_engine_version(
            &state,
            &json!({
                "Engine": "custom-sqlserver-ee",
                "EngineVersion": "15.00.cev1",
            }),
            &ctx(),
        )
        .unwrap();
        // available -> delete is rejected
        let err = delete_custom_db_engine_version(
            &state,
            &json!({
                "Engine": "custom-sqlserver-ee",
                "EngineVersion": "15.00.cev1",
            }),
            &ctx(),
        )
        .unwrap_err();
        assert_eq!(err.code, "InvalidCustomDBEngineVersionStateFault");

        // inactive -> delete works
        modify_custom_db_engine_version(
            &state,
            &json!({
                "Engine": "custom-sqlserver-ee",
                "EngineVersion": "15.00.cev1",
                "Status": "inactive",
            }),
            &ctx(),
        )
        .unwrap();
        delete_custom_db_engine_version(
            &state,
            &json!({
                "Engine": "custom-sqlserver-ee",
                "EngineVersion": "15.00.cev1",
            }),
            &ctx(),
        )
        .unwrap();
        assert!(state.custom_engine_versions.is_empty());
    }
}

#[cfg(test)]
mod aurora_discovery_tests {
    use super::*;

    fn versions_for(engine: &str) -> Vec<Value> {
        let state = RdsState::default();
        let resp = describe_db_engine_versions(&state, &json!({ "Engine": engine })).unwrap();
        resp["DBEngineVersions"]["DBEngineVersion"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    #[test]
    fn engine_versions_advertise_aurora_postgresql_families() {
        let entries = versions_for("aurora-postgresql");
        assert!(!entries.is_empty());
        assert!(entries.iter().all(|v| v["Engine"] == "aurora-postgresql"));
        assert!(
            entries
                .iter()
                .any(|v| v["DBParameterGroupFamily"] == "aurora-postgresql16")
        );
        assert!(entries.iter().all(|v| v["SupportsGlobalDatabases"] == true));
    }

    #[test]
    fn engine_versions_advertise_aurora_mysql_family() {
        let entries = versions_for("aurora-mysql");
        assert!(!entries.is_empty());
        assert!(entries.iter().all(|v| v["Engine"] == "aurora-mysql"));
        assert!(
            entries
                .iter()
                .all(|v| v["DBParameterGroupFamily"] == "aurora-mysql8.0")
        );
        assert!(entries.iter().all(|v| v["SupportsParallelQuery"] == true));
    }

    #[test]
    fn orderable_options_for_aurora_use_cluster_storage_and_classes() {
        let resp = describe_orderable_db_instance_options(&json!({
            "Engine": "aurora-postgresql",
        }))
        .unwrap();
        let options = resp["OrderableDBInstanceOptions"]["OrderableDBInstanceOption"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(!options.is_empty());
        assert!(options.iter().all(|o| o["StorageType"] == "aurora"));
        assert!(options.iter().all(|o| o["SupportsClusters"] == true));
        assert!(options.iter().all(|o| o["EngineVersion"] == "16.1"));
        assert!(
            options
                .iter()
                .any(|o| o["DBInstanceClass"] == "db.serverless")
        );
    }

    #[test]
    fn orderable_options_for_standalone_engine_stay_provisioned() {
        let resp =
            describe_orderable_db_instance_options(&json!({ "Engine": "postgres" })).unwrap();
        let options = resp["OrderableDBInstanceOptions"]["OrderableDBInstanceOption"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(options.iter().all(|o| o["SupportsClusters"] == false));
        assert!(options.iter().any(|o| o["StorageType"] == "gp2"));
        assert!(
            options
                .iter()
                .all(|o| o["DBInstanceClass"] != "db.serverless")
        );
    }
}
