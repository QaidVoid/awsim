pub mod error;
mod handler;
mod operations;
mod state;

pub use handler::EcsService;

#[cfg(test)]
mod tests {
    use awsim_core::RequestContext;
    use serde_json::json;

    use super::handler::EcsService;
    use awsim_core::ServiceHandler;

    fn ctx() -> RequestContext {
        RequestContext::new("ecs", "us-east-1")
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

        fn noop_clone(_: *const ()) -> RawWaker {
            noop_raw_waker()
        }
        fn noop(_: *const ()) {}
        fn noop_raw_waker() -> RawWaker {
            static VTABLE: RawWakerVTable = RawWakerVTable::new(noop_clone, noop, noop, noop);
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        let waker = unsafe { Waker::from_raw(noop_raw_waker()) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = std::pin::pin!(f);
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                Poll::Pending => {}
            }
        }
    }

    // -----------------------------------------------------------------------
    // Clusters
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_cluster() {
        let svc = EcsService::new();
        let ctx = ctx();
        let result = block_on(svc.handle(
            "CreateCluster",
            json!({ "clusterName": "my-cluster" }),
            &ctx,
        ))
        .unwrap();
        let arn = result["cluster"]["clusterArn"].as_str().unwrap();
        assert!(arn.contains("my-cluster"), "arn={arn}");
    }

    #[test]
    fn test_create_cluster_idempotent() {
        let svc = EcsService::new();
        let ctx = ctx();
        let r1 =
            block_on(svc.handle("CreateCluster", json!({ "clusterName": "idem" }), &ctx)).unwrap();
        let r2 =
            block_on(svc.handle("CreateCluster", json!({ "clusterName": "idem" }), &ctx)).unwrap();
        assert_eq!(r1["cluster"]["clusterArn"], r2["cluster"]["clusterArn"]);
    }

