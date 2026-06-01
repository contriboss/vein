use anyhow::Result;
use percent_encoding::percent_decode_str;
use rama::{
    http::{Body, Request, Response},
    telemetry::tracing::warn,
};

use crate::http_cache::{CacheOutcome, CachedTextOptions, MetaStoreMode, fetch_cached_text};

use super::{CacheStatus, RequestContext, VeinProxy, quarantine, utils};

#[derive(Debug, Clone)]
pub(super) enum CompactRequest {
    Versions,
    Names,
    Info { name: String },
}

impl CompactRequest {
    pub(super) fn from_path(path: &str) -> Option<Self> {
        match path {
            "/versions" => Some(Self::Versions),
            "/names" => Some(Self::Names),
            _ if path.starts_with("/info/") => {
                let decoded = path.trim_start_matches("/info/");
                if decoded.is_empty() {
                    return None;
                }
                let decoded = percent_decode_str(decoded).decode_utf8().ok()?;
                Some(Self::Info {
                    name: decoded.to_string(),
                })
            }
            _ => None,
        }
    }

    fn storage_path(&self) -> String {
        match self {
            Self::Versions => "compact_index/versions".to_string(),
            Self::Names => "compact_index/names".to_string(),
            Self::Info { name, .. } => {
                format!("compact_index/info/{}", utils::sanitize_filename(name))
            }
        }
    }

    fn meta_key(&self) -> String {
        match self {
            Self::Versions => "compact:versions".to_string(),
            Self::Names => "compact:names".to_string(),
            Self::Info { name, .. } => format!("compact:info:{name}"),
        }
    }

    fn content_type(&self) -> &'static str {
        "text/plain"
    }
}

impl VeinProxy {
    pub(super) async fn handle_compact_request(
        &self,
        req: &Request<Body>,
        compact: CompactRequest,
        _ctx: &mut RequestContext,
    ) -> Result<Option<(Response<Body>, CacheStatus)>> {
        let storage_path = compact.storage_path();
        let meta_key = compact.meta_key();
        let content_type = compact.content_type();
        let info_name = match &compact {
            CompactRequest::Info { name } => Some(name.clone()),
            _ => None,
        };

        let delay_policy = &self.config.delay_policy;
        let index = self.index.as_ref();

        let result = fetch_cached_text(
            &self.storage,
            index,
            CachedTextOptions {
                storage_path: &storage_path,
                meta_key: &meta_key,
                content_type,
                cache_control: "public, max-age=300",
                include_content_length: true,
                meta_mode: MetaStoreMode::Strict,
                strip_transfer_encoding: true,
            },
            |headers| async move { self.fetch_with_fallback(req, Some(&headers)).await },
            move |body| async move {
                if let Some(name) = info_name {
                    match quarantine::filter_compact_info(delay_policy, index, &name, &body).await {
                        Ok(filtered) => Ok(filtered),
                        Err(err) => {
                            warn!(
                                error = %err,
                                gem = %name,
                                "Failed to filter quarantined versions"
                            );
                            Ok(body)
                        }
                    }
                } else {
                    Ok(body)
                }
            },
        )
        .await?;

        let cache_status = match result.outcome {
            CacheOutcome::Hit => CacheStatus::Hit,
            CacheOutcome::Miss => CacheStatus::Miss,
            CacheOutcome::Revalidated => CacheStatus::Revalidated,
            CacheOutcome::Pass => CacheStatus::Pass,
        };

        Ok(Some((result.response, cache_status)))
    }
}
