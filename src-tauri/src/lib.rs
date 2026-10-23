pub mod connections;
pub mod db;
pub mod error;
pub mod forward;
pub mod json_store;
pub mod saved_queries;
pub mod settings;
pub mod ssh;
pub mod tab_state;
pub mod vault;

use std::sync::Arc;

use tauri::{Manager, RunEvent};

use connections::ConnectionStore;
use db::sessions::SessionManager;
use saved_queries::SavedQueryStore;
use settings::SettingsStore;
use tab_state::TabStateStore;
use ssh::known_hosts::KnownHosts;
use vault::Vault;

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            app.manage(Arc::new(Vault::new(data_dir.join("secrets.vault"))));
            app.manage(Arc::new(ConnectionStore::load(data_dir.join("connections.json"))));
            app.manage(Arc::new(KnownHosts::load(data_dir.join("known_hosts.json"))));
            app.manage(Arc::new(SavedQueryStore::load(data_dir.join("saved_queries.json"))));
            app.manage(Arc::new(TabStateStore::load(data_dir.join("tab_state.json"))));
            app.manage(Arc::new(SettingsStore::load(data_dir.join("settings.json"))));
            app.manage(SessionManager::default());
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
            db::commands::open_session,
            db::commands::test_connection,
            db::commands::close_session,
            db::commands::list_catalogs,
            db::commands::switch_catalog,
            db::commands::list_databases,
            db::commands::list_objects,
            db::commands::get_object_structure,
            db::commands::fetch_table_rows,
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
        ])
        .build(tauri::generate_context!())
        .expect("error while building the app");

    app.run(|app_handle, event| {
        if let RunEvent::Exit = event {
            let sessions = app_handle.state::<SessionManager>().drain();
            tauri::async_runtime::block_on(async move {
                for open in sessions {
                    open.session.close().await;
                }
            });
        }
    });
}
