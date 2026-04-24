use crate::chk;
use crate::runner::common::*;

pub async fn test_scheduler(endpoint: &str, verbose: bool) -> Vec<OpResult> {
    let config = make_config(endpoint).await;
    let client = aws_sdk_scheduler::Client::new(&config);
    let mut results = Vec::new();

    // CreateScheduleGroup
    let create_grp_r = client
        .create_schedule_group()
        .name("conformance-group")
        .send()
        .await;
    results.push(chk!("CreateScheduleGroup", create_grp_r, verbose));

    // ListScheduleGroups
    results.push(chk!(
        "ListScheduleGroups",
        client.list_schedule_groups().send().await,
        verbose
    ));

    // CreateSchedule
    let create_sched_r = client
        .create_schedule()
        .name("conformance-schedule")
        .schedule_expression("rate(1 minute)")
        .flexible_time_window(
            aws_sdk_scheduler::types::FlexibleTimeWindow::builder()
                .mode(aws_sdk_scheduler::types::FlexibleTimeWindowMode::Off)
                .build()
                .unwrap(),
        )
        .target(
            aws_sdk_scheduler::types::Target::builder()
                .arn("arn:aws:lambda:us-east-1:000000000000:function:conformance")
                .role_arn("arn:aws:iam::000000000000:role/scheduler-role")
                .build()
                .unwrap(),
        )
        .send()
        .await;
    results.push(chk!("CreateSchedule", create_sched_r, verbose));

    // ListSchedules
    results.push(chk!(
        "ListSchedules",
        client.list_schedules().send().await,
        verbose
    ));

    // GetSchedule
    results.push(chk!(
        "GetSchedule",
        client
            .get_schedule()
            .name("conformance-schedule")
            .send()
            .await,
        verbose
    ));

    // UpdateSchedule
    results.push(chk!(
        "UpdateSchedule",
        client
            .update_schedule()
            .name("conformance-schedule")
            .schedule_expression("rate(5 minutes)")
            .flexible_time_window(
                aws_sdk_scheduler::types::FlexibleTimeWindow::builder()
                    .mode(aws_sdk_scheduler::types::FlexibleTimeWindowMode::Off)
                    .build()
                    .unwrap(),
            )
            .target(
                aws_sdk_scheduler::types::Target::builder()
                    .arn("arn:aws:lambda:us-east-1:000000000000:function:conformance")
                    .role_arn("arn:aws:iam::000000000000:role/scheduler-role")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // GetScheduleGroup
    results.push(chk!(
        "GetScheduleGroup",
        client
            .get_schedule_group()
            .name("conformance-group")
            .send()
            .await,
        verbose
    ));

    // TagResource (on schedule)
    let sched_arn = format!(
        "arn:aws:scheduler:us-east-1:000000000000:schedule/default/conformance-schedule"
    );
    results.push(chk!(
        "TagResource",
        client
            .tag_resource()
            .resource_arn(&sched_arn)
            .tags(
                aws_sdk_scheduler::types::Tag::builder()
                    .key("env")
                    .value("conformance")
                    .build()
                    .unwrap(),
            )
            .send()
            .await,
        verbose
    ));

    // ListTagsForResource
    results.push(chk!(
        "ListTagsForResource",
        client
            .list_tags_for_resource()
            .resource_arn(&sched_arn)
            .send()
            .await,
        verbose
    ));

    // UntagResource
    results.push(chk!(
        "UntagResource",
        client
            .untag_resource()
            .resource_arn(&sched_arn)
            .tag_keys("env")
            .send()
            .await,
        verbose
    ));

    // DeleteSchedule
    results.push(chk!(
        "DeleteSchedule",
        client
            .delete_schedule()
            .name("conformance-schedule")
            .send()
            .await,
        verbose
    ));

    // DeleteScheduleGroup
    results.push(chk!(
        "DeleteScheduleGroup",
        client
            .delete_schedule_group()
            .name("conformance-group")
            .send()
            .await,
        verbose
    ));

    results
}
