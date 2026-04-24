use crate::chk;
use crate::runner::common::*;

pub async fn test_sqs(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_sqs::Client::new(&config);
    let mut results = Vec::new();

    // CreateQueue
    let create_r = client
        .create_queue()
        .queue_name("conformance-queue")
        .send()
        .await;
    let queue_url = create_r
        .as_ref()
        .ok()
        .and_then(|r| r.queue_url.clone())
        .unwrap_or_else(|| {
            format!(
                "{}/000000000000/conformance-queue",
                endpoint.replace("http://", "http://sqs.us-east-1.")
            )
        });
    results.push(chk!("CreateQueue", create_r, verbose));

    // ListQueues
    results.push(chk!(
        "ListQueues",
        client.list_queues().send().await,
        verbose
    ));

    // GetQueueUrl
    results.push(chk!(
        "GetQueueUrl",
        client
            .get_queue_url()
            .queue_name("conformance-queue")
            .send()
            .await,
        verbose
    ));

    // GetQueueAttributes
    results.push(chk!(
        "GetQueueAttributes",
        client
            .get_queue_attributes()
            .queue_url(&queue_url)
            .send()
            .await,
        verbose
    ));

    // SendMessage
    let send_r = client
        .send_message()
        .queue_url(&queue_url)
        .message_body("conformance test message")
        .send()
        .await;
    results.push(chk!("SendMessage", send_r, verbose));

    // ReceiveMessage
    let recv_r = client
        .receive_message()
        .queue_url(&queue_url)
        .max_number_of_messages(1)
        .send()
        .await;
    let receipt_handle = recv_r
        .as_ref()
        .ok()
        .and_then(|r| r.messages.as_ref())
        .and_then(|m| m.first())
        .and_then(|m| m.receipt_handle.clone());
    results.push(chk!("ReceiveMessage", recv_r, verbose));

    // ChangeMessageVisibility (use receipt handle if available)
    if let Some(ref handle) = receipt_handle {
        results.push(chk!(
            "ChangeMessageVisibility",
            client
                .change_message_visibility()
                .queue_url(&queue_url)
                .receipt_handle(handle)
                .visibility_timeout(30)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("ChangeMessageVisibility".to_string()));
    }

    // SendMessageBatch
    results.push(chk!(
        "SendMessageBatch",
        client
            .send_message_batch()
            .queue_url(&queue_url)
            .entries(
                aws_sdk_sqs::types::SendMessageBatchRequestEntry::builder()
                    .id("msg-1")
                    .message_body("batch message 1")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // DeleteMessage
    if let Some(ref handle) = receipt_handle {
        results.push(chk!(
            "DeleteMessage",
            client
                .delete_message()
                .queue_url(&queue_url)
                .receipt_handle(handle)
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteMessage".to_string()));
    }

    // DeleteMessageBatch — receive a fresh message first
    let recv2 = client
        .receive_message()
        .queue_url(&queue_url)
        .max_number_of_messages(1)
        .send()
        .await;
    let handle2 = recv2
        .as_ref()
        .ok()
        .and_then(|r| r.messages.as_ref())
        .and_then(|m| m.first())
        .and_then(|m| m.receipt_handle.clone());
    if let Some(h) = handle2 {
        results.push(chk!(
            "DeleteMessageBatch",
            client
                .delete_message_batch()
                .queue_url(&queue_url)
                .entries(
                    aws_sdk_sqs::types::DeleteMessageBatchRequestEntry::builder()
                        .id("del-1")
                        .receipt_handle(h)
                        .build()
                        .unwrap(),
                )
                .send()
                .await,
            verbose
        ));
    } else {
        results.push(OpResult::Skipped("DeleteMessageBatch".to_string()));
    }

    // PurgeQueue
    results.push(chk!(
        "PurgeQueue",
        client.purge_queue().queue_url(&queue_url).send().await,
        verbose
    ));

    // SetQueueAttributes
    results.push(chk!(
        "SetQueueAttributes",
        client
            .set_queue_attributes()
            .queue_url(&queue_url)
            .attributes(
                aws_sdk_sqs::types::QueueAttributeName::MessageRetentionPeriod,
                "86400",
            )
            .send()
            .await,
        verbose
    ));

    // TagQueue
    results.push(chk!(
        "TagQueue",
        client
            .tag_queue()
            .queue_url(&queue_url)
            .tags("env", "conformance")
            .send()
            .await,
        verbose
    ));

    // ListQueueTags
    results.push(chk!(
        "ListQueueTags",
        client.list_queue_tags().queue_url(&queue_url).send().await,
        verbose
    ));

    // UntagQueue
    results.push(chk!(
        "UntagQueue",
        client
            .untag_queue()
            .queue_url(&queue_url)
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // ListDeadLetterSourceQueues
    results.push(chk!(
        "ListDeadLetterSourceQueues",
        client
            .list_dead_letter_source_queues()
            .queue_url(&queue_url)
            .send()
            .await,
        verbose
    ));

    // ListMessageMoveTasks
    let dlq_arn = format!(
        "arn:aws:sqs:us-east-1:000000000000:{}",
        "conformance-queue"
    );
    results.push(chk!(
        "ListMessageMoveTasks",
        client
            .list_message_move_tasks()
            .source_arn(&dlq_arn)
            .send()
            .await,
        verbose
    ));

    // AddPermission
    results.push(chk!(
        "AddPermission",
        client
            .add_permission()
            .queue_url(&queue_url)
            .label("conformance-perm")
            .aws_account_ids("000000000000")
            .actions("SendMessage")
            .send()
            .await,
        verbose
    ));

    // RemovePermission
    results.push(chk!(
        "RemovePermission",
        client
            .remove_permission()
            .queue_url(&queue_url)
            .label("conformance-perm")
            .send()
            .await,
        verbose
    ));

    // DeleteQueue
    results.push(chk!(
        "DeleteQueue",
        client.delete_queue().queue_url(&queue_url).send().await,
        verbose
    ));

    results
}
