use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use rmcp::{
    RoleClient, ServiceExt,
    model::{CallToolRequestParams, CallToolResult},
    service::RunningService,
    transport::StreamableHttpClientTransport,
};
use serde_json::{Value, json};

use super::config::AgentConfig;
use super::host::{AgentHost, CloseReason, Notifier};
use crate::connections::ConnectionStore;
use crate::ssh::known_hosts::KnownHosts;
use crate::vault::Vault;

#[derive(Default)]
struct Events(Mutex<Vec<String>>);

impl Notifier for Events {
    fn emit(&self, event: &str, _payload: Value) {
        self.0.lock().unwrap().push(event.to_string());
    }

    fn bring_to_front(&self) {}
}

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

fn write_json(path: &Path, value: Value) {
    std::fs::write(path, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
}

type Client = RunningService<RoleClient, ()>;

async fn call(client: &Client, tool: &str, arguments: Value) -> CallToolResult {
    let params = CallToolRequestParams::new(tool.to_string()).with_arguments(arguments.as_object().cloned().unwrap());
    client.call_tool(params).await.unwrap()
}

fn data(result: &CallToolResult) -> Value {
    assert_ne!(result.is_error, Some(true), "the tool failed: {}", text(result));
    result.structured_content.clone().unwrap()
}

fn text(result: &CallToolResult) -> String {
    result.content.iter().filter_map(|block| block.as_text()).map(|text| text.text.clone()).collect()
}

fn answer_next(host: &Arc<AgentHost>, approved: bool) {
    let host = host.clone();
    tokio::spawn(async move {
        loop {
            if let Some(request) = host.approvals.list().first() {
                host.answer(&request.id, approved);
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
}

#[tokio::test(flavor = "multi_thread")]
async fn serves_tools_over_http() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let dir =TempDir(std::env::temp_dir().join(format!("fran-db-agent-{}", uuid::Uuid::new_v4())));
    std::fs::create_dir_all(&dir.0).unwrap();
    let database = dir.0.join("app.sqlite");
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute_batch(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, team TEXT); \
             CREATE TABLE posts (id INTEGER PRIMARY KEY, user_id INTEGER REFERENCES users (id)); \
             INSERT INTO users (name, team) VALUES ('ana', 'red'), ('bo', 'red'), ('cy', 'blue');",
        )
        .unwrap();

    let port = free_port();
    let options = json!({ "path": database.to_string_lossy() });
    write_json(
        &dir.0.join("connections.json"),
        json!({ "connections": [
            { "id": "c1", "name": "Local", "kind": "sqlite", "options": options, "agentAccess": { "mode": "ask" } },
            { "id": "c2", "name": "Private", "kind": "sqlite", "options": options },
        ]}),
    );
    write_json(&dir.0.join("agent.json"), json!({ "enabled": true, "port": port, "historyEnabled": false }));

    let events = Arc::new(Events::default());
    let host = Arc::new(AgentHost::new(
        events.clone(),
        dir.0.clone(),
        Arc::new(ConnectionStore::load(dir.0.join("connections.json"))),
        Arc::new(Vault::new(dir.0.join("secrets.vault"))),
        Arc::new(KnownHosts::load(dir.0.join("known_hosts.json"))),
    ));
    host.apply().await;
    assert!(host.status().running, "{:?}", host.status());

    let url = format!("http://127.0.0.1:{port}/mcp");
    let refused = reqwest::Client::new()
        .post(&url)
        .header("Origin", "http://evil.example")
        .header("Accept", "application/json, text/event-stream")
        .json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }))
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), 403);

    let client = ().serve(StreamableHttpClientTransport::from_uri(url.as_str())).await.unwrap();
    let tools = client.list_all_tools().await.unwrap();
    assert!(tools.iter().any(|tool| tool.name == "execute_sql"));

    let connections = data(&call(&client, "list_connections", json!({})).await);
    assert_eq!(connections["connections"].as_array().unwrap().len(), 1);
    assert!(call(&client, "open_session", json!({ "connection": "Private" })).await.is_error == Some(true));

    let opened = data(&call(&client, "open_session", json!({ "connection": "local", "database": "main" })).await);
    let session = opened["session"].as_str().unwrap().to_string();
    assert_eq!(opened["databases"], json!(["main"]));

    let described = data(&call(&client, "describe_table", json!({ "session": session, "table": "users" })).await);
    assert_eq!(described["primaryKey"], json!(["id"]));
    assert_eq!(described["referencedBy"][0]["table"], "posts");

    let rows = data(
        &call(
            &client,
            "read_rows",
            json!({
                "session": session,
                "table": "users",
                "columns": ["name"],
                "filters": [{ "column": "team", "op": "eq", "value": "red" }],
                "order_by": [{ "column": "name", "descending": true }],
            }),
        )
        .await,
    );
    assert_eq!(rows["rows"], json!([["bo"], ["ana"]]));
    assert_eq!(rows["truncated"], false);

    let values = data(&call(&client, "column_values", json!({ "session": session, "table": "users", "column": "team" })).await);
    assert_eq!(values["values"][0], json!({ "value": "red", "count": 2 }));

    let write = call(&client, "run_query", json!({ "session": session, "sql": "DELETE FROM users" })).await;
    assert_eq!(write.is_error, Some(true));
    assert!(text(&write).contains("only runs reads"));
    let query = data(&call(&client, "run_query", json!({ "session": session, "sql": "SELECT name FROM users WHERE id = 1" })).await);
    assert_eq!(query["statements"][0]["results"][0]["rows"], json!([["ana"]]));

    answer_next(&host, true);
    let inserted = data(
        &call(&client, "insert_rows", json!({ "session": session, "table": "users", "rows": [{ "name": "di", "team": "blue" }] }))
            .await,
    );
    assert_eq!(inserted["executed"], 1);

    answer_next(&host, false);
    let rejected = call(&client, "delete_rows", json!({ "session": session, "table": "users", "keys": [{ "id": 1 }] })).await;
    assert!(text(&rejected).contains("rejected"));
    let count = data(&call(&client, "count_rows", json!({ "session": session, "table": "users" })).await);
    assert_eq!(count["count"], 4);

    host.set_config(AgentConfig { enabled: true, port, history_enabled: false, allow_changes: false, ..AgentConfig::default() })
        .await
        .unwrap();
    let tools = client.list_all_tools().await.unwrap();
    assert!(!tools.iter().any(|tool| tool.name == "execute_sql" || tool.name == "insert_rows"));
    let blocked = call(&client, "insert_rows", json!({ "session": session, "table": "users", "rows": [] })).await;
    assert!(text(&blocked).contains("turned off"));

    assert!(host.close_session(&session, CloseReason::User).await);
    let closed = call(&client, "count_rows", json!({ "session": session, "table": "users" })).await;
    assert!(text(&closed).contains("No open session"));

    let events = events.0.lock().unwrap().clone();
    assert!(events.iter().any(|event| event == "agent-approvals-changed"));
    assert!(events.iter().any(|event| event == "agent-sessions-changed"));

    host.set_config(AgentConfig { enabled: false, ..AgentConfig::default() }).await.unwrap();
    assert!(!host.status().running);
    let _ = client.cancel().await;
}
