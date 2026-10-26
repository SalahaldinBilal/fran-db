pub mod agent;
pub mod connections;
pub mod db;
pub mod error;
pub mod forward;
pub mod json_store;
pub mod saved_queries;
pub mod settings;
pub mod splash;
pub mod ssh;
pub mod tab_state;
pub mod vault;

use std::sync::Arc;

use tauri::{Manager, RunEvent};

use agent::{AgentHost, CloseReason};
use connections::ConnectionStore;
use db::sessions::SessionManager;
use saved_queries::SavedQueryStore;
use settings::SettingsStore;
use tab_state::TabStateStore;
use ssh::{forwards::PortForwards, known_hosts::KnownHosts};
use vault::Vault;

pub fn run() {
    let app = tauri::Builder::default()
        // Has to be the first plugin, so a second launch exits before anything else starts.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| splash::bring_to_front(app)))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_os::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            let vault = Arc::new(Vault::new(data_dir.join("secrets.vault")));
            let store = Arc::new(ConnectionStore::load(data_dir.join("connections.json")));
            let known_hosts = Arc::new(KnownHosts::load(data_dir.join("known_hosts.json")));
            let agent = Arc::new(AgentHost::new(
                Arc::new(app.handle().clone()),
                data_dir.clone(),
                store.clone(),
                vault.clone(),
                known_hosts.clone(),
            ));

            app.manage(vault);
            app.manage(store);
            app.manage(known_hosts);
            app.manage(Arc::new(SavedQueryStore::load(data_dir.join("saved_queries.json"))));
            app.manage(Arc::new(TabStateStore::load(data_dir.join("tab_state.json"))));
            app.manage(Arc::new(SettingsStore::load(data_dir.join("settings.json"))));
            app.manage(SessionManager::default());
            app.manage(PortForwards::default());
            app.manage(agent.clone());

            agent.start_maintenance();
            tauri::async_runtime::spawn(async move { agent.apply().await });
            splash::open(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            vault::commands::get_vault_status,
            vault::commands::reset_vault,
            connections::commands::list_connections,
            connections::commands::save_connection,
            connections::commands::duplicate_connection,
            connections::commands::delete_connection,
            connections::commands::get_stored_secrets,
            connections::commands::set_connection_secret,
            connections::commands::clear_connection_secret,
            ssh::known_hosts::trust_host_key,
            ssh::forwards::start_port_forward,
            ssh::forwards::stop_port_forward,
            ssh::forwards::list_port_forwards,
            db::commands::open_session,
            db::commands::test_connection,
            db::commands::close_session,
            db::commands::list_catalogs,
            db::commands::switch_catalog,
            db::commands::list_databases,
            db::commands::get_database_sizes,
            db::commands::list_objects,
            db::commands::get_object_structure,
            db::commands::get_primary_key,
            db::commands::get_column_choices,
            db::commands::open_query_context,
            db::commands::close_query_context,
            db::commands::execute_query,
            db::commands::cancel_query,
            db::commands::split_sql,
            db::commands::list_columns,
            db::commands::get_object_sql,
            db::commands::get_database_sql,
            db::commands::preview_row_changes,
            db::commands::execute_batch,
            db::commands::get_table_design,
            db::commands::get_design_options,
            db::commands::preview_table_design,
            saved_queries::list_saved_queries,
            saved_queries::save_query,
            saved_queries::delete_saved_query,
            tab_state::get_tab_state,
            tab_state::set_tab_state,
            settings::get_settings,
            settings::set_settings,
            splash::show_main_window,
            agent::commands::get_agent_config,
            agent::commands::set_agent_config,
            agent::commands::get_agent_status,
            agent::commands::list_agent_sessions,
            agent::commands::close_agent_session,
            agent::commands::close_all_agent_sessions,
            agent::commands::list_agent_approvals,
            agent::commands::answer_agent_request,
            agent::commands::list_agent_history,
            agent::commands::get_agent_history_entry,
            agent::commands::get_agent_history_size,
            agent::commands::clear_agent_history,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the app");

    app.run(|app_handle, event| {
        if let RunEvent::Exit = event {
            let sessions = app_handle.state::<SessionManager>().drain();
            let agent = app_handle.state::<Arc<AgentHost>>().inner().clone();
            tauri::async_runtime::block_on(async move {
                for open in sessions {
                    open.session.close().await;
                }
                agent.close_all(CloseReason::AppExit).await;
            });
        }
    });
}
