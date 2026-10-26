use std::{sync::Arc, time::Instant};

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::{router::tool::ToolRouter, tool::ToolCallContext, wrapper::Parameters},
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation, ListToolsResult,
        PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    },
    service::{NotificationContext, RequestContext},
    tool, tool_router,
};
use serde_json::Value;

use super::args::{
    AlterTableArgs, ColumnValuesArgs, CountRowsArgs, CreateTableArgs, DefinitionArgs, DeleteRowsArgs, DescribeTableArgs,
    DropObjectArgs, InsertRowsArgs, ListObjectsArgs, OpenSessionArgs, ReadRowsArgs, RelationshipsArgs, RunQueryArgs,
    SearchSchemaArgs, SessionArgs, SqlArgs, TableArgs, UpdateRowsArgs,
};
use super::call::CallLog;
use super::config::AgentConfig;
use super::history::{HistoryEntry, HistoryEvent, Outcome, now_ms};
use super::host::AgentHost;
use crate::error::AppResult;

const INSTRUCTIONS: &str = "Fran DB gives you the database connections its user chose to share. \
Call list_connections, then open_session on one, pass the session id to every other tool, and call \
close_session when you're done. Learn the schema with list_databases, list_objects, describe_table, \
search_schema and get_relationships, and read data with read_rows, count_rows and column_values before \
reaching for raw SQL. A connection is either read only or lets you make changes, and every change waits \
for the user to approve it in Fran DB, so say what you're about to change and why.";

const RAW_SQL_TOOLS: [&str; 3] = ["run_query", "explain_query", "execute_sql"];
const CHANGE_TOOLS: [&str; 8] = [
    "insert_rows",
    "update_rows",
    "delete_rows",
    "get_table_design",
    "create_table",
    "alter_table",
    "drop_object",
    "execute_sql",
];

pub fn tool_enabled(name: &str, config: &AgentConfig) -> bool {
    (config.allow_raw_sql || !RAW_SQL_TOOLS.contains(&name)) && (config.allow_changes || !CHANGE_TOOLS.contains(&name))
}

fn reply(result: AppResult<Value>) -> Result<CallToolResult, McpError> {
    Ok(match result {
        Ok(value) => CallToolResult::structured(value),
        Err(error) => CallToolResult::error(vec![ContentBlock::text(error.message)]),
    })
}

fn call_log(context: &RequestContext<RoleServer>) -> Result<Arc<CallLog>, McpError> {
    context
        .extensions
        .get::<Arc<CallLog>>()
        .cloned()
        .ok_or_else(|| McpError::internal_error("The call wasn't set up for history", None))
}

#[derive(Clone)]
pub struct AgentServer {
    host: Arc<AgentHost>,
    tool_router: ToolRouter<Self>,
}

impl AgentServer {
    pub fn new(host: Arc<AgentHost>) -> Self {
        Self {
            host,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl AgentServer {
    #[tool(description = "Lists the connections the user shares with agents, with each one's access mode", annotations(read_only_hint = true))]
    async fn list_connections(&self) -> Result<CallToolResult, McpError> {
        reply(self.host.list_connections())
    }

    #[tool(
        description = "Connects to a shared connection and returns a session id for the other tools, plus the databases you can use",
        annotations(read_only_hint = true)
    )]
    async fn open_session(
        &self,
        Parameters(args): Parameters<OpenSessionArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.open_session(args, &log).await)
    }

