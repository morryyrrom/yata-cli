//! Deny list and exception list of the read-only guard.
//!
//! Changes to these lists must not be mixed into feature changes: they are made in a
//! dedicated change together with the reason.
//!
//! The service names are the values of `Metadata::service()` that the generated SDK code
//! stores for each operation (the Smithy `sdkId`), not the `aws-sdk-<service>` crate names.

/// One operation listed in the deny list or the exception list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ListedOperation {
    /// Service name as reported by the SDK operation metadata, e.g. `Secrets Manager`.
    pub(crate) service: &'static str,
    /// Operation name, e.g. `GetSecretValue`.
    pub(crate) operation: &'static str,
    /// Corresponding IAM action name, e.g. `secretsmanager:GetSecretValue`.
    pub(crate) iam_action: &'static str,
    /// Why the operation is listed.
    pub(crate) reason: &'static str,
}

/// Operations that are blocked even though their names start with `Get`.
pub(crate) const DENY_LIST: &[ListedOperation] = &[
    ListedOperation {
        service: "Secrets Manager",
        operation: "GetSecretValue",
        iam_action: "secretsmanager:GetSecretValue",
        reason: "Retrieves a secret value.",
    },
    ListedOperation {
        service: "SSM",
        operation: "GetParameter",
        iam_action: "ssm:GetParameter",
        reason: "Can retrieve the value of a SecureString parameter.",
    },
    ListedOperation {
        service: "SSM",
        operation: "GetParameters",
        iam_action: "ssm:GetParameters",
        reason: "Can retrieve the value of a SecureString parameter.",
    },
    ListedOperation {
        service: "SSM",
        operation: "GetParametersByPath",
        iam_action: "ssm:GetParametersByPath",
        reason: "Can retrieve the value of a SecureString parameter.",
    },
    ListedOperation {
        service: "SSM",
        operation: "GetParameterHistory",
        iam_action: "ssm:GetParameterHistory",
        reason: "Can retrieve the value of a SecureString parameter.",
    },
    ListedOperation {
        service: "STS",
        operation: "GetSessionToken",
        iam_action: "sts:GetSessionToken",
        reason: "Issues temporary credentials.",
    },
    ListedOperation {
        service: "STS",
        operation: "GetFederationToken",
        iam_action: "sts:GetFederationToken",
        reason: "Issues temporary credentials.",
    },
    ListedOperation {
        service: "ECR",
        operation: "GetAuthorizationToken",
        iam_action: "ecr:GetAuthorizationToken",
        reason: "Issues registry credentials.",
    },
    ListedOperation {
        service: "S3",
        operation: "GetObject",
        iam_action: "s3:GetObject",
        reason: "Reads the data stored in a bucket.",
    },
];

/// Operations that are allowed although their names do not start with `Describe`, `List` or `Get`.
pub(crate) const EXCEPTION_LIST: &[ListedOperation] = &[ListedOperation {
    service: "IAM",
    operation: "GenerateCredentialReport",
    iam_action: "iam:GenerateCredentialReport",
    reason: "Only regenerates the single credential report of the account and does not create, \
             change or delete any IAM resource or setting. Required by the CIS 2.10 to 2.12 checks.",
}];
