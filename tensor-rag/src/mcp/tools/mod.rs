use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use serde_json::Value as JsonValue;

pub trait ToolHandler: Send + Sync {
    fn handle<'a>(
        &'a self,
        args: JsonValue,
        db_pools: &'a crate::db::DbPools,
        metrics: Option<Arc<crate::metrics::Metrics>>,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>>;
}

pub mod search;
pub mod search_text;
pub mod read_document;
pub mod list_documents;
pub mod get_document_metadata;
pub mod exec;
pub mod help;

pub use search::SearchTool;
pub use search_text::SearchTextTool;
pub use read_document::ReadDocumentTool;
pub use list_documents::ListDocumentsTool;
pub use get_document_metadata::GetDocumentMetadataTool;
pub use exec::ExecTool;
pub use help::HelpTool;

pub fn register_search_tool(server: &mut super::server::McpServer) {
    server.register_tool("search", Box::new(SearchTool));
    server.register_tool("search_text", Box::new(SearchTextTool));
    server.register_tool("read_document", Box::new(ReadDocumentTool));
    server.register_tool("list_documents", Box::new(ListDocumentsTool));
    server.register_tool("get_document_metadata", Box::new(GetDocumentMetadataTool));
    server.register_tool("exec", Box::new(ExecTool));
    server.register_tool("help", Box::new(HelpTool));
}
