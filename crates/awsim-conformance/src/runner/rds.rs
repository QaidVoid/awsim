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

    // ---- Aurora cluster lifecycle ----

    // CreateDBCluster
    results.push(chk!(
        "CreateDBCluster",
        client
            .create_db_cluster()
            .db_cluster_identifier("conformance-cluster")
            .engine("aurora-postgresql")
            .master_username("admin")
            .master_user_password("Password123!")
            .send()
            .await,
        verbose
    ));

    // CreateDBInstance (Aurora cluster member)
    results.push(chk!(
        "CreateDBInstance:aurora-member",
        client
            .create_db_instance()
            .db_instance_identifier("conformance-cluster-1")
            .db_instance_class("db.r6g.large")
            .engine("aurora-postgresql")
            .db_cluster_identifier("conformance-cluster")
            .send()
            .await,
        verbose
    ));

    // DescribeDBClusters
    results.push(chk!(
        "DescribeDBClusters",
        client.describe_db_clusters().send().await,
        verbose
    ));

    // ModifyDBCluster
    results.push(chk!(
        "ModifyDBCluster",
        client
            .modify_db_cluster()
            .db_cluster_identifier("conformance-cluster")
            .backup_retention_period(7)
            .apply_immediately(true)
            .send()
            .await,
        verbose
    ));

    // CreateDBClusterSnapshot
    results.push(chk!(
        "CreateDBClusterSnapshot",
        client
            .create_db_cluster_snapshot()
            .db_cluster_snapshot_identifier("conformance-cluster-snap")
            .db_cluster_identifier("conformance-cluster")
            .send()
            .await,
        verbose
    ));

    // DescribeDBClusterSnapshots
    results.push(chk!(
        "DescribeDBClusterSnapshots",
        client.describe_db_cluster_snapshots().send().await,
        verbose
    ));

    // FailoverDBCluster
    results.push(chk!(
        "FailoverDBCluster",
        client
            .failover_db_cluster()
            .db_cluster_identifier("conformance-cluster")
            .send()
            .await,
        verbose
    ));

    // CreateDBClusterParameterGroup
    results.push(chk!(
        "CreateDBClusterParameterGroup",
        client
            .create_db_cluster_parameter_group()
            .db_cluster_parameter_group_name("conformance-cpg")
            .db_parameter_group_family("aurora-postgresql16")
            .description("conformance")
            .send()
            .await,
        verbose
    ));

    // DescribeDBClusterParameterGroups
    results.push(chk!(
        "DescribeDBClusterParameterGroups",
        client.describe_db_cluster_parameter_groups().send().await,
        verbose
    ));

    // DescribeDBClusterParameters
    results.push(chk!(
        "DescribeDBClusterParameters",
        client
            .describe_db_cluster_parameters()
            .db_cluster_parameter_group_name("conformance-cpg")
            .send()
            .await,
        verbose
    ));

    // RestoreDBClusterFromSnapshot
    results.push(chk!(
        "RestoreDBClusterFromSnapshot",
        client
            .restore_db_cluster_from_snapshot()
            .db_cluster_identifier("conformance-cluster-restored")
            .snapshot_identifier("conformance-cluster-snap")
            .engine("aurora-postgresql")
            .send()
            .await,
        verbose
    ));

    // DeleteDBClusterSnapshot
    results.push(chk!(
        "DeleteDBClusterSnapshot",
        client
            .delete_db_cluster_snapshot()
            .db_cluster_snapshot_identifier("conformance-cluster-snap")
            .send()
            .await,
        verbose
    ));

    // DeleteDBInstance (Aurora member, before its cluster)
    results.push(chk!(
        "DeleteDBInstance:aurora-member",
        client
            .delete_db_instance()
            .db_instance_identifier("conformance-cluster-1")
            .skip_final_snapshot(true)
            .send()
            .await,
        verbose
    ));

    // DeleteDBCluster
    results.push(chk!(
        "DeleteDBCluster",
        client
            .delete_db_cluster()
            .db_cluster_identifier("conformance-cluster")
            .skip_final_snapshot(true)
            .send()
            .await,
        verbose
    ));

    results
}
