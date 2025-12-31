use loco_rs::prelude::*;
use serde::Deserialize;
use std::sync::Arc;
use tera::Tera;

use super::resources;
use crate::views;

#[derive(Debug, Deserialize, Default)]
struct DashboardQuery {
    #[serde(default)]
    upstream: Option<String>,
}

pub fn routes() -> Routes {
    Routes::new().add("/", get(index))
}

#[debug_handler]
async fn index(
    State(ctx): State<AppContext>,
    Query(query): Query<DashboardQuery>,
) -> Result<Response> {
    let tera = ctx
        .shared_store
        .get::<Arc<Tera>>()
        .ok_or_else(|| Error::Message("Tera not available".into()))?;

    let resources = resources(&ctx)?;
    let snapshot = resources
        .snapshot()
        .await
        .map_err(|err| Error::Message(err.to_string()))?;

    let show_upstream = query.upstream.is_some();
    let data = views::dashboard::DashboardData::from_snapshot(&snapshot, show_upstream);

    views::dashboard::index(&tera, data)
}
