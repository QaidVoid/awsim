/**
 * Cross-service detector for AWS authorization-failure responses.
 *
 * Different services surface authz failures with different error
 * codes: IAM and most JSON-protocol services use
 * `AccessDeniedException`, EC2 uses `UnauthorizedOperation`, S3
 * uses `AccessDenied`, and a few stragglers use `AuthFailure`. The
 * UI cares about all of them the same way, so funnel detection
 * through one helper.
 */

const ACCESS_DENIED_CODES = [
	"AccessDenied",
	"AccessDeniedException",
	"UnauthorizedOperation",
	"AuthFailure",
	"NotAuthorizedException",
	"NotAuthorized",
	"Forbidden",
];

/**
 * Returns true when the thrown error looks like an AWS authz
 * rejection. Matches on message substring because most of the API
 * client wrappers in `$lib/api/*.ts` flatten the AWS error envelope
 * into a plain Error with the code in the message body.
 */
export function isAccessDenied(err: unknown): boolean {
	if (!err) return false;
	const msg = err instanceof Error ? err.message : String(err);
	if (!msg) return false;
	return ACCESS_DENIED_CODES.some((code) => msg.includes(code));
}

/**
 * Extract the AWS error code from an error message when possible.
 * Used to surface "Required: <code>" hints; falls back to null when
 * the message doesn't follow a recognisable shape.
 */
export function extractErrorCode(err: unknown): string | null {
	if (!err) return null;
	const msg = err instanceof Error ? err.message : String(err);
	for (const code of ACCESS_DENIED_CODES) {
		if (msg.includes(code)) return code;
	}
	return null;
}
