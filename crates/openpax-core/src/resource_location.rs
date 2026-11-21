use std::fmt;
use std::str::FromStr;

use crate::Error;

/// A `namespace:path` identifier, e.g. `minecraft:block/stone`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResourceLocation {
    namespace: String,
    path: String,
}

impl ResourceLocation {
    pub const DEFAULT_NAMESPACE: &'static str = "minecraft";

    pub fn new(namespace: &str, path: &str) -> Result<Self, Error> {
        if Self::is_valid_namespace(namespace) && Self::is_valid_path(path) {
            Ok(Self {
                namespace: namespace.to_owned(),
                path: path.to_owned(),
            })
        } else {
            Err(Error::InvalidLocation(format!("{namespace}:{path}")))
        }
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn is_valid_namespace(s: &str) -> bool {
        !s.is_empty()
            && s.bytes()
                .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.'))
    }

    pub fn is_valid_path(s: &str) -> bool {
        !s.is_empty()
            && !s.starts_with('/')
            && !s.ends_with('/')
            && !s.contains("//")
            && s.bytes()
                .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.' | b'/'))
    }
}

impl FromStr for ResourceLocation {
    type Err = Error;

    /// Parses `namespace:path`; a missing namespace defaults to `minecraft`.
    fn from_str(s: &str) -> Result<Self, Error> {
        match s.split_once(':') {
            Some((namespace, path)) => Self::new(namespace, path),
            None => Self::new(Self::DEFAULT_NAMESPACE, s),
        }
    }
}

impl fmt::Display for ResourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_with_and_without_namespace() {
        let a: ResourceLocation = "block/stone".parse().unwrap();
        assert_eq!(a.namespace(), "minecraft");
        assert_eq!(a.path(), "block/stone");
        let b: ResourceLocation = "my_pack:item/ruby".parse().unwrap();
        assert_eq!(b.to_string(), "my_pack:item/ruby");
    }

    #[test]
    fn rejects_invalid() {
        for bad in [
            "",
            "Minecraft:stone",
            "a:B",
            "a:",
            ":x",
            "a:x//y",
            "a:/x",
            "a:x y",
            "a:b:c",
        ] {
            assert!(
                bad.parse::<ResourceLocation>().is_err(),
                "{bad:?} should be rejected"
            );
        }
    }
}
