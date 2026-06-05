pub mod types;
pub mod ipc;
pub mod tools;
pub mod executor;
pub mod server;

pub use types::{McpRequest, McpResponse, McpSession, RateLimiter};
pub use ipc::VmIpcClient;
pub use tools::{
    GetDocumentMetadataTool, HelpTool, ListDocumentsTool, ReadDocumentTool,
    SearchTextTool, SearchTool, ToolHandler, register_search_tool, ExecTool,
};
pub use executor::execute_program;
pub use server::McpServer;