    #[tool(description = "Closes a session when you're done with it", annotations(read_only_hint = true))]
    async fn close_session(
        &self,
        Parameters(args): Parameters<SessionArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.close_agent_session(args, &log).await)
    }

    #[tool(
        description = "Lists the databases (schemas on Postgres) you can use in a session, with their sizes",
        annotations(read_only_hint = true)
    )]
    async fn list_databases(
        &self,
        Parameters(args): Parameters<SessionArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.list_databases(args, &log).await)
    }

    #[tool(
        description = "Lists tables, views, routines, triggers and sequences in a database, with row estimates, sizes and comments",
        annotations(read_only_hint = true)
    )]
    async fn list_objects(
        &self,
        Parameters(args): Parameters<ListObjectsArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.list_objects(args, &log).await)
    }

    #[tool(
        description = "Describes a table or view: columns, primary key, indexes, its foreign keys and the keys in other tables that point at it",
        annotations(read_only_hint = true)
    )]
    async fn describe_table(
        &self,
        Parameters(args): Parameters<DescribeTableArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.describe_table(args, &log).await)
    }

    #[tool(
        description = "Returns the CREATE statement of a table, view, routine, trigger or other object",
        annotations(read_only_hint = true)
    )]
    async fn get_definition(
        &self,
        Parameters(args): Parameters<DefinitionArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.get_definition(args, &log).await)
    }

    #[tool(
        description = "Finds tables and columns whose names contain some text, across the databases you can use",
        annotations(read_only_hint = true)
    )]
    async fn search_schema(
        &self,
        Parameters(args): Parameters<SearchSchemaArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.search_schema(args, &log).await)
    }

    #[tool(
        description = "Lists the foreign keys of a database, or only those from or to one table",
        annotations(read_only_hint = true)
    )]
    async fn get_relationships(
        &self,
        Parameters(args): Parameters<RelationshipsArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.get_relationships(args, &log).await)
    }

    #[tool(
        description = "Reads rows from one table with optional columns, filters (use isNull for nulls), order, limit and offset",
        annotations(read_only_hint = true)
    )]
    async fn read_rows(
        &self,
        Parameters(args): Parameters<ReadRowsArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.read_rows(args, &log).await)
    }

    #[tool(description = "Counts the rows of a table, optionally only those matching filters", annotations(read_only_hint = true))]
    async fn count_rows(
        &self,
        Parameters(args): Parameters<CountRowsArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.count_rows(args, &log).await)
    }

    #[tool(
        description = "Lists the distinct values of a column with how often each appears, most common first",
        annotations(read_only_hint = true)
    )]
    async fn column_values(
        &self,
        Parameters(args): Parameters<ColumnValuesArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.column_values(args, &log).await)
    }

    #[tool(
        description = "Shows how the server would run one statement, without running it",
        annotations(read_only_hint = true)
    )]
    async fn explain_query(
        &self,
        Parameters(args): Parameters<SqlArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.explain_query(args, &log).await)
    }

    #[tool(
        description = "Runs SQL that only reads, such as SELECT, SHOW or EXPLAIN. Anything that could change data is refused. \
                       Prefer the other read tools when they fit",
        annotations(read_only_hint = true)
    )]
    async fn run_query(
        &self,
        Parameters(args): Parameters<RunQueryArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.run_query(args, &log).await)
    }

    #[tool(
        description = "Inserts rows into a table. Waits for the user to approve it in Fran DB",
        annotations(destructive_hint = false)
    )]
    async fn insert_rows(
        &self,
        Parameters(args): Parameters<InsertRowsArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.insert_rows(args, &log).await)
    }

    #[tool(
        description = "Updates rows found by their full primary key. Waits for the user to approve it in Fran DB",
        annotations(destructive_hint = true)
    )]
    async fn update_rows(
        &self,
        Parameters(args): Parameters<UpdateRowsArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.update_rows(args, &log).await)
    }

    #[tool(
        description = "Deletes rows found by their full primary key. Waits for the user to approve it in Fran DB",
        annotations(destructive_hint = true)
    )]
    async fn delete_rows(
        &self,
        Parameters(args): Parameters<DeleteRowsArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.delete_rows(args, &log).await)
    }

    #[tool(
        description = "Returns a table's design (columns, indexes, foreign keys) to edit and pass to alter_table, \
                       or to use as the shape for create_table",
        annotations(read_only_hint = true)
    )]
    async fn get_table_design(
        &self,
        Parameters(args): Parameters<TableArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.get_table_design(args, &log).await)
    }

    #[tool(
        description = "Creates a table from a design. Waits for the user to approve it in Fran DB",
        annotations(destructive_hint = false)
    )]
    async fn create_table(
        &self,
        Parameters(args): Parameters<CreateTableArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.create_table(args, &log).await)
    }

    #[tool(
        description = "Changes a table to match an edited design from get_table_design. Waits for the user to approve it in Fran DB",
        annotations(destructive_hint = true)
    )]
    async fn alter_table(
        &self,
        Parameters(args): Parameters<AlterTableArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.alter_table(args, &log).await)
    }

    #[tool(
        description = "Drops a table, view, routine, trigger or other object. Waits for the user to approve it in Fran DB",
        annotations(destructive_hint = true)
    )]
    async fn drop_object(
        &self,
        Parameters(args): Parameters<DropObjectArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.drop_object(args, &log).await)
    }

    #[tool(
        description = "Runs SQL that changes data or schema, when no other tool fits. Waits for the user to approve it in Fran DB",
        annotations(destructive_hint = true)
    )]
    async fn execute_sql(
        &self,
        Parameters(args): Parameters<SqlArgs>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let log = call_log(&context)?;
        reply(self.host.execute_sql(args, &log).await)
    }
}

