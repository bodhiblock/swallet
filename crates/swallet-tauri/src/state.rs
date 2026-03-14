use std::sync::Mutex;
use swallet_core::service::WalletService;

pub struct AppState {
    pub service: Mutex<WalletService>,
}
