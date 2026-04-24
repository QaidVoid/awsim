use crate::chk;
use crate::runner::common::*;

pub async fn test_rds(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_rds::Client::new(&config);
    let mut results = Vec::new();

    // DescribeDBEngineVersions
    results.push(chk!(
        "DescribeDBEngineVersions",
        client.describe_db_engine_versions().send().await,
        verbose
    ));

    // CreateDBInstance
    let db_r = client
        .create_db_instance()
        .db_instance_identifier("conformance-db")
        .db_instance_class("db.t3.micro")
        .engine("mysql")
        .master_username("admin")
        .master_user_password("Password123!")
        .allocated_storage(20)
        .send()
        .await;
    results.push(chk!("CreateDBInstance", db_r, verbose));

    // DescribeDBInstances
    results.push(chk!(
        "DescribeDBInstances",
        client.describe_db_instances().send().await,
        verbose
    ));

    // CreateDBSnapshot
    let snap_r = client
        .create_db_snapshot()
        .db_instance_identifier("conformance-db")
        .db_snapshot_identifier("conformance-snapshot")
        .send()
        .await;
    results.push(chk!("CreateDBSnapshot", snap_r, verbose));

    // DescribeDBSnapshots
    results.push(chk!(
        "DescribeDBSnapshots",
        client.describe_db_snapshots().send().await,
        verbose
    ));

    // DeleteDBSnapshot
    results.push(chk!(
        "DeleteDBSnapshot",
        client
            .delete_db_snapshot()
            .db_snapshot_identifier("conformance-snapshot")
            .send()
            .await,
        verbose
    ));

    // DeleteDBInstance
    results.push(chk!(
        "DeleteDBInstance",
        client
            .delete_db_instance()
            .db_instance_identifier("conformance-db")
            .skip_final_snapshot(true)
            .send()
            .await,
        verbose
    ));

    results
}
