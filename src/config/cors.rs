use std::str::FromStr;

use axum::http::HeaderValue;
use url::Url;

/// Which origins the admin listener lets browsers call it from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CorsOrigins {
    /// Any origin, written `*`
    Any,
    /// Only these origins, normalized to how browsers send them. Empty means
    /// no cross-origin access at all.
    Only(Vec<HeaderValue>),
}

impl FromStr for CorsOrigins {
    type Err = String;

    /// Either `*`, or a comma-separated list of origins such as
    /// `http://localhost:5173,https://admin.example.com`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let entries: Vec<&str> = s
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .collect();

        if entries.contains(&"*") {
            return match entries.len() {
                1 => Ok(Self::Any),
                _ => Err("`*` can't be combined with other origins".to_owned()),
            };
        }

        let mut origins = Vec::with_capacity(entries.len());
        for entry in entries {
            let url = Url::parse(entry).map_err(|e| format!("invalid origin `{entry}`: {e}"))?;

            // Browsers send the scheme, host and port and nothing else, so
            // anything more specific than that could never match.
            let is_bare = url.origin().is_tuple()
                && url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none()
                && url.username().is_empty()
                && url.password().is_none();
            if !is_bare {
                return Err(format!(
                    "invalid origin `{entry}`: expected scheme://host[:port] with no path"
                ));
            }

            // Serializing from the parsed form drops default ports and
            // lowercases the host, as browsers do.
            let origin = url.origin().ascii_serialization();
            let value = HeaderValue::from_str(&origin)
                .map_err(|e| format!("invalid origin `{entry}`: {e}"))?;
            if !origins.contains(&value) {
                origins.push(value);
            }
        }

        Ok(Self::Only(origins))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only(origins: &[&str]) -> CorsOrigins {
        CorsOrigins::Only(
            origins
                .iter()
                .map(|origin| HeaderValue::from_str(origin).unwrap())
                .collect(),
        )
    }

    #[test]
    fn star_means_any_origin() {
        assert_eq!("*".parse(), Ok(CorsOrigins::Any));
        assert_eq!(" * ".parse(), Ok(CorsOrigins::Any));
    }

    #[test]
    fn star_cannot_be_combined() {
        assert!("*,http://localhost:5173".parse::<CorsOrigins>().is_err());
    }

    #[test]
    fn empty_means_no_origins() {
        assert_eq!("".parse(), Ok(only(&[])));
        assert_eq!(" , ,".parse(), Ok(only(&[])));
    }

    #[test]
    fn origins_are_normalized_like_browsers_send_them() {
        assert_eq!(
            "HTTP://LocalHost:5173, https://Admin.Example.com:443".parse(),
            Ok(only(&[
                "http://localhost:5173",
                "https://admin.example.com"
            ]))
        );
    }

    #[test]
    fn duplicates_are_dropped() {
        assert_eq!(
            "http://a.test,http://a.test:80".parse(),
            Ok(only(&["http://a.test"]))
        );
    }

    #[test]
    fn anything_more_than_an_origin_is_rejected() {
        for s in [
            "http://a.test/path",
            "http://a.test?q=1",
            "http://a.test#frag",
            "http://user@a.test",
            "a.test",
            "null",
            "file:///tmp",
        ] {
            assert!(s.parse::<CorsOrigins>().is_err(), "{s:?}");
        }
    }
}
