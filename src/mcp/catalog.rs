//! Pre-seeded list of well-known remote MCP servers so a user only has to
//! paste an API key rather than hand-configure a base URL from scratch.
//! Anything not in this list is a "custom" server, configured purely in DB.

pub struct CatalogEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub base_url: &'static str,
    pub docs_url: &'static str,
}

pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "context7",
        name: "Context7",
        base_url: "https://mcp.context7.com/mcp",
        docs_url: "https://context7.com/docs",
    },
    CatalogEntry {
        id: "deepwiki",
        name: "DeepWiki",
        base_url: "https://mcp.deepwiki.com/mcp",
        docs_url: "https://docs.devin.ai/work-with-devin/deepwiki-mcp",
    },
    CatalogEntry {
        id: "github",
        name: "GitHub",
        base_url: "https://api.githubcopilot.com/mcp/",
        docs_url: "https://github.com/github/github-mcp-server",
    },
    CatalogEntry {
        id: "sentry",
        name: "Sentry",
        base_url: "https://mcp.sentry.dev/mcp",
        docs_url: "https://docs.sentry.io/product/sentry-mcp/",
    },
    CatalogEntry {
        id: "linear",
        name: "Linear",
        base_url: "https://mcp.linear.app/mcp",
        docs_url: "https://linear.app/docs/mcp",
    },
    CatalogEntry {
        id: "notion",
        name: "Notion",
        base_url: "https://mcp.notion.com/mcp",
        docs_url: "https://developers.notion.com/docs/mcp",
    },
    CatalogEntry {
        id: "atlassian",
        name: "Atlassian (Jira / Confluence)",
        base_url: "https://mcp.atlassian.com/v1/mcp",
        docs_url: "https://www.atlassian.com/platform/remote-mcp-server",
    },
    CatalogEntry {
        id: "cloudflare-docs",
        name: "Cloudflare Docs",
        base_url: "https://docs.mcp.cloudflare.com/mcp",
        docs_url: "https://developers.cloudflare.com/agents/model-context-protocol/",
    },
];

pub fn find(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}
