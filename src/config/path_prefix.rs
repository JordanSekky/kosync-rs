use std::{fmt, str::FromStr};

/// A URL path every admin route is served under, so that the admin API can
/// share a host and port with other things behind a reverse proxy.
///
/// Stored normalized: either empty, meaning no prefix, or a leading slash
/// followed by segments and no trailing slash, such as `/admin/api`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathPrefix(String);

impl PathPrefix {
    /// The prefix without a trailing slash, or "" when there is none.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Display for PathPrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_empty() { "/" } else { &self.0 })
    }
}

impl FromStr for PathPrefix {
    type Err = String;

    /// Accepts `api`, `/api`, `/api/` and `/admin/api`; empty and `/` mean no
    /// prefix.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim().trim_matches('/');
        if trimmed.is_empty() {
            return Ok(Self::default());
        }

        for segment in trimmed.split('/') {
            if segment.is_empty() {
                return Err(format!("invalid path prefix `{s}`: empty path segment"));
            }
            if matches!(segment, "." | "..") {
                return Err(format!("invalid path prefix `{s}`: `{segment}` segment"));
            }
            // Kept to characters that mean the same thing literally, in a
            // route pattern and in a URL, so the prefix matches what the
            // proxy forwards byte for byte.
            let valid = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~');
            if let Some(c) = segment.chars().find(|&c| !valid(c)) {
                return Err(format!(
                    "invalid path prefix `{s}`: unsupported character `{c}`"
                ));
            }
        }

        Ok(Self(format!("/{trimmed}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> PathPrefix {
        s.parse().unwrap()
    }

    #[test]
    fn empty_and_slash_mean_no_prefix() {
        for s in ["", "/", "  ", "//"] {
            assert!(parse(s).is_empty(), "{s:?}");
            assert_eq!(parse(s).as_str(), "");
        }
    }

    #[test]
    fn is_normalized_to_a_leading_slash_and_no_trailing_one() {
        for s in ["api", "/api", "/api/", " /api "] {
            assert_eq!(parse(s).as_str(), "/api", "{s:?}");
        }
        assert_eq!(parse("admin/api").as_str(), "/admin/api");
    }

    #[test]
    fn rejects_empty_and_dot_segments() {
        for s in ["/a//b", "/a/./b", "/../a", "/a/.."] {
            assert!(s.parse::<PathPrefix>().is_err(), "{s:?}");
        }
    }

    #[test]
    fn rejects_characters_that_mean_something_in_a_route_or_url() {
        for s in ["/a b", "/{id}", "/a?b", "/a#b", "/*", "/a:b", "/%41"] {
            assert!(s.parse::<PathPrefix>().is_err(), "{s:?}");
        }
    }

    #[test]
    fn displays_the_root_when_empty() {
        assert_eq!(PathPrefix::default().to_string(), "/");
        assert_eq!(parse("/api").to_string(), "/api");
    }
}
