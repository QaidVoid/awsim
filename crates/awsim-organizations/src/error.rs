//! Organizations error helpers. The Smithy model maps the `*NotFound`
//! variants to 404, conflicts to 409, and access errors to 403; AWS
//! returns those statuses verbatim.

use awsim_core::AwsError;

pub fn account_not_found(message: impl Into<String>) -> AwsError {
    AwsError::not_found("AccountNotFoundException", message)
}

pub fn organizational_unit_not_found(message: impl Into<String>) -> AwsError {
    AwsError::not_found("OrganizationalUnitNotFoundException", message)
}

pub fn policy_not_found(message: impl Into<String>) -> AwsError {
    AwsError::not_found("PolicyNotFoundException", message)
}

pub fn organizations_not_in_use(message: impl Into<String>) -> AwsError {
    AwsError::not_found("AWSOrganizationsNotInUseException", message)
}

pub fn already_in_organization(message: impl Into<String>) -> AwsError {
    AwsError::conflict("AlreadyInOrganizationException", message)
}
