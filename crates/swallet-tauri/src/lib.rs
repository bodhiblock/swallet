mod commands;
mod error;
mod state;

use state::AppState;
use std::sync::Mutex;
use swallet_core::config::AppConfig;
use swallet_core::service::WalletService;

#[cfg(mobile)]
#[tauri::mobile_entry_point]
pub fn mobile_run() {
    run(None, None);
}

pub fn run(data_path: Option<std::path::PathBuf>, config_path: Option<std::path::PathBuf>) {
    let config = AppConfig::load_or_create(config_path.as_deref())
        .unwrap_or_else(|e| panic!("配置加载失败: {e}"));

    let service = WalletService::new(config, data_path);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            service: Mutex::new(service),
        })
        .invoke_handler(tauri::generate_handler![
            commands::auth::has_data_file,
            commands::auth::create_store,
            commands::auth::unlock,
            commands::auth::verify_password,
            commands::auth::is_unlocked,
            commands::wallet::get_wallets,
            commands::wallet::generate_mnemonic,
            commands::wallet::add_mnemonic_wallet,
            commands::wallet::add_private_key_wallet,
            commands::wallet::add_watch_wallet,
            commands::wallet::add_derived_address,
            commands::wallet::edit_wallet_name,
            commands::wallet::edit_address_label,
            commands::wallet::move_wallet,
            commands::wallet::hide_wallet,
            commands::wallet::hide_address,
            commands::wallet::delete_wallet,
            commands::wallet::restore_hidden_wallets,
            commands::wallet::restore_hidden_addresses,
            commands::balance::get_rpc_url_for_address,
            commands::balance::get_cached_balances,
            commands::balance::refresh_balances,
            commands::transfer::build_transfer_assets,
            commands::transfer::execute_transfer,
            commands::multisig::get_local_sol_addresses,
            commands::multisig::get_solana_chains,
            commands::multisig::get_fee_payers,
            commands::multisig::import_multisig,
            commands::multisig::fetch_proposals,
            commands::multisig::create_sol_transfer_proposal,
            commands::multisig::approve_proposal,
            commands::multisig::reject_proposal,
            commands::multisig::execute_proposal,
            commands::multisig::get_preset_programs,
            commands::multisig::create_proposal,
            commands::multisig::create_multisig,
            commands::staking::fetch_vote_account,
            commands::staking::fetch_stake_account,
            commands::staking::create_vote_account,
            commands::staking::create_stake_account,
            commands::staking::stake_delegate,
            commands::staking::stake_deactivate,
            commands::staking::stake_withdraw,
        ])
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用失败");
}
