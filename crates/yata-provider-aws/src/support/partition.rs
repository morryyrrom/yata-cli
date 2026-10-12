//! Partition helpers. Only the `aws` partition is supported.

/// The only supported partition.
pub(crate) const SUPPORTED_PARTITION: &str = "aws";

/// Default region used for global services in the `aws` partition.
pub(crate) const DEFAULT_GLOBAL_REGION: &str = "us-east-1";

/// Returns the partition (the second `:`-separated element) of an ARN.
pub(crate) fn partition_of_arn(arn: &str) -> Option<&str> {
    let mut parts = arn.split(':');
    if parts.next()? != "arn" {
        return None;
    }
    parts.next().filter(|partition| !partition.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_the_partition() {
        assert_eq!(
            partition_of_arn("arn:aws:iam::123456789012:user/example"),
            Some("aws")
        );
        assert_eq!(
            partition_of_arn("arn:aws-cn:iam::123456789012:user/example"),
            Some("aws-cn")
        );
        assert_eq!(partition_of_arn("not-an-arn"), None);
        assert_eq!(partition_of_arn("arn::iam"), None);
        assert_eq!(partition_of_arn(""), None);
    }
}
