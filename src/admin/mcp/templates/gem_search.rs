use mcp_host::prelude::*;
use mcp_host::registry::router::McpResourceTemplateRouter;

use crate::admin::mcp::views::Search;
use crate::admin::mcp::{
    SEARCH_LIMIT, VeinMcp, json_resource, read_error, search_query, search_uri,
};

impl VeinMcp {
    /// Search every gem name published on rubygems.org by substring.
    #[mcp_resource_template(
        uri_template = "vein://gems/search/{query}",
        name = "gem_search",
        title = "Search gems",
        mime_type = "application/json"
    )]
    async fn gem_search(&self, ctx: Ctx<'_>) -> ResourceResult {
        let query = search_query(&ctx)?;
        let names = self
            .resources
            .catalog_search(&query, SEARCH_LIMIT)
            .await
            .map_err(read_error)?;
        let catalog_size = self.resources.catalog_total().await.map_err(read_error)?;
        json_resource(
            search_uri("gems", &query),
            &Search {
                query,
                scope: "rubygems.org",
                names,
                catalog_size: Some(catalog_size),
            },
        )
    }
}

pub fn mount(router: McpResourceTemplateRouter<VeinMcp>) -> McpResourceTemplateRouter<VeinMcp> {
    router.with_template(
        VeinMcp::gem_search_template_info(),
        VeinMcp::gem_search_handler,
        None,
    )
}
