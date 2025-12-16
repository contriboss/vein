use loco_rs::prelude::*;

use crate::state::AdminResources;

pub mod catalog;
pub mod changelog;
pub mod dashboard;
pub mod health;
pub mod permissions;
pub mod quarantine;
pub mod security;

pub(crate) fn resources(ctx: &AppContext) -> Result<AdminResources> {
    ctx.shared_store
        .get::<AdminResources>()
        .ok_or_else(|| Error::Message("admin resources unavailable".into()))
}
