use crate::config::reliability::ReliabilityConfig;
use rama::http::Uri;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct UpstreamConfig {
    #[serde(default = "default_upstream_url", with = "serde_url")]
    pub url: Uri,
    #[serde(default, with = "serde_url_vec")]
    pub fallback_urls: Vec<Uri>,
    #[serde(default)]
    pub reliability: ReliabilityConfig,
}

impl Default for UpstreamConfig {
    fn default() -> Self {
        Self {
            url: default_upstream_url(),
            fallback_urls: Vec::new(),
            reliability: ReliabilityConfig::default(),
        }
    }
}

fn default_upstream_url() -> Uri {
    Uri::from_static("https://rubygems.org/")
}

mod serde_url {
    use rama::http::Uri;
    use serde::{Deserialize, Deserializer};
    use std::str::FromStr;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Uri, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Uri::from_str(&s).map_err(serde::de::Error::custom)
    }
}

mod serde_url_vec {
    use rama::http::Uri;
    use serde::{Deserialize, Deserializer};
    use std::str::FromStr;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Uri>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let list = Vec::<String>::deserialize(deserializer)?;
        list.into_iter()
            .map(|s| Uri::from_str(&s).map_err(serde::de::Error::custom))
            .collect()
    }
}