    #[test]
    fn test_list_clusters() {
        let svc = EcsService::new();
        let ctx = ctx();
        block_on(svc.handle("CreateCluster", json!({ "clusterName": "c1" }), &ctx)).unwrap();
        block_on(svc.handle("CreateCluster", json!({ "clusterName": "c2" }), &ctx)).unwrap();
        let result = block_on(svc.handle("ListClusters", json!({}), &ctx)).unwrap();
        assert_eq!(result["clusterArns"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_describe_clusters() {
        let svc = EcsService::new();
        let ctx = ctx();
        block_on(svc.handle("CreateCluster", json!({ "clusterName": "dc" }), &ctx)).unwrap();
        let result =
            block_on(svc.handle("DescribeClusters", json!({ "clusters": ["dc"] }), &ctx)).unwrap();
        assert_eq!(result["clusters"].as_array().unwrap().len(), 1);
        assert_eq!(result["failures"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_describe_clusters_missing() {
        let svc = EcsService::new();
        let ctx = ctx();
        let result =
            block_on(svc.handle("DescribeClusters", json!({ "clusters": ["ghost"] }), &ctx))
                .unwrap();
        assert_eq!(result["clusters"].as_array().unwrap().len(), 0);
        assert_eq!(result["failures"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_delete_cluster() {
        let svc = EcsService::new();
        let ctx = ctx();
        block_on(svc.handle("CreateCluster", json!({ "clusterName": "todelete" }), &ctx)).unwrap();
        block_on(svc.handle("DeleteCluster", json!({ "cluster": "todelete" }), &ctx)).unwrap();
        let list = block_on(svc.handle("ListClusters", json!({}), &ctx)).unwrap();
        assert_eq!(list["clusterArns"].as_array().unwrap().len(), 0);
    }

    // -----------------------------------------------------------------------
    // Task Definitions
    // -----------------------------------------------------------------------

    #[test]
    fn test_register_task_definition() {
        let svc = EcsService::new();
        let ctx = ctx();
        let result = block_on(svc.handle(
            "RegisterTaskDefinition",
            json!({
                "family": "web",
                "containerDefinitions": [
                    { "name": "nginx", "image": "nginx:latest", "cpu": 256, "memory": 512, "essential": true }
                ],
                "networkMode": "awsvpc",
                "requiresCompatibilities": ["FARGATE"],
                "cpu": "256",
                "memory": "512",
            }),
            &ctx,
        ))
        .unwrap();
        let td = &result["taskDefinition"];
        assert_eq!(td["family"].as_str().unwrap(), "web");
        assert_eq!(td["revision"].as_u64().unwrap(), 1);
        let arn = td["taskDefinitionArn"].as_str().unwrap();
        assert!(arn.contains("web:1"), "arn={arn}");
    }

    #[test]
    fn test_register_task_definition_multiple_revisions() {
        let svc = EcsService::new();
        let ctx = ctx();
        let r1 = block_on(svc.handle(
            "RegisterTaskDefinition",
            json!({ "family": "worker", "containerDefinitions": [] }),
            &ctx,
        ))
        .unwrap();
        let r2 = block_on(svc.handle(
            "RegisterTaskDefinition",
            json!({ "family": "worker", "containerDefinitions": [] }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(r1["taskDefinition"]["revision"].as_u64().unwrap(), 1);
        assert_eq!(r2["taskDefinition"]["revision"].as_u64().unwrap(), 2);
    }

    #[test]
    fn test_describe_task_definition() {
        let svc = EcsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "RegisterTaskDefinition",
            json!({ "family": "api", "containerDefinitions": [] }),
            &ctx,
        ))
        .unwrap();
        let result = block_on(svc.handle(
            "DescribeTaskDefinition",
            json!({ "taskDefinition": "api:1" }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["taskDefinition"]["family"].as_str().unwrap(), "api");
    }

    #[test]
    fn test_list_task_definitions() {
        let svc = EcsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "RegisterTaskDefinition",
            json!({ "family": "svc-a", "containerDefinitions": [] }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "RegisterTaskDefinition",
            json!({ "family": "svc-b", "containerDefinitions": [] }),
            &ctx,
        ))
        .unwrap();
        let result = block_on(svc.handle("ListTaskDefinitions", json!({}), &ctx)).unwrap();
        assert_eq!(result["taskDefinitionArns"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_deregister_task_definition() {
        let svc = EcsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "RegisterTaskDefinition",
            json!({ "family": "old-svc", "containerDefinitions": [] }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "DeregisterTaskDefinition",
            json!({ "taskDefinition": "old-svc:1" }),
            &ctx,
        ))
        .unwrap();
        let result = block_on(svc.handle("ListTaskDefinitions", json!({}), &ctx)).unwrap();
        assert_eq!(result["taskDefinitionArns"].as_array().unwrap().len(), 0);
    }

    // -----------------------------------------------------------------------
    // Services
    // -----------------------------------------------------------------------

    #[test]
    fn test_create_and_list_service() {
        let svc = EcsService::new();
        let ctx = ctx();
        block_on(svc.handle("CreateCluster", json!({ "clusterName": "prod" }), &ctx)).unwrap();
        block_on(svc.handle(
            "CreateService",
            json!({
                "cluster": "prod",
                "serviceName": "web-svc",
                "taskDefinition": "web:1",
                "desiredCount": 2,
                "launchType": "FARGATE"
            }),
            &ctx,
        ))
        .unwrap();
        let list =
            block_on(svc.handle("ListServices", json!({ "cluster": "prod" }), &ctx)).unwrap();
        assert_eq!(list["serviceArns"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_create_service_round_trips_load_balancers_and_deployment_config() {
        let svc = EcsService::new();
        let ctx = ctx();
        block_on(svc.handle("CreateCluster", json!({ "clusterName": "deploy" }), &ctx)).unwrap();
        let create = block_on(svc.handle(
            "CreateService",
            json!({
                "cluster": "deploy",
                "serviceName": "edge",
                "taskDefinition": "edge:1",
                "desiredCount": 3,
                "launchType": "FARGATE",
                "loadBalancers": [{
                    "targetGroupArn": "arn:aws:elasticloadbalancing:us-east-1:000000000000:targetgroup/edge/abc",
                    "containerName": "edge",
                    "containerPort": 8080
                }],
                "deploymentConfiguration": {
                    "minimumHealthyPercent": 50,
                    "maximumPercent": 200
                },
                "deploymentController": { "type": "CODE_DEPLOY" },
                "networkConfiguration": {
                    "awsvpcConfiguration": {
                        "subnets": ["subnet-1"],
                        "assignPublicIp": "ENABLED"
                    }
                }
            }),
            &ctx,
        ))
        .unwrap();
        let s = &create["service"];
        assert_eq!(s["loadBalancers"][0]["containerName"], "edge");
        assert_eq!(s["deploymentConfiguration"]["maximumPercent"], 200);
        assert_eq!(s["deploymentController"]["type"], "CODE_DEPLOY");
        assert_eq!(
            s["networkConfiguration"]["awsvpcConfiguration"]["assignPublicIp"],
            "ENABLED"
        );
    }

    #[test]
    fn test_create_service_rejects_invalid_deployment_controller_type() {
        let svc = EcsService::new();
        let ctx = ctx();
        block_on(svc.handle("CreateCluster", json!({ "clusterName": "bad-dc" }), &ctx)).unwrap();
        let err = block_on(svc.handle(
            "CreateService",
            json!({
                "cluster": "bad-dc",
                "serviceName": "x",
                "taskDefinition": "x:1",
                "deploymentController": { "type": "MAGIC" }
            }),
            &ctx,
        ))
        .unwrap_err();
        assert_eq!(err.code, "InvalidParameterException");
    }

    #[test]
    fn test_describe_services() {
        let svc = EcsService::new();
        let ctx = ctx();
        block_on(svc.handle("CreateCluster", json!({ "clusterName": "staging" }), &ctx)).unwrap();
        block_on(svc.handle(
            "CreateService",
            json!({
                "cluster": "staging",
                "serviceName": "api-svc",
                "taskDefinition": "api:1",
                "desiredCount": 1,
            }),
            &ctx,
        ))
        .unwrap();
        let result = block_on(svc.handle(
            "DescribeServices",
            json!({ "cluster": "staging", "services": ["api-svc"] }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["services"].as_array().unwrap().len(), 1);
        assert_eq!(result["failures"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_update_service() {
        let svc = EcsService::new();
        let ctx = ctx();
        block_on(svc.handle("CreateCluster", json!({ "clusterName": "test" }), &ctx)).unwrap();
        block_on(svc.handle(
            "CreateService",
            json!({ "cluster": "test", "serviceName": "mysvc", "taskDefinition": "foo:1", "desiredCount": 1 }),
            &ctx,
        ))
        .unwrap();
        let result = block_on(svc.handle(
            "UpdateService",
            json!({ "cluster": "test", "service": "mysvc", "desiredCount": 5 }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["service"]["desiredCount"].as_i64().unwrap(), 5);
    }

    #[test]
    fn test_delete_service() {
        let svc = EcsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateCluster",
            json!({ "clusterName": "del-cluster" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "CreateService",
            json!({ "cluster": "del-cluster", "serviceName": "del-svc", "taskDefinition": "t:1", "desiredCount": 0 }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "DeleteService",
            json!({ "cluster": "del-cluster", "service": "del-svc" }),
            &ctx,
        ))
        .unwrap();
        let list = block_on(svc.handle("ListServices", json!({ "cluster": "del-cluster" }), &ctx))
            .unwrap();
        assert_eq!(list["serviceArns"].as_array().unwrap().len(), 0);
    }

    // -----------------------------------------------------------------------
    // Tasks
    // -----------------------------------------------------------------------

    #[test]
    fn test_run_task() {
        let svc = EcsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateCluster",
            json!({ "clusterName": "run-cluster" }),
            &ctx,
        ))
        .unwrap();
        let result = block_on(svc.handle(
            "RunTask",
            json!({ "cluster": "run-cluster", "taskDefinition": "web:1", "count": 2 }),
            &ctx,
        ))
        .unwrap();
        let tasks = result["tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0]["lastStatus"].as_str().unwrap(), "RUNNING");
    }

    #[test]
    fn test_stop_task() {
        let svc = EcsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateCluster",
            json!({ "clusterName": "stop-cluster" }),
            &ctx,
        ))
        .unwrap();
        let run = block_on(svc.handle(
            "RunTask",
            json!({ "cluster": "stop-cluster", "taskDefinition": "web:1", "count": 1 }),
            &ctx,
        ))
        .unwrap();
        let task_arn = run["tasks"][0]["taskArn"].as_str().unwrap().to_string();
        let result = block_on(svc.handle(
            "StopTask",
            json!({ "cluster": "stop-cluster", "task": task_arn }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["task"]["lastStatus"].as_str().unwrap(), "STOPPED");
    }

    #[test]
    fn test_list_tasks() {
        let svc = EcsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateCluster",
            json!({ "clusterName": "list-cluster" }),
            &ctx,
        ))
        .unwrap();
        block_on(svc.handle(
            "RunTask",
            json!({ "cluster": "list-cluster", "taskDefinition": "web:1", "count": 3 }),
            &ctx,
        ))
        .unwrap();
        let result =
            block_on(svc.handle("ListTasks", json!({ "cluster": "list-cluster" }), &ctx)).unwrap();
        assert_eq!(result["taskArns"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn test_describe_tasks() {
        let svc = EcsService::new();
        let ctx = ctx();
        block_on(svc.handle(
            "CreateCluster",
            json!({ "clusterName": "desc-cluster" }),
            &ctx,
        ))
        .unwrap();
        let run = block_on(svc.handle(
            "RunTask",
            json!({ "cluster": "desc-cluster", "taskDefinition": "web:1", "count": 1 }),
            &ctx,
        ))
        .unwrap();
        let task_arn = run["tasks"][0]["taskArn"].as_str().unwrap().to_string();
        let result = block_on(svc.handle(
            "DescribeTasks",
            json!({ "cluster": "desc-cluster", "tasks": [task_arn] }),
            &ctx,
        ))
        .unwrap();
        assert_eq!(result["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(result["failures"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn test_unknown_operation() {
        let svc = EcsService::new();
        let ctx = ctx();
        let err = block_on(svc.handle("NoSuchOp", json!({}), &ctx)).unwrap_err();
        assert_eq!(err.code, "UnknownOperationException");
    }
}
