//! Aurora cluster behavior contract: drive the real AWS RDS SDK against a
//! live in-process AWSim through the full cluster lifecycle and assert the
//! results, not just response shape. These cover parity that the envelope-only
//! coverage runner cannot catch: cluster membership and writer/reader roles,
//! failover, Serverless v2 scaling round-trip, and numeric/boolean option
//! coercion over the awsQuery wire.

use aws_sdk_rds::types::ServerlessV2ScalingConfiguration;

async fn rds_client() -> aws_sdk_rds::Client {
    let endpoint = awsim_conformance::server::start().await;
    let config = awsim_conformance::runner::common::make_config(&endpoint).await;
    aws_sdk_rds::Client::new(&config)
}

#[tokio::test]
async fn aurora_cluster_members_failover_snapshot_and_restore() {
    let client = rds_client().await;

    // Create a Serverless v2 Aurora PostgreSQL cluster.
    client
        .create_db_cluster()
        .db_cluster_identifier("lifecycle-cluster")
        .engine("aurora-postgresql")
        .engine_version("15.4")
        .master_username("admin")
        .master_user_password("Password123!")
        .serverless_v2_scaling_configuration(
            ServerlessV2ScalingConfiguration::builder()
                .min_capacity(0.5)
                .max_capacity(16.0)
                .build(),
        )
        .send()
        .await
        .expect("create cluster");

    // The Serverless v2 capacities must round-trip (they cross the awsQuery
    // wire as strings, so this also guards the numeric coercion fix).
    let described = client
        .describe_db_clusters()
        .db_cluster_identifier("lifecycle-cluster")
        .send()
        .await
        .expect("describe cluster");
    let cluster = &described.db_clusters()[0];
    let scaling = cluster
        .serverless_v2_scaling_configuration()
        .expect("serverless v2 config present");
    assert_eq!(scaling.min_capacity(), Some(0.5));
    assert_eq!(scaling.max_capacity(), Some(16.0));

    // Add two instances. The first to join is the writer, the second a reader.
    for id in ["lifecycle-1", "lifecycle-2"] {
        client
            .create_db_instance()
            .db_instance_identifier(id)
            .db_instance_class("db.r6g.large")
            .engine("aurora-postgresql")
            .db_cluster_identifier("lifecycle-cluster")
            .send()
            .await
            .unwrap_or_else(|e| panic!("create instance {id}: {e:?}"));
    }

    let members = describe_members(&client, "lifecycle-cluster").await;
    assert_eq!(members.len(), 2, "cluster should have two members");
    assert_eq!(
        writer(&members),
        Some("lifecycle-1".to_string()),
        "first instance to join is the writer"
    );

    // Snapshot the cluster.
    client
        .create_db_cluster_snapshot()
        .db_cluster_snapshot_identifier("lifecycle-snap")
        .db_cluster_identifier("lifecycle-cluster")
        .send()
        .await
        .expect("create cluster snapshot");
    let snaps = client
        .describe_db_cluster_snapshots()
        .db_cluster_snapshot_identifier("lifecycle-snap")
        .send()
        .await
        .expect("describe cluster snapshots");
    assert_eq!(snaps.db_cluster_snapshots().len(), 1);

    // Failover to the reader: it must become the writer.
    client
        .failover_db_cluster()
        .db_cluster_identifier("lifecycle-cluster")
        .target_db_instance_identifier("lifecycle-2")
        .send()
        .await
        .expect("failover");
    let members = describe_members(&client, "lifecycle-cluster").await;
    assert_eq!(
        writer(&members),
        Some("lifecycle-2".to_string()),
        "failover should promote the target to writer"
    );

    // Restore a new cluster from the snapshot.
    client
        .restore_db_cluster_from_snapshot()
        .db_cluster_identifier("lifecycle-restored")
        .snapshot_identifier("lifecycle-snap")
        .engine("aurora-postgresql")
        .send()
        .await
        .expect("restore cluster");
    let restored = client
        .describe_db_clusters()
        .db_cluster_identifier("lifecycle-restored")
        .send()
        .await
        .expect("describe restored");
    assert_eq!(
        restored.db_clusters()[0].engine(),
        Some("aurora-postgresql")
    );

    // Tear down: members first, then the cluster (deletion protection is off).
    for id in ["lifecycle-1", "lifecycle-2"] {
        client
            .delete_db_instance()
            .db_instance_identifier(id)
            .skip_final_snapshot(true)
            .send()
            .await
            .unwrap_or_else(|e| panic!("delete instance {id}: {e:?}"));
    }
    client
        .delete_db_cluster()
        .db_cluster_identifier("lifecycle-cluster")
        .skip_final_snapshot(true)
        .send()
        .await
        .expect("delete cluster");
}

#[tokio::test]
async fn modify_db_cluster_applies_scalar_changes() {
    let client = rds_client().await;
    client
        .create_db_cluster()
        .db_cluster_identifier("modify-cluster")
        .engine("aurora-postgresql")
        .master_username("admin")
        .master_user_password("Password123!")
        .send()
        .await
        .expect("create cluster");

    client
        .modify_db_cluster()
        .db_cluster_identifier("modify-cluster")
        .backup_retention_period(7)
        .deletion_protection(true)
        .apply_immediately(true)
        .send()
        .await
        .expect("modify cluster");

    let cluster = client
        .describe_db_clusters()
        .db_cluster_identifier("modify-cluster")
        .send()
        .await
        .expect("describe");
    let c = &cluster.db_clusters()[0];
    assert_eq!(c.backup_retention_period(), Some(7));
    assert_eq!(c.deletion_protection(), Some(true));
}

#[tokio::test]
async fn instance_numeric_and_boolean_options_round_trip() {
    // Guards the awsQuery coercion fix: numeric and boolean options must
    // survive the wire rather than falling back to defaults.
    let client = rds_client().await;
    client
        .create_db_instance()
        .db_instance_identifier("opts-db")
        .db_instance_class("db.t3.micro")
        .engine("postgres")
        .master_username("admin")
        .master_user_password("Password123!")
        .allocated_storage(50)
        .publicly_accessible(true)
        .backup_retention_period(7)
        .send()
        .await
        .expect("create instance");

    let described = client
        .describe_db_instances()
        .db_instance_identifier("opts-db")
        .send()
        .await
        .expect("describe");
    let inst = &described.db_instances()[0];
    assert_eq!(inst.allocated_storage(), Some(50));
    assert_eq!(inst.publicly_accessible(), Some(true));
}

async fn describe_members(
    client: &aws_sdk_rds::Client,
    cluster_id: &str,
) -> Vec<aws_sdk_rds::types::DbClusterMember> {
    client
        .describe_db_clusters()
        .db_cluster_identifier(cluster_id)
        .send()
        .await
        .expect("describe cluster")
        .db_clusters()[0]
        .db_cluster_members()
        .to_vec()
}

fn writer(members: &[aws_sdk_rds::types::DbClusterMember]) -> Option<String> {
    members
        .iter()
        .find(|m| m.is_cluster_writer() == Some(true))
        .and_then(|m| m.db_instance_identifier().map(str::to_string))
}
