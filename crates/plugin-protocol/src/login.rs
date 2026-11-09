//! Trusted packaged login declarations. Runtime RPCs contain only scoped window
//! handles and operations; they cannot upload or broaden this policy.

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoginPolicy {
    pub start_url: String,
    pub cookie_url: String,
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
            || self.cookie_names.is_empty()
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
        let cookie = Url::parse(&self.cookie_url).map_err(|_| bad)?;
        let probe = Url::parse(&self.probe_origin).map_err(|_| bad)?;
        if !self.allows(&start)
            || !self.allows(&cookie)
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
