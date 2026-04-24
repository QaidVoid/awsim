use crate::chk;
use crate::runner::common::*;

pub async fn test_kinesis(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_kinesis::Client::new(&config);
    let mut results = Vec::new();

    // CreateStream
    results.push(chk!(
        "CreateStream",
        client
            .create_stream()
            .stream_name("conformance-stream")
            .shard_count(1)
            .send()
            .await,
        verbose
    ));

    // ListStreams
    results.push(chk!(
        "ListStreams",
        client.list_streams().send().await,
        verbose
    ));

    // DescribeStream
    results.push(chk!(
        "DescribeStream",
        client
            .describe_stream()
            .stream_name("conformance-stream")
            .send()
            .await,
        verbose
    ));

    // DescribeStreamSummary
    results.push(chk!(
        "DescribeStreamSummary",
        client
            .describe_stream_summary()
            .stream_name("conformance-stream")
            .send()
            .await,
        verbose
    ));

    // PutRecord
    results.push(chk!(
        "PutRecord",
        client
            .put_record()
            .stream_name("conformance-stream")
            .partition_key("pk-1")
            .data(aws_sdk_kinesis::primitives::Blob::new(
                b"hello stream".to_vec()
            ))
            .send()
            .await,
        verbose
    ));

    // GetShardIterator — need to know shard ID first
    let describe_r = client
        .describe_stream()
        .stream_name("conformance-stream")
        .send()
        .await;
    let shard_id = describe_r
        .as_ref()
        .ok()
        .and_then(|r| r.stream_description.as_ref())
        .and_then(|sd| sd.shards.first())
        .map(|s| s.shard_id.clone());

    if let Some(ref sid) = shard_id {
        let iter_r = client
            .get_shard_iterator()
            .stream_name("conformance-stream")
            .shard_id(sid)
            .shard_iterator_type(aws_sdk_kinesis::types::ShardIteratorType::TrimHorizon)
            .send()
            .await;
        let shard_iter = iter_r.as_ref().ok().and_then(|r| r.shard_iterator.clone());
        results.push(chk!("GetShardIterator", iter_r, verbose));

        if let Some(iter) = shard_iter {
            results.push(chk!(
                "GetRecords",
                client.get_records().shard_iterator(iter).send().await,
                verbose
            ));
        } else {
            results.push(OpResult::Skipped("GetRecords".to_string()));
        }
    } else {
        results.push(OpResult::Skipped("GetShardIterator".to_string()));
        results.push(OpResult::Skipped("GetRecords".to_string()));
    }

    // ListShards
    results.push(chk!(
        "ListShards",
        client
            .list_shards()
            .stream_name("conformance-stream")
            .send()
            .await,
        verbose
    ));

    // PutRecords (batch)
    results.push(chk!(
        "PutRecords",
        client
            .put_records()
            .stream_name("conformance-stream")
            .records(
                aws_sdk_kinesis::types::PutRecordsRequestEntry::builder()
                    .partition_key("pk-batch")
                    .data(aws_sdk_kinesis::primitives::Blob::new(
                        b"batch record 1".to_vec()
                    ))
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // AddTagsToStream
    results.push(chk!(
        "AddTagsToStream",
        client
            .add_tags_to_stream()
            .stream_name("conformance-stream")
            .tags("env", "conformance")
            .send()
            .await,
        verbose
    ));

    // ListTagsForStream
    results.push(chk!(
        "ListTagsForStream",
        client
            .list_tags_for_stream()
            .stream_name("conformance-stream")
            .send()
            .await,
        verbose
    ));

    // RemoveTagsFromStream
    results.push(chk!(
        "RemoveTagsFromStream",
        client
            .remove_tags_from_stream()
            .stream_name("conformance-stream")
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // IncreaseStreamRetentionPeriod
    results.push(chk!(
        "IncreaseStreamRetentionPeriod",
        client
            .increase_stream_retention_period()
            .stream_name("conformance-stream")
            .retention_period_hours(48)
            .send()
            .await,
        verbose
    ));

    // DecreaseStreamRetentionPeriod (back to default 24h)
    results.push(chk!(
        "DecreaseStreamRetentionPeriod",
        client
            .decrease_stream_retention_period()
            .stream_name("conformance-stream")
            .retention_period_hours(24)
            .send()
            .await,
        verbose
    ));

    // MergeShards (requires 2 shards — will get service error = pass)
    if let Some(ref sid) = shard_id {
        results.push(chk!(
            "MergeShards",
            client
                .merge_shards()
                .stream_name("conformance-stream")
                .shard_to_merge(sid)
                .adjacent_shard_to_merge(sid)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("MergeShards".to_string()));
    }

    // SplitShard (on the single shard)
    if let Some(ref sid) = shard_id {
        results.push(chk!(
            "SplitShard",
            client
                .split_shard()
                .stream_name("conformance-stream")
                .shard_to_split(sid)
                .new_starting_hash_key("170141183460469231731687303715884105728")
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("SplitShard".to_string()));
    }

    // RegisterStreamConsumer
    let stream_arn_r = client
        .describe_stream_summary()
        .stream_name("conformance-stream")
        .send()
        .await;
    let stream_arn = stream_arn_r
        .as_ref()
        .ok()
        .and_then(|r| r.stream_description_summary.as_ref())
        .map(|s| s.stream_arn.clone())
        .unwrap_or_else(|| {
            "arn:aws:kinesis:us-east-1:000000000000:stream/conformance-stream".to_string()
        });

    let consumer_r = client
        .register_stream_consumer()
        .stream_arn(&stream_arn)
        .consumer_name("conformance-consumer")
        .send()
        .await;
    let consumer_arn = consumer_r
        .as_ref()
        .ok()
        .and_then(|r| r.consumer.as_ref())
        .map(|c| c.consumer_arn.clone());
    results.push(chk!("RegisterStreamConsumer", consumer_r, verbose));

    // ListStreamConsumers
    results.push(chk!(
        "ListStreamConsumers",
        client
            .list_stream_consumers()
            .stream_arn(&stream_arn)
            .send()
            .await,
        verbose
    ));

    // DescribeStreamConsumer
    if let Some(ref carn) = consumer_arn {
        results.push(chk!(
            "DescribeStreamConsumer",
            client
                .describe_stream_consumer()
                .consumer_arn(carn)
                .send()
                .await,
            verbose
        ));

        // DeregisterStreamConsumer
        results.push(chk!(
            "DeregisterStreamConsumer",
            client
                .deregister_stream_consumer()
                .consumer_arn(carn)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DescribeStreamConsumer".to_string()));
        results.push(OpResult::Skipped("DeregisterStreamConsumer".to_string()));
    }

    // EnableEnhancedMonitoring
    results.push(chk!(
        "EnableEnhancedMonitoring",
        client
            .enable_enhanced_monitoring()
            .stream_name("conformance-stream")
            .shard_level_metrics(aws_sdk_kinesis::types::MetricsName::IncomingBytes)
            .send()
            .await,
        verbose
    ));

    // DisableEnhancedMonitoring
    results.push(chk!(
        "DisableEnhancedMonitoring",
        client
            .disable_enhanced_monitoring()
            .stream_name("conformance-stream")
            .shard_level_metrics(aws_sdk_kinesis::types::MetricsName::IncomingBytes)
            .send()
            .await,
        verbose
    ));

    // StartStreamEncryption
    results.push(chk!(
        "StartStreamEncryption",
        client
            .start_stream_encryption()
            .stream_name("conformance-stream")
            .encryption_type(aws_sdk_kinesis::types::EncryptionType::Kms)
            .key_id("alias/aws/kinesis")
            .send()
            .await,
        verbose
    ));

    // StopStreamEncryption
    results.push(chk!(
        "StopStreamEncryption",
        client
            .stop_stream_encryption()
            .stream_name("conformance-stream")
            .encryption_type(aws_sdk_kinesis::types::EncryptionType::Kms)
            .key_id("alias/aws/kinesis")
            .send()
            .await,
        verbose
    ));

    // UpdateShardCount
    results.push(chk!(
        "UpdateShardCount",
        client
            .update_shard_count()
            .stream_name("conformance-stream")
            .target_shard_count(2)
            .scaling_type(aws_sdk_kinesis::types::ScalingType::UniformScaling)
            .send()
            .await,
        verbose
    ));

    // DescribeLimits
    results.push(chk!(
        "DescribeLimits",
        client.describe_limits().send().await,
        verbose
    ));

    // PutResourcePolicy
    results.push(chk!(
        "PutResourcePolicy",
        client
            .put_resource_policy()
            .resource_arn(&stream_arn)
            .policy(r#"{"Version":"2012-10-17","Statement":[]}"#)
            .send()
            .await,
        verbose
    ));

    // GetResourcePolicy
    results.push(chk!(
        "GetResourcePolicy",
        client
            .get_resource_policy()
            .resource_arn(&stream_arn)
            .send()
            .await,
        verbose
    ));

    // UpdateStreamMode
    results.push(chk!(
        "UpdateStreamMode",
        client
            .update_stream_mode()
            .stream_arn(&stream_arn)
            .stream_mode_details(
                aws_sdk_kinesis::types::StreamModeDetails::builder()
                    .stream_mode(aws_sdk_kinesis::types::StreamMode::OnDemand)
                    .build()
                    .unwrap()
            )
            .send()
            .await,
        verbose
    ));

    // DeleteStream
    results.push(chk!(
        "DeleteStream",
        client
            .delete_stream()
            .stream_name("conformance-stream")
            .send()
            .await,
        verbose
    ));

    results
}
