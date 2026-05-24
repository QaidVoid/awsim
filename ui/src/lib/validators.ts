/**
 * Client-side validators mirroring the server-side AWS-spec
 * checks. These exist so the forms can show inline feedback
 * before the user clicks the button; the server still enforces
 * the same rules independently.
 *
 * Each function returns `null` on success or a short
 * human-readable error message on failure. The wording matches
 * the AWS docs vocabulary so users can search for the constraint
 * easily.
 */

/**
 * ECR repository name. AWS-documented constraint: 2-256
 * characters, slash-separated path segments where each segment
 * starts with a lowercase letter or digit and otherwise contains
 * lowercase letters, digits, and the separators dot, underscore,
 * and hyphen. Uppercase letters are rejected.
 */
export function validateEcrRepositoryName(name: string): string | null {
	if (name.length < 2 || name.length > 256) {
		return "Repository name must be 2-256 characters.";
	}
	for (const segment of name.split("/")) {
		if (segment.length === 0) {
			return "Repository name segments must not be empty.";
		}
		if (!/^[a-z0-9]/.test(segment)) {
			return "Each segment must start with a lowercase letter or digit.";
		}
		if (!/^[a-z0-9]+(?:[._-][a-z0-9]+)*$/.test(segment)) {
			return "Use lowercase letters, digits, and . _ - separators only.";
		}
	}
	return null;
}

/**
 * EKS cluster name. AWS-documented constraint: 1-100
 * characters, must start with an ASCII letter or digit, and the
 * remaining characters are letters, digits, hyphens, or
 * underscores.
 */
export function validateEksClusterName(name: string): string | null {
	if (name.length === 0 || name.length > 100) {
		return "Cluster name must be 1-100 characters.";
	}
	if (!/^[0-9A-Za-z][A-Za-z0-9\-_]*$/.test(name)) {
		return "Use letters, digits, hyphens, and underscores; must start with a letter or digit.";
	}
	return null;
}

/**
 * RDS DBInstanceIdentifier: 1-63 characters, starts with a
 * letter, only letters / digits / hyphens, no consecutive
 * hyphens, no trailing hyphen.
 */
export function validateRdsDbIdentifier(name: string): string | null {
	if (name.length === 0 || name.length > 63) {
		return "Identifier must be 1-63 characters.";
	}
	if (!/^[A-Za-z]/.test(name)) {
		return "Must start with a letter.";
	}
	if (name.endsWith("-")) {
		return "Must not end with a hyphen.";
	}
	if (name.includes("--")) {
		return "Must not contain consecutive hyphens.";
	}
	if (!/^[A-Za-z0-9-]+$/.test(name)) {
		return "Use only letters, digits, and hyphens.";
	}
	return null;
}

/**
 * IAM role ARN shape used by EKS / Lambda / ECS form fields.
 * Not exhaustive (real ARN parsing lives server-side via
 * awsim_core::arn::parse), just enough to catch common typos.
 */
export function validateIamRoleArn(arn: string): string | null {
	if (arn.length === 0) {
		return "Role ARN is required.";
	}
	if (!/^arn:[a-z-]+:iam::\d{12}:role\/[A-Za-z0-9+=,.@_/-]+$/.test(arn)) {
		return "Expected arn:aws:iam::<account>:role/<RoleName>.";
	}
	return null;
}
