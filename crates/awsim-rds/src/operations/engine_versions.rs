use awsim_core::AwsError;
use serde_json::{Value, json};

use super::opt_str;

/// DescribeDBEngineVersions — return hardcoded engine versions for postgres, mysql, mariadb.
pub fn describe_db_engine_versions(input: &Value) -> Result<Value, AwsError> {
    let filter_engine = opt_str(input, "Engine");
    let filter_version = opt_str(input, "EngineVersion");

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
    ];

    let versions: Vec<Value> = all_versions
        .into_iter()
        .filter(|v| {
            if let Some(eng) = filter_engine {
                if v["Engine"].as_str().unwrap_or("") != eng {
                    return false;
                }
            }
            if let Some(ver) = filter_version {
                if v["EngineVersion"].as_str().unwrap_or("") != ver {
                    return false;
                }
            }
            true
        })
        .collect();

    Ok(json!({
        "DBEngineVersions": { "DBEngineVersion": versions },
        "Marker": null,
    }))
}

/// DescribeOrderableDBInstanceOptions — return available instance classes per engine.
pub fn describe_orderable_db_instance_options(input: &Value) -> Result<Value, AwsError> {
    let engine = opt_str(input, "Engine").unwrap_or("mysql");

    let classes = vec![
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
    ];

    let storage_types = vec!["gp2", "io1", "standard"];

    let options: Vec<Value> = classes
        .iter()
        .flat_map(|class| {
            storage_types.iter().map(move |storage| {
                json!({
                    "Engine": engine,
                    "EngineVersion": "8.0.35",
                    "DBInstanceClass": class,
                    "LicenseModel": "general-public-license",
                    "StorageType": storage,
                    "MultiAZCapable": true,
                    "ReadReplicaCapable": true,
                    "Vpc": true,
                    "SupportsStorageEncryption": true,
                    "SupportsIops": storage == &"io1",
                    "SupportsEnhancedMonitoring": true,
                    "SupportsIAMDatabaseAuthentication": true,
                    "SupportsPerformanceInsights": true,
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
