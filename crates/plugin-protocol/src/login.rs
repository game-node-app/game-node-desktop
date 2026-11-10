//! Trusted packaged login declarations. Runtime RPCs contain only scoped window
//! handles and operations; they cannot upload or broaden this policy.

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginPolicy {
    pub start_url: String,
    /// Where the host may read the declared cookies. Empty for probe-only
    /// policies such as an authorization-code flow. Several URLs allow a
    /// session spread over more than one Steam host.
    #[serde(default)]
    pub cookie_urls: Vec<String>,
    pub probe_origin: String,
    pub title: String,
    pub allowed_hosts: Vec<String>,
    pub cookie_names: Vec<String>,
    pub probe_script: String,
    pub width: u32,
    pub height: u32,
    pub timeout_seconds: u64,
}

impl LoginPolicy {
    pub fn allows(&self, url: &Url) -> bool {
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.host_str().is_some_and(|host| {
                self.allowed_hosts.iter().any(|suffix| {
                    host == suffix
                        || host
                            .strip_suffix(suffix)
                            .is_some_and(|prefix| prefix.ends_with('.'))
                })
            })
    }

    pub fn validate(&self) -> Result<(), crate::WireError> {
        let bad = crate::WireError;
        if self.allowed_hosts.is_empty()
            || self.allowed_hosts.len() > 16
            || self.allowed_hosts.iter().any(|host| {
                host.is_empty()
                    || host.len() > 253
                    || host.starts_with('.')
                    || host.ends_with('.')
                    || !host.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.'
                    })
            })
            // A policy either reads cookies from at least one URL or reads no cookies at all.
            || self.cookie_urls.is_empty() != self.cookie_names.is_empty()
            || self.cookie_urls.len() > 4
            || self.cookie_names.len() > 16
            || self.cookie_names.iter().any(|name| {
                name.is_empty()
                    || name.len() > 128
                    || !name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            })
        {
            return Err(bad);
        }
        let start = Url::parse(&self.start_url).map_err(|_| bad)?;
        let cookie_urls = self
            .cookie_urls
            .iter()
            .map(|url| Url::parse(url).map_err(|_| bad))
            .collect::<Result<Vec<_>, _>>()?;
        let probe = Url::parse(&self.probe_origin).map_err(|_| bad)?;
        if !self.allows(&start)
            || cookie_urls.iter().any(|url| !self.allows(url))
            || !self.allows(&probe)
            || probe.path() != "/"
            || probe.query().is_some()
            || probe.fragment().is_some()
            || self.title.is_empty()
            || self.title.len() > 128
            || self.title.chars().any(char::is_control)
            || self.probe_script.trim().is_empty()
            || self.probe_script.len() > 16 * 1024
            || !(320..=1280).contains(&self.width)
            || !(320..=1280).contains(&self.height)
            || !(30..=600).contains(&self.timeout_seconds)
        {
            return Err(bad);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(cookie_urls: &[&str], cookie_names: &[&str]) -> LoginPolicy {
        LoginPolicy {
            start_url: "https://store.example.test/login".into(),
            cookie_urls: cookie_urls.iter().map(|url| (*url).into()).collect(),
            probe_origin: "https://store.example.test".into(),
            title: "Example".into(),
            allowed_hosts: vec!["example.test".into()],
            cookie_names: cookie_names.iter().map(|name| (*name).into()).collect(),
            probe_script: "null".into(),
            width: 600,
            height: 760,
            timeout_seconds: 600,
        }
    }

    #[test]
    fn a_cookie_free_policy_is_valid() {
        policy(&[], &[]).validate().unwrap();
    }

    #[test]
    fn a_cookie_policy_is_valid() {
        policy(&["https://store.example.test/"], &["session"])
            .validate()
            .unwrap();
    }

    #[test]
    fn several_cookie_urls_are_allowed_for_one_session() {
        policy(
            &["https://store.example.test/", "https://login.example.test/"],
            &["session", "refresh"],
        )
        .validate()
        .unwrap();
    }

    #[test]
    fn cookie_urls_and_cookie_names_must_agree() {
        assert!(
            policy(&["https://store.example.test/"], &[])
                .validate()
                .is_err()
        );
        assert!(policy(&[], &["session"]).validate().is_err());
    }

    #[test]
    fn a_cookie_url_must_stay_on_an_allowed_host() {
        assert!(
            policy(&["https://other.test/"], &["session"])
                .validate()
                .is_err()
        );
    }

    #[test]
    fn an_empty_cookie_url_is_rejected() {
        assert!(policy(&[""], &["session"]).validate().is_err());
    }

    #[test]
    fn invalid_cookie_names_are_rejected_even_when_the_url_is_absent() {
        assert!(policy(&[], &["bad name"]).validate().is_err());
    }

    #[test]
    fn a_missing_cookie_urls_list_deserializes_as_cookie_free() {
        let policy: LoginPolicy = serde_json::from_str(
            r#"{
                "start_url": "https://store.example.test/login",
                "probe_origin": "https://store.example.test",
                "title": "Example",
                "allowed_hosts": ["example.test"],
                "cookie_names": [],
                "probe_script": "null",
                "width": 600,
                "height": 760,
                "timeout_seconds": 600
            }"#,
        )
        .unwrap();
        assert_eq!(policy.cookie_urls, Vec::<String>::new());
        policy.validate().unwrap();
    }
}
