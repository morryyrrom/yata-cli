//! `x.y.z` versions of `yata` and policy sets.

use std::fmt;

/// An `x.y.z` version, compared numerically part by part.
///
/// Used for the version of the running `yata` (without pre-release or build metadata) and for
/// `min_yata_version` in policy manifests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct YataVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

/// A version string is not of the form `x.y.z`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid version {input:?}: expected x.y.z with decimal numbers")]
pub struct VersionError {
    input: String,
}

impl YataVersion {
    /// Creates a version from its three parts.
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parses a strict `x.y.z` string (ASCII digits only, each part fitting in `u64`).
    pub fn parse(version: &str) -> Result<Self, VersionError> {
        let err = || VersionError {
            input: version.to_string(),
        };
        let mut parts = version.split('.');
        let mut next = || -> Result<u64, VersionError> {
            let part = parts.next().ok_or_else(err)?;
            if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
                return Err(err());
            }
            part.parse::<u64>().map_err(|_| err())
        };
        let major = next()?;
        let minor = next()?;
        let patch = next()?;
        if parts.next().is_some() {
            return Err(err());
        }
        Ok(Self::new(major, minor, patch))
    }

    /// Parses a Cargo package version such as `0.1.0-alpha.1+build5`, ignoring the pre-release
    /// part (from `-`) and the build metadata (from `+`).
    pub fn from_package_version(version: &str) -> Result<Self, VersionError> {
        let end = version.find(['-', '+']).unwrap_or(version.len());
        Self::parse(&version[..end]).map_err(|_| VersionError {
            input: version.to_string(),
        })
    }
}

impl fmt::Display for YataVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_strict_versions() {
        assert_eq!(
            YataVersion::parse("1.2.3").unwrap(),
            YataVersion::new(1, 2, 3)
        );
        assert_eq!(
            YataVersion::parse("0.0.0").unwrap(),
            YataVersion::new(0, 0, 0)
        );
        for bad in [
            "", "1", "1.2", "1.2.3.4", "1.2.x", "1..3", "+1.2.3", "1.2.-3", " 1.2.3", "1.2.3-rc",
        ] {
            assert!(YataVersion::parse(bad).is_err(), "{bad:?}");
        }
        assert!(YataVersion::parse("1.2.99999999999999999999999").is_err());
    }

    #[test]
    fn package_version_drops_pre_release_and_build() {
        let expected = YataVersion::new(0, 1, 0);
        assert_eq!(
            YataVersion::from_package_version("0.1.0").unwrap(),
            expected
        );
        assert_eq!(
            YataVersion::from_package_version("0.1.0-alpha.1").unwrap(),
            expected
        );
        assert_eq!(
            YataVersion::from_package_version("0.1.0+build.5").unwrap(),
            expected
        );
        assert_eq!(
            YataVersion::from_package_version("0.1.0-rc.1+b").unwrap(),
            expected
        );
        assert!(YataVersion::from_package_version("0.1-alpha").is_err());
    }

    #[test]
    fn compares_numerically() {
        let v = |s| YataVersion::parse(s).unwrap();
        assert!(v("0.10.0") > v("0.9.0"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert!(v("0.1.0") <= v("0.1.0"));
    }
}