fn history_entry(log: &CallLog, tool: String, arguments: Option<Value>, at: u64, started: Instant, response: &Result<CallToolResponse, McpError>) -> HistoryEntry {
    let (outcome, message, reply) = match response {
        Ok(CallToolResponse::Complete(result)) if result.is_error == Some(true) => {
            let text: Vec<&str> = result.content.iter().filter_map(|block| block.as_text()).map(|text| text.text.as_str()).collect();
            (Outcome::Error, Some(text.join("\n")), None)
        }
        Ok(CallToolResponse::Complete(result)) => (Outcome::Ok, None, result.structured_content.clone()),
        Ok(_) => (Outcome::Ok, None, None),
        Err(error) => (Outcome::Error, Some(error.message.to_string()), None),
    };
    let state = log.take();

    HistoryEntry {
        id: uuid::Uuid::new_v4().to_string(),
        at,
        event: HistoryEvent::Call,
        agent: Some(log.agent.clone()),
        session_id: state.session_id,
        connection_id: state.connection_id,
        connection_name: state.connection_name,
        database: state.database,
        tool: Some(tool),
        arguments,
        statements: state.statements,
        approval: state.approval,
        outcome: state.outcome.unwrap_or(outcome),
        message,
        duration_ms: started.elapsed().as_secs_f64() * 1000.0,
        reply,
    }
}

impl ServerHandler for AgentServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().enable_tool_list_changed().build())
            .with_server_info(Implementation::new("fran-db", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let config = self.host.config().map_err(|error| McpError::internal_error(error.message, None))?;
        let tools: Vec<Tool> = self
            .tool_router
            .list_all()
            .into_iter()
            .filter(|tool| tool_enabled(&tool.name, &config))
            .collect();
        Ok(ListToolsResult::with_all_items(tools).with_ttl_ms(0))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        let config = self.host.config().ok()?;
        tool_enabled(name, &config).then(|| self.tool_router.get(name).cloned()).flatten()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        mut context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let at = now_ms();
        let started = Instant::now();
        let tool = request.name.to_string();
        let arguments = request.arguments.clone().map(Value::Object);
        let agent = context.client_info().map_or_else(|| "Unknown agent".to_string(), |info| info.name);
        let log = Arc::new(CallLog::new(agent, arguments.clone(), context.ct.clone()));

        let enabled = self.host.config().is_ok_and(|config| tool_enabled(&tool, &config));
        let response = if enabled {
            context.extensions.insert(log.clone());
            self.tool_router.call(ToolCallContext::new(self, request, context)).await
        } else {
            log.outcome(Outcome::Refused);
            Ok(CallToolResult::error(vec![ContentBlock::text(format!("{tool} is turned off for agents in Fran DB"))]).into())
        };

        self.host.record(history_entry(&log, tool, arguments, at, started, &response));
        response
    }

    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        self.host.add_peer(context.peer.clone());
    }
}
