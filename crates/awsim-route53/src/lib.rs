mod operations;
mod state;

use std::sync::Arc;

use async_trait::async_trait;
use awsim_core::{
    AccountRegionStore, AwsError, Protocol, RequestContext, RouteDefinition, ServiceHandler,
};
use serde_json::Value;
use tracing::debug;

use state::Route53State;

/// The AWSim Route53 service handler.
///
/// Route53 is a global service — state is stored per account under the "global" region key.
pub struct Route53Service {
    store: AccountRegionStore<Route53State>,
}

impl Route53Service {
    pub fn new() -> Self {
        Self {
            store: AccountRegionStore::new(),
        }
    }

    fn get_state(&self, ctx: &RequestContext) -> Arc<Route53State> {
        // Route53 is global — ignore region.
        self.store.get(&ctx.account_id, "global")
    }
}

impl Default for Route53Service {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ServiceHandler for Route53Service {
    fn service_name(&self) -> &str {
        "route53"
    }

    fn signing_name(&self) -> &str {
        "route53"
    }

    fn protocol(&self) -> Protocol {
        Protocol::RestXml
    }

    fn routes(&self) -> Vec<RouteDefinition> {
        vec![
            // Hosted Zones
            RouteDefinition {
                method: "POST",
                path_pattern: "/2013-04-01/hostedzone",
                operation: "CreateHostedZone",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/hostedzone",
                operation: "ListHostedZones",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/hostedzone/{Id}",
                operation: "GetHostedZone",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2013-04-01/hostedzone/{Id}",
                operation: "DeleteHostedZone",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/hostedzonesbyname",
                operation: "ListHostedZonesByName",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/hostedzonecount",
                operation: "GetHostedZoneCount",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/hostedzonesbyvpc",
                operation: "ListHostedZonesByVPC",
                required_query_param: None,
            },
            // DNSSEC
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/hostedzone/{Id}/dnssec",
                operation: "GetDNSSEC",
                required_query_param: None,
            },
            // Record Sets
            RouteDefinition {
                method: "POST",
                path_pattern: "/2013-04-01/hostedzone/{Id}/rrset",
                operation: "ChangeResourceRecordSets",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/hostedzone/{Id}/rrset",
                operation: "ListResourceRecordSets",
                required_query_param: None,
            },
            // Health Checks
            RouteDefinition {
                method: "POST",
                path_pattern: "/2013-04-01/healthcheck",
                operation: "CreateHealthCheck",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/healthcheck",
                operation: "ListHealthChecks",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2013-04-01/healthcheck/{Id}",
                operation: "DeleteHealthCheck",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/2013-04-01/healthcheck/{Id}",
                operation: "UpdateHealthCheck",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/healthcheckcount",
                operation: "GetHealthCheckCount",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/healthcheck/{Id}/status",
                operation: "GetHealthCheckStatus",
                required_query_param: None,
            },
            // DNS testing
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/testdnsanswer",
                operation: "TestDNSAnswer",
                required_query_param: None,
            },
            // Checker IP ranges
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/checkeripranges",
                operation: "GetCheckerIpRanges",
                required_query_param: None,
            },
            // Query Logging
            RouteDefinition {
                method: "POST",
                path_pattern: "/2013-04-01/queryloggingconfig",
                operation: "CreateQueryLoggingConfig",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/queryloggingconfig",
                operation: "ListQueryLoggingConfigs",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2013-04-01/queryloggingconfig/{Id}",
                operation: "DeleteQueryLoggingConfig",
                required_query_param: None,
            },
            // Tags
            RouteDefinition {
                method: "POST",
                path_pattern: "/2013-04-01/tags/{ResourceType}/{ResourceId}",
                operation: "ChangeTagsForResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/tags/{ResourceType}/{ResourceId}",
                operation: "ListTagsForResource",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/2013-04-01/tags/{ResourceType}",
                operation: "ListTagsForResources",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/change/{Id}",
                operation: "GetChange",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/healthcheck/{HealthCheckId}",
                operation: "GetHealthCheck",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/queryloggingconfig/{Id}",
                operation: "GetQueryLoggingConfig",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/geolocation",
                operation: "GetGeoLocation",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/geolocations",
                operation: "ListGeoLocations",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/delegationset",
                operation: "ListReusableDelegationSets",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/2013-04-01/delegationset",
                operation: "CreateReusableDelegationSet",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/2013-04-01/trafficpolicy",
                operation: "CreateTrafficPolicy",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/trafficpolicies",
                operation: "ListTrafficPolicies",
                required_query_param: None,
            },
            RouteDefinition {
                method: "GET",
                path_pattern: "/2013-04-01/trafficpolicy/{Id}/{Version}",
                operation: "GetTrafficPolicy",
                required_query_param: None,
            },
            RouteDefinition {
                method: "DELETE",
                path_pattern: "/2013-04-01/trafficpolicy/{Id}/{Version}",
                operation: "DeleteTrafficPolicy",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/2013-04-01/hostedzone/{Id}/associatevpc",
                operation: "AssociateVPCWithHostedZone",
                required_query_param: None,
            },
            RouteDefinition {
                method: "POST",
                path_pattern: "/2013-04-01/hostedzone/{Id}/disassociatevpc",
                operation: "DisassociateVPCFromHostedZone",
                required_query_param: None,
            },
        ]
    }

    async fn handle(
        &self,
        operation: &str,
        input: Value,
        ctx: &RequestContext,
    ) -> Result<Value, AwsError> {
        debug!(operation = %operation, "Route53 operation");
        let state = self.get_state(ctx);

        match operation {
            // Hosted Zones
            "CreateHostedZone" => operations::zones::create_hosted_zone(&state, &input, ctx),
            "GetHostedZone" => operations::zones::get_hosted_zone(&state, &input, ctx),
            "ListHostedZones" => operations::zones::list_hosted_zones(&state, &input, ctx),
            "DeleteHostedZone" => operations::zones::delete_hosted_zone(&state, &input, ctx),
            "ListHostedZonesByName" => {
                operations::zones::list_hosted_zones_by_name(&state, &input, ctx)
            }
            "GetHostedZoneCount" => operations::extra::get_hosted_zone_count(&state, &input, ctx),
            "ListHostedZonesByVPC" => {
                operations::extra::list_hosted_zones_by_vpc(&state, &input, ctx)
            }
            "GetDNSSEC" => operations::extra::get_dnssec(&state, &input, ctx),

            // Record Sets
            "ChangeResourceRecordSets" => {
                operations::records::change_resource_record_sets(&state, &input, ctx)
            }
            "ListResourceRecordSets" => {
                operations::records::list_resource_record_sets(&state, &input, ctx)
            }

            // Health Checks
            "CreateHealthCheck" => {
                operations::health_checks::create_health_check(&state, &input, ctx)
            }
            "ListHealthChecks" => {
                operations::health_checks::list_health_checks(&state, &input, ctx)
            }
            "DeleteHealthCheck" => {
                operations::health_checks::delete_health_check(&state, &input, ctx)
            }
            "GetHealthCheckCount" => {
                operations::health_checks::get_health_check_count(&state, &input, ctx)
            }
            "GetHealthCheckStatus" => {
                operations::health_checks::get_health_check_status(&state, &input, ctx)
            }
            "UpdateHealthCheck" => {
                operations::health_checks::update_health_check(&state, &input, ctx)
            }

            // DNS testing
            "TestDNSAnswer" => operations::extra::test_dns_answer(&state, &input, ctx),

            // Checker IP ranges
            "GetCheckerIpRanges" => operations::extra::get_checker_ip_ranges(&state, &input, ctx),

            // Query Logging
            "CreateQueryLoggingConfig" => {
                operations::extra::create_query_logging_config(&state, &input, ctx)
            }
            "DeleteQueryLoggingConfig" => {
                operations::extra::delete_query_logging_config(&state, &input, ctx)
            }
            "ListQueryLoggingConfigs" => {
                operations::extra::list_query_logging_configs(&state, &input, ctx)
            }

            // Tags
            "ChangeTagsForResource" => {
                operations::tags::change_tags_for_resource(&state, &input, ctx)
            }
            "ListTagsForResource" => operations::tags::list_tags_for_resource(&state, &input, ctx),
            "ListTagsForResources" => {
                operations::more::list_tags_for_resources(&state, &input, ctx)
            }

            "GetChange" => operations::more::get_change(&state, &input, ctx),
            "GetHealthCheck" => operations::more::get_health_check(&state, &input, ctx),
            "GetQueryLoggingConfig" => {
                operations::more::get_query_logging_config(&state, &input, ctx)
            }
            "GetGeoLocation" => operations::more::get_geo_location(&state, &input, ctx),
            "ListGeoLocations" => operations::more::list_geo_locations(&state, &input, ctx),
            "ListReusableDelegationSets" => {
                operations::more::list_reusable_delegation_sets(&state, &input, ctx)
            }
            "CreateReusableDelegationSet" => {
                operations::more::create_reusable_delegation_set(&state, &input, ctx)
            }
            "CreateTrafficPolicy" => operations::more::create_traffic_policy(&state, &input, ctx),
            "GetTrafficPolicy" => operations::more::get_traffic_policy(&state, &input, ctx),
            "ListTrafficPolicies" => operations::more::list_traffic_policies(&state, &input, ctx),
            "DeleteTrafficPolicy" => operations::more::delete_traffic_policy(&state, &input, ctx),
            "AssociateVPCWithHostedZone" => {
                operations::more::associate_vpc_with_hosted_zone(&state, &input, ctx)
            }
            "DisassociateVPCFromHostedZone" => {
                operations::more::disassociate_vpc_from_hosted_zone(&state, &input, ctx)
            }

            _ => Err(AwsError::unknown_operation(operation)),
        }
    }
}
