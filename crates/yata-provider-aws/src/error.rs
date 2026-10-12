//! Errors returned by the public API of the provider.
//!
//! Messages are in English and never contain credential values. Messages of
//! [`SetupError`] include the SDK error code but not the message body returned by the
//! service (the body can contain the caller's ARN); the details go to the `debug` log.

use aws_smithy_runtime_api::box_error::BoxError;

/// An error that prevents the collection from starting or continuing.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProviderError {
    /// A requested resource type is not supported by this provider.
    #[error("unsupported resource type: {0}")]
    UnsupportedResourceType(String),
    /// A region name has an invalid format, or the region list is empty.
    #[error("invalid region: {0}")]
    InvalidRegion(String),
    /// Credentials, account or region discovery failed.
    #[error(transparent)]
    Setup(#[from] SetupError),
    /// The cancel future completed before the collection finished.
    #[error("the collection was interrupted")]
    Interrupted,
}

/// An error in the preparation phase (credentials, account, regions).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SetupError {
    /// `sts:GetCallerIdentity` failed, which includes missing credentials or profile.
    #[error(
        "sts:GetCallerIdentity failed ({code}); check the credentials, the profile and the region configuration"
    )]
    CallerIdentity {
        /// SDK error code or error type.
        code: String,
        /// The SDK error.
        #[source]
        source: BoxError,
    },
    /// `sts:GetCallerIdentity` returned an account id that is not 12 digits.
    #[error("sts:GetCallerIdentity returned an account id that is not 12 digits")]
    InvalidAccountId,
    /// `sts:GetCallerIdentity` returned no ARN.
    #[error("sts:GetCallerIdentity returned no ARN")]
    MissingArn,
    /// The caller belongs to a partition other than `aws`.
    #[error("unsupported partition '{partition}': only the 'aws' partition is supported")]
    UnsupportedPartition {
        /// The partition found in the caller ARN.
        partition: String,
    },
    /// `ec2:DescribeRegions` failed.
    #[error(
        "ec2:DescribeRegions failed ({code}); specify the target regions with --region to skip this call"
    )]
    DescribeRegions {
        /// SDK error code or error type.
        code: String,
        /// The SDK error.
        #[source]
        source: BoxError,
    },
}

impl ProviderError {
    /// Returns the process exit code that `yata-cli` uses for this error.
    pub fn exit_code(&self) -> i32 {
        match self {
            ProviderError::UnsupportedResourceType(_) | ProviderError::InvalidRegion(_) => 2,
            ProviderError::Setup(_) => 3,
            ProviderError::Interrupted => 130,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_follow_the_design() {
        assert_eq!(
            ProviderError::UnsupportedResourceType("x".into()).exit_code(),
            2
        );
        assert_eq!(ProviderError::InvalidRegion("x".into()).exit_code(), 2);
        assert_eq!(
            ProviderError::Setup(SetupError::InvalidAccountId).exit_code(),
            3
        );
        assert_eq!(ProviderError::Interrupted.exit_code(), 130);
    }
}
