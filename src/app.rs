use std::str::FromStr;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use crate::crypto::SecureClear;

use crate::chain::{registry, BalanceCache};
use crate::config::AppConfig;
use crate::crypto::{eth_keys, mnemonic, sol_keys};
use crate::multisig::{self, ProposalType};
use crate::storage::data::{
    ChainType, DerivedAccount, VaultAccount, Wallet, WalletStore, WalletType, WatchOnlySource,
};
use crate::storage::encrypted;
use crate::transfer::{self, AssetKind, TransferableAsset};
use crate::tui::event;
use crate::tui::screens::{
    action_menu, add_wallet, dex as dex_screen, main_screen,
    multisig as multisig_screen, staking as staking_screen,
    transfer as transfer_screen, unlock,
};
use crate::tui::state::{
    ActionContext, ActionItem, AddWalletOption, AddWalletStep, InputPurpose, MsChainSelectPurpose,
    MultisigStep, Screen, StakingCreateType, StakingOp, StakingStep, TransferStep, UiState,
    UnlockMode, VoteAction,
};

/// 后台任务消息
#[allow(dead_code)]
enum BgMessage {
    BalancesUpdated(BalanceCache),
    BalanceFetchError(String),
    TransferComplete { success: bool, message: String },
    MultisigFetched(multisig::MultisigInfo),
    MultisigFetchError(String),
    ProposalsFetched(Vec<multisig::ProposalInfo>),
    ProposalsFetchError(String),
    MultisigOpComplete { success: bool, message: String },
    MultisigCreated { address: String, tx_sig: String },
    StakingOpComplete { success: bool, message: String },
    VoteAccountFetched(crate::staking::VoteAccountInfo),
    StakeAccountFetched(crate::staking::StakeAccountInfo),
    StakingFetchError(String),
}

pub struct App {
    pub config: AppConfig,
    pub store: Option<WalletStore>,
    pub ui: UiState,
    pub balance_cache: BalanceCache,
    /// 解锁后保存密码用于后续数据保存
    password: Option<Vec<u8>>,
    /// 数据文件路径
    data_path: std::path::PathBuf,
    /// tokio 运行时（后台 RPC 查询）
    runtime: tokio::runtime::Runtime,
    /// 后台消息接收端
    bg_rx: mpsc::Receiver<BgMessage>,
    /// 后台消息发送端（clone 给 spawned tasks）
    bg_tx: mpsc::Sender<BgMessage>,
    /// 是否正在加载余额
    loading_balances: bool,
    /// 上次刷新时间
    last_refresh: Option<Instant>,
}

/// 自动刷新间隔（秒）
const AUTO_REFRESH_SECS: u64 = 60;

impl App {
    pub fn new(config: AppConfig, data_path: Option<std::path::PathBuf>) -> Self {
        let data_path = data_path.unwrap_or_else(encrypted::default_data_file_path);
        let has_data = encrypted::data_file_exists(&data_path);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("创建 tokio 运行时失败");
        let (bg_tx, bg_rx) = mpsc::channel();
        Self {
            config,
            store: None,
            ui: UiState::new(has_data),
            balance_cache: BalanceCache::default(),
            password: None,
            data_path,
            runtime,
            bg_rx,
            bg_tx,
            loading_balances: false,
            last_refresh: None,
        }
    }

    pub fn run(&mut self, terminal: &mut crate::tui::Tui) -> anyhow::Result<()> {
        while !self.ui.should_quit {
            // 处理后台消息
            self.process_bg_messages();

            // 自动刷新余额
            self.auto_refresh_balances();

            terminal.draw(|frame| self.render(frame))?;

            if let Some(key) = event::poll_key_event(Duration::from_millis(100))? {
                if event::is_quit_key(&key) {
                    self.ui.should_quit = true;
                    continue;
                }
                self.handle_key(key);
            }
        }
        Ok(())
    }

    fn render(&self, frame: &mut Frame) {
        match self.ui.screen {
            Screen::Unlock => unlock::render(frame, &self.ui),
            Screen::Main => {
                let store = self.store.as_ref().unwrap_or(&EMPTY_STORE);
                main_screen::render(frame, &self.ui, store, &self.balance_cache, self.loading_balances);
            }
            Screen::AddWallet | Screen::ShowMnemonic | Screen::TextInput => {
                add_wallet::render(frame, &self.ui);
            }
            Screen::ActionMenu => {
                let store = self.store.as_ref().unwrap_or(&EMPTY_STORE);
                main_screen::render(frame, &self.ui, store, &self.balance_cache, self.loading_balances);
                action_menu::render(frame, &self.ui);
            }
            Screen::Transfer => {
                transfer_screen::render(frame, &self.ui);
            }
            Screen::Multisig => {
                let multisigs = self
                    .store
                    .as_ref()
                    .map(|s| s.multisigs.as_slice())
                    .unwrap_or(&[]);
                let address_labels = self
                    .store
                    .as_ref()
                    .map(|s| s.address_labels())
                    .unwrap_or_default();
                multisig_screen::render(frame, &self.ui, multisigs, &address_labels);
            }
            Screen::Dex => {
                dex_screen::render(frame);
            }
            Screen::Staking => {
                staking_screen::render(frame, &self.ui);
            }
            Screen::ConfirmDelete => {
                let store = self.store.as_ref().unwrap_or(&EMPTY_STORE);
                main_screen::render(frame, &self.ui, store, &self.balance_cache, self.loading_balances);
                self.render_confirm_delete(frame);
            }
        }
    }

    /// 处理后台消息
    fn process_bg_messages(&mut self) {
        while let Ok(msg) = self.bg_rx.try_recv() {
            match msg {
                BgMessage::BalancesUpdated(cache) => {
                    self.balance_cache = cache;
                    self.loading_balances = false;
                    self.last_refresh = Some(Instant::now());
                    self.ui.clear_status();
                }
                BgMessage::BalanceFetchError(err) => {
                    self.loading_balances = false;
                    self.ui.set_status(format!("余额查询失败: {err}"));
                }
                BgMessage::TransferComplete { success, message } => {
                    self.ui.transfer_result = Some((success, message));
                    self.ui.transfer_step = TransferStep::Result;
                }
                BgMessage::MultisigFetched(info) => {
                    // 首次导入（从导入流程或创建后导入），保存到 store
                    if matches!(
                        self.ui.ms_step,
                        MultisigStep::InputAddress
                            | MultisigStep::List
                            | MultisigStep::Submitting
                    ) {
                        self.save_multisig_to_store(&info);
                    }
                    self.ui.ms_current_info = Some(info);
                    self.ui.ms_step = MultisigStep::ViewDetail;
                    self.ui.clear_status();
                }
                BgMessage::MultisigFetchError(err) => {
                    self.ui.set_status(format!("获取多签信息失败: {err}"));
                    // 导入失败回到输入页，详情页失败留在详情页
                    if matches!(
                        self.ui.ms_step,
                        MultisigStep::InputAddress | MultisigStep::Submitting
                    ) {
                        self.ui.ms_step = MultisigStep::InputAddress;
                    } else {
                        self.ui.ms_step = MultisigStep::List;
                    }
                }
                BgMessage::ProposalsFetched(proposals) => {
                    self.ui.ms_proposals = proposals;
                    self.ui.ms_proposal_selected = 0;
                    self.ui.ms_step = MultisigStep::ViewProposals;
                }
                BgMessage::ProposalsFetchError(err) => {
                    self.ui.set_status(format!("获取提案失败: {err}"));
                }
                BgMessage::MultisigOpComplete { success, message } => {
                    self.ui.ms_result = Some((success, message));
                    self.ui.ms_step = MultisigStep::Result;
                }
                BgMessage::MultisigCreated { address, tx_sig } => {
                    // 显示创建成功结果
                    self.ui.ms_result = Some((
                        true,
                        format!("多签创建成功!\n地址: {address}\n交易: {tx_sig}\n\n按任意键导入该多签..."),
                    ));
                    self.ui.ms_step = MultisigStep::Result;
                    // 存储待导入的地址和交易签名
                    self.ui.ms_created_address = Some((address, tx_sig));
                }
                BgMessage::StakingOpComplete { success, message } => {
                    self.ui.stk_result = Some((success, message));
                    self.ui.stk_step = StakingStep::Result;
                }
                BgMessage::VoteAccountFetched(info) => {
                    self.ui.stk_vote_info = Some(info);
                    self.ui.stk_step = StakingStep::VoteDetail;
                    self.ui.stk_detail_selected = 0;
                    self.ui.clear_status();
                }
                BgMessage::StakeAccountFetched(info) => {
                    self.ui.stk_stake_info = Some(info);
                    self.ui.stk_step = StakingStep::StakeDetail;
                    self.ui.stk_detail_selected = 0;
                    self.ui.clear_status();
                }
                BgMessage::StakingFetchError(err) => {
                    self.ui.stk_fetch_error = Some(err);
                }
            }
        }
    }

    /// 触发后台余额刷新
    fn trigger_balance_refresh(&mut self) {
        if self.loading_balances {
            return;
        }
        let store = match &self.store {
            Some(s) => s.clone(),
            None => return,
        };

        // 如果缓存为空，先填充占位数据（所有链显示 -）
        if self.balance_cache.is_empty() {
            self.balance_cache = registry::build_placeholder_cache(&self.config, &store);
        }

        let config = self.config.clone();
        let tx = self.bg_tx.clone();
        self.loading_balances = true;

        self.runtime.spawn(async move {
            let cache = registry::fetch_all_balances(&config, &store).await;
            let _ = tx.send(BgMessage::BalancesUpdated(cache));
        });
    }

    /// 自动刷新：进入主界面后首次刷新，之后按间隔刷新
    fn auto_refresh_balances(&mut self) {
        if self.ui.screen != Screen::Main || self.store.is_none() {
            return;
        }
        let should_refresh = match self.last_refresh {
            None => true,
            Some(last) => last.elapsed() > Duration::from_secs(AUTO_REFRESH_SECS),
        };
        if should_refresh {
            self.trigger_balance_refresh();
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match self.ui.screen {
            Screen::Unlock => self.handle_unlock_key(key),
            Screen::Main => self.handle_main_key(key),
            Screen::AddWallet | Screen::ShowMnemonic | Screen::TextInput => {
                self.handle_add_wallet_key(key);
            }
            Screen::ActionMenu => self.handle_action_menu_key(key),
            Screen::Transfer => self.handle_transfer_key(key),
            Screen::Multisig => self.handle_multisig_key(key),
            Screen::Dex => {
                if key.code == KeyCode::Esc {
                    self.ui.back_to_main();
                }
            }
            Screen::Staking => self.handle_staking_key(key),
            Screen::ConfirmDelete => self.handle_confirm_delete_key(key),
        }
    }

    // ========== 解锁 ==========

    fn handle_unlock_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) => {
                self.ui.clear_status();
                self.ui.password_input.push(c);
            }
            KeyCode::Backspace => {
                self.ui.clear_status();
                self.ui.password_input.pop();
            }
            KeyCode::Enter => {
                if self.ui.password_input.is_empty() {
                    self.ui.set_status("密码不能为空");
                    return;
                }
                self.process_unlock();
            }
            KeyCode::Esc => {
                if self.ui.unlock_mode == UnlockMode::Confirm {
                    self.ui.unlock_mode = UnlockMode::Create;
                    self.ui.password_input.clear();
                    self.ui.password_first = None;
                    self.ui.clear_status();
                }
            }
            _ => {}
        }
    }

    fn process_unlock(&mut self) {
        match self.ui.unlock_mode {
            UnlockMode::Create => {
                if self.ui.password_input.len() < 8 {
                    self.ui.set_status("密码至少8位");
                    return;
                }
                self.ui.password_first = Some(self.ui.password_input.clone());
                self.ui.password_input.clear();
                self.ui.unlock_mode = UnlockMode::Confirm;
                self.ui.clear_status();
            }
            UnlockMode::Confirm => {
                let first = self.ui.password_first.as_deref().unwrap_or("");
                if self.ui.password_input != first {
                    self.ui.set_status("两次密码不一致");
                    self.ui.password_input.clear();
                    return;
                }
                let store = WalletStore::new();
                let pw = self.ui.password_input.as_bytes().to_vec();
                match encrypted::save(&store, &pw, &self.data_path) {
                    Ok(()) => {
                        self.store = Some(store);
                        self.password = Some(pw);
                        self.ui.password_input.clear();
                        self.ui.password_first = None;
                        self.ui.screen = Screen::Main;
                        self.ui.clear_status();
                    }
                    Err(e) => {
                        self.ui.set_status(format!("保存失败: {e}"));
                    }
                }
            }
            UnlockMode::Enter => {
                let pw = self.ui.password_input.as_bytes().to_vec();
                match encrypted::load(&pw, &self.data_path) {
                    Ok(mut store) => {
                        store.migrate();
                        self.store = Some(store);
                        self.password = Some(pw);
                        let _ = self.save_store();
                        self.ui.password_input.clear();
                        self.ui.screen = Screen::Main;
                        self.ui.clear_status();
                    }
                    Err(_) => {
                        self.ui.set_status("密码错误");
                        self.ui.password_input.clear();
                    }
                }
            }
        }
    }

    // ========== 主界面 ==========

    fn handle_main_key(&mut self, key: KeyEvent) {
        let total_lines = self.count_main_lines();
        match key.code {
            KeyCode::Up => {
                if self.ui.selected_index > 0 {
                    self.ui.selected_index -= 1;
                }
            }
            KeyCode::Down => {
                if self.ui.selected_index + 1 < total_lines {
                    self.ui.selected_index += 1;
                }
            }
            KeyCode::Enter => {
                self.handle_main_enter();
            }
            KeyCode::Char('r') => {
                self.trigger_balance_refresh();
            }
            KeyCode::Char('s') => {
                self.ui.screen = Screen::Dex;
                self.ui.clear_status();
            }
            _ => {}
        }
    }

    /// 计算主界面可选行数
    fn count_main_lines(&self) -> usize {
        let store = match &self.store {
            Some(s) => s,
            None => return 2, // 空白行 + 添加钱包
        };
        let mut count = 0;
        for w in store.wallets.iter().filter(|w| !w.hidden) {
            count += 1; // 钱包行
            match &w.wallet_type {
                WalletType::Mnemonic {
                    eth_accounts,
                    sol_accounts,
                    ..
                } => {
                    count += eth_accounts.iter().filter(|a| !a.hidden).count();
                    count += sol_accounts.iter().filter(|a| !a.hidden).count();
                }
                WalletType::PrivateKey { .. } | WalletType::WatchOnly { .. } => {
                    count += 1; // 地址行
                }
                WalletType::Multisig { vaults, .. } => {
                    count += vaults.iter().filter(|v| !v.hidden).count();
                }
            }
        }
        count += 2; // 空白行 + 添加钱包
        count
    }

    /// 根据当前选中行判断选中了什么
    fn resolve_selection(&self) -> Option<SelectionTarget> {
        let store = self.store.as_ref()?;
        let visible: Vec<_> = store.wallets.iter().enumerate().filter(|(_, w)| !w.hidden).collect();

        let mut line = 0;
        for (wi, wallet) in &visible {
            if line == self.ui.selected_index {
                return Some(SelectionTarget::Wallet(*wi));
            }
            line += 1;

            match &wallet.wallet_type {
                WalletType::Mnemonic {
                    eth_accounts,
                    sol_accounts,
                    ..
                } => {
                    for (ai, acc) in eth_accounts.iter().enumerate() {
                        if acc.hidden {
                            continue;
                        }
                        if line == self.ui.selected_index {
                            return Some(SelectionTarget::MnemonicAddress {
                                wallet_index: *wi,
                                chain_type: ChainType::Ethereum,
                                account_index: ai,
                            });
                        }
                        line += 1;
                    }
                    for (ai, acc) in sol_accounts.iter().enumerate() {
                        if acc.hidden {
                            continue;
                        }
                        if line == self.ui.selected_index {
                            return Some(SelectionTarget::MnemonicAddress {
                                wallet_index: *wi,
                                chain_type: ChainType::Solana,
                                account_index: ai,
                            });
                        }
                        line += 1;
                    }
                }
                WalletType::PrivateKey { .. } => {
                    if line == self.ui.selected_index {
                        return Some(SelectionTarget::PrivateKeyAddress(*wi));
                    }
                    line += 1;
                }
                WalletType::WatchOnly { .. } => {
                    if line == self.ui.selected_index {
                        return Some(SelectionTarget::WatchAddress(*wi));
                    }
                    line += 1;
                }
                WalletType::Multisig { vaults, .. } => {
                    for (vi, vault) in vaults.iter().enumerate() {
                        if vault.hidden {
                            continue;
                        }
                        if line == self.ui.selected_index {
                            return Some(SelectionTarget::MultisigVault {
                                wallet_index: *wi,
                                vault_pos: vi,
                            });
                        }
                        line += 1;
                    }
                }
            }
        }

        // 最后两行：空白 + 添加钱包
        line += 1; // 空白行
        if line == self.ui.selected_index {
            return Some(SelectionTarget::AddWallet);
        }

        None
    }

    fn handle_main_enter(&mut self) {
        let target = match self.resolve_selection() {
            Some(t) => t,
            None => return,
        };

        match target {
            SelectionTarget::Wallet(wi) => {
                // 多签钱包使用专用的 ActionContext
                let is_multisig = self
                    .store
                    .as_ref()
                    .and_then(|s| s.wallets.get(wi))
                    .map(|w| matches!(w.wallet_type, WalletType::Multisig { .. }))
                    .unwrap_or(false);
                if is_multisig {
                    self.ui
                        .enter_action_menu(ActionContext::MultisigWallet { wallet_index: wi });
                } else {
                    self.ui
                        .enter_action_menu(ActionContext::Wallet { wallet_index: wi });
                }
            }
            SelectionTarget::MnemonicAddress {
                wallet_index,
                chain_type,
                account_index,
            } => {
                // SOL 地址检测 Vote/Stake 账户，直接进详情页
                if chain_type == ChainType::Solana
                    && let Some(address) = self.get_sol_address(wallet_index, account_index)
                {
                    let owner = self
                        .balance_cache
                        .get(&address)
                        .and_then(|p| p.account_owner.as_deref())
                        .unwrap_or("");
                    if owner == crate::chain::solana::VOTE_PROGRAM {
                        self.enter_vote_detail(wallet_index, account_index, &address);
                        return;
                    }
                    if owner == crate::chain::solana::STAKE_PROGRAM {
                        self.enter_stake_detail(wallet_index, account_index, &address);
                        return;
                    }
                }
                self.ui.enter_action_menu(ActionContext::MnemonicAddress {
                    wallet_index,
                    chain_type,
                    account_index,
                });
            }
            SelectionTarget::PrivateKeyAddress(wi) => {
                let ct = self.store.as_ref()
                    .and_then(|s| s.wallets.get(wi))
                    .and_then(|w| match &w.wallet_type {
                        WalletType::PrivateKey { chain_type, .. } => Some(chain_type.clone()),
                        _ => None,
                    })
                    .unwrap_or(ChainType::Ethereum);
                // SOL 私钥钱包检测 Vote/Stake 账户
                if ct == ChainType::Solana
                    && let Some(address) = self.get_sol_address(wi, 0) {
                        let owner = self
                            .balance_cache
                            .get(&address)
                            .and_then(|p| p.account_owner.as_deref())
                            .unwrap_or("");
                        if owner == crate::chain::solana::VOTE_PROGRAM {
                            self.enter_vote_detail(wi, 0, &address);
                            return;
                        }
                        if owner == crate::chain::solana::STAKE_PROGRAM {
                            self.enter_stake_detail(wi, 0, &address);
                            return;
                        }
                    }
                self.ui
                    .enter_action_menu(ActionContext::PrivateKeyAddress { wallet_index: wi, chain_type: ct });
            }
            SelectionTarget::WatchAddress(wi) => {
                self.ui
                    .enter_action_menu(ActionContext::WatchAddress { wallet_index: wi });
            }
            SelectionTarget::MultisigVault {
                wallet_index,
                vault_pos,
            } => {
                self.enter_multisig_from_wallet(wallet_index, vault_pos);
            }
            SelectionTarget::AddWallet => {
                self.ui.enter_add_wallet();
            }
        }
    }

    /// 从主页进入多签详情（通过 wallet_index 和 vault_pos）
    fn enter_multisig_from_wallet(&mut self, wallet_index: usize, vault_pos: usize) {
        let store = match &self.store {
            Some(s) => s,
            None => return,
        };
        let wallet = match store.wallets.get(wallet_index) {
            Some(w) => w,
            None => return,
        };
        let (multisig_address, rpc_url, chain_id, vaults) = match &wallet.wallet_type {
            WalletType::Multisig {
                multisig_address,
                rpc_url,
                chain_id,
                vaults,
                ..
            } => (multisig_address.clone(), rpc_url.clone(), chain_id.clone(), vaults),
            _ => return,
        };

        // 找到选中的 vault（按可见 vault 的位置）
        let visible_vaults: Vec<_> = vaults.iter().filter(|v| !v.hidden).collect();
        if let Some(vault) = visible_vaults.get(vault_pos) {
            self.ui.ms_current_vault_index = vault.vault_index;
            self.ui.ms_current_vault_address = vault.address.clone();
            self.ui.ms_current_vault_label = vault.label.clone();
        }

        self.ui.ms_current_wallet_index = wallet_index;
        self.ui.ms_selected_chain_id = chain_id;
        self.ui.ms_selected_rpc_url = rpc_url.clone();
        self.ui.screen = Screen::Multisig;
        self.ui.ms_step = MultisigStep::ViewDetail;
        self.ui.set_status("正在加载...");

        let tx = self.bg_tx.clone();
        self.runtime.spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap();

            match multisig::squads::fetch_multisig(&client, &rpc_url, &multisig_address).await {
                Ok(info) => {
                    let _ = tx.send(BgMessage::MultisigFetched(info));
                }
                Err(err) => {
                    let _ = tx.send(BgMessage::MultisigFetchError(err));
                }
            }
        });
    }

    // ========== 添加钱包 ==========

    fn handle_add_wallet_key(&mut self, key: KeyEvent) {
        match self.ui.add_wallet_step {
            AddWalletStep::SelectType => self.handle_select_type_key(key),
            AddWalletStep::InputName
            | AddWalletStep::InputMnemonic
            | AddWalletStep::InputPrivateKey
            | AddWalletStep::InputAddress => self.handle_text_input_key(key),
            AddWalletStep::ShowMnemonic => self.handle_show_mnemonic_key(key),
            AddWalletStep::SelectChainType => self.handle_select_chain_key(key),
            AddWalletStep::SelectHiddenItem => {
                if key.code == KeyCode::Esc {
                    self.ui.back_to_main();
                }
            }
        }
    }

    fn handle_select_type_key(&mut self, key: KeyEvent) {
        let options = AddWalletOption::all();
        match key.code {
            KeyCode::Up => {
                if self.ui.add_wallet_selected > 0 {
                    self.ui.add_wallet_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.ui.add_wallet_selected + 1 < options.len() {
                    self.ui.add_wallet_selected += 1;
                }
            }
            KeyCode::Enter => {
                let selected = options[self.ui.add_wallet_selected].clone();
                self.ui.add_wallet_option = Some(selected.clone());
                match selected {
                    AddWalletOption::CreateMnemonic | AddWalletOption::ImportMnemonic => {
                        self.ui.add_wallet_step = AddWalletStep::InputName;
                        self.ui.input_buffer.clear();
                    }
                    AddWalletOption::ImportPrivateKey => {
                        self.ui.add_wallet_step = AddWalletStep::SelectChainType;
                        self.ui.chain_type_selected = 0;
                    }
                    AddWalletOption::ImportWatchOnly => {
                        self.ui.add_wallet_step = AddWalletStep::SelectChainType;
                        self.ui.chain_type_selected = 0;
                    }
                    AddWalletOption::CreateMultisig => {
                        self.ui.ms_create_use_seed = false;
                        self.ui.ms_create_preset_creator = None;
                        self.enter_chain_select(MsChainSelectPurpose::Create);
                    }
                    AddWalletOption::CreateMultisigWithSeed => {
                        self.ui.ms_create_use_seed = true;
                        self.ui.ms_create_preset_creator = None;
                        self.enter_chain_select(MsChainSelectPurpose::Create);
                    }
                    AddWalletOption::ImportMultisig => {
                        self.enter_chain_select(MsChainSelectPurpose::Import);
                    }
                    AddWalletOption::RestoreHiddenWallet | AddWalletOption::RestoreHiddenAddress => {
                        self.handle_restore_hidden(&selected);
                    }
                }
            }
            KeyCode::Esc => {
                self.ui.back_to_main();
            }
            _ => {}
        }
    }

    fn handle_select_chain_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                if self.ui.chain_type_selected > 0 {
                    self.ui.chain_type_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.ui.chain_type_selected < 1 {
                    self.ui.chain_type_selected += 1;
                }
            }
            KeyCode::Enter => {
                if self.ui.input_purpose == Some(InputPurpose::AddMnemonicAddress) {
                    // 添加助记词地址：选完链后直接派生
                    let chain_type = if self.ui.chain_type_selected == 0 {
                        ChainType::Ethereum
                    } else {
                        ChainType::Solana
                    };
                    let wallet_index = self.ui.transfer_wallet_index;
                    self.ui.input_purpose = None;
                    self.add_derived_address(wallet_index, chain_type);
                } else {
                    // 导入私钥/观察钱包：选完链类型后输入名称
                    self.ui.add_wallet_step = AddWalletStep::InputName;
                    self.ui.input_buffer.clear();
                }
            }
            KeyCode::Esc => {
                if self.ui.input_purpose == Some(InputPurpose::AddMnemonicAddress) {
                    self.ui.input_purpose = None;
                    self.ui.back_to_main();
                } else {
                    self.ui.add_wallet_step = AddWalletStep::SelectType;
                }
            }
            _ => {}
        }
    }

    fn handle_text_input_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) => {
                self.ui.clear_status();
                self.ui.input_buffer.push(c);
            }
            KeyCode::Backspace => {
                self.ui.clear_status();
                self.ui.input_buffer.pop();
            }
            KeyCode::Enter => {
                if self.ui.input_buffer.is_empty() {
                    self.ui.set_status("不能为空");
                    return;
                }
                self.process_text_input();
            }
            KeyCode::Esc => {
                // 编辑标签时按 Esc 返回
                if self.ui.input_purpose == Some(InputPurpose::EditLabel) {
                    self.ui.input_purpose = None;
                    self.ui.back_to_main();
                    return;
                }
                if self.ui.input_purpose == Some(InputPurpose::EditVaultLabel) {
                    self.ui.input_purpose = None;
                    self.ui.screen = Screen::Multisig;
                    self.ui.ms_step = MultisigStep::ViewDetail;
                    return;
                }
                match self.ui.add_wallet_step {
                    AddWalletStep::InputName => {
                        // 如果是私钥/观察钱包，返回链选择；否则返回类型选择
                        let option = self.ui.add_wallet_option.clone();
                        match option {
                            Some(AddWalletOption::ImportPrivateKey)
                            | Some(AddWalletOption::ImportWatchOnly) => {
                                self.ui.add_wallet_step = AddWalletStep::SelectChainType;
                            }
                            _ => {
                                self.ui.add_wallet_step = AddWalletStep::SelectType;
                            }
                        }
                    }
                    AddWalletStep::InputMnemonic => {
                        self.ui.add_wallet_step = AddWalletStep::InputName;
                        self.ui.input_buffer = self.ui.wallet_name_buffer.clone();
                    }
                    AddWalletStep::InputPrivateKey | AddWalletStep::InputAddress => {
                        self.ui.add_wallet_step = AddWalletStep::InputName;
                        self.ui.input_buffer = self.ui.wallet_name_buffer.clone();
                    }
                    _ => {
                        self.ui.back_to_main();
                    }
                }
                self.ui.clear_status();
            }
            _ => {}
        }
    }

    fn process_text_input(&mut self) {
        // 如果是编辑标签，走专门的处理逻辑
        if self.ui.input_purpose == Some(InputPurpose::EditLabel) {
            self.save_edited_label();
            return;
        }
        if self.ui.input_purpose == Some(InputPurpose::EditVaultLabel) {
            self.save_vault_label();
            return;
        }

        match self.ui.add_wallet_step {
            AddWalletStep::InputName => {
                self.ui.wallet_name_buffer = self.ui.input_buffer.clone();
                self.ui.input_buffer.clear();

                let option = self.ui.add_wallet_option.clone();
                match option {
                    Some(AddWalletOption::CreateMnemonic) => {
                        // 生成助记词
                        match mnemonic::generate_mnemonic() {
                            Ok(phrase) => {
                                self.ui.mnemonic_buffer = phrase;
                                self.ui.add_wallet_step = AddWalletStep::ShowMnemonic;
                                self.ui.screen = Screen::ShowMnemonic;
                            }
                            Err(e) => {
                                self.ui.set_status(format!("生成助记词失败: {e}"));
                            }
                        }
                    }
                    Some(AddWalletOption::ImportMnemonic) => {
                        self.ui.add_wallet_step = AddWalletStep::InputMnemonic;
                    }
                    Some(AddWalletOption::ImportPrivateKey) => {
                        self.ui.add_wallet_step = AddWalletStep::InputPrivateKey;
                    }
                    Some(AddWalletOption::ImportWatchOnly) => {
                        self.ui.add_wallet_step = AddWalletStep::InputAddress;
                    }
                    _ => {}
                }
            }
            AddWalletStep::InputMnemonic => {
                let phrase = self.ui.input_buffer.trim().to_string();
                if mnemonic::validate_mnemonic(&phrase).is_err() {
                    self.ui.set_status("无效的助记词");
                    return;
                }
                self.ui.mnemonic_buffer = phrase;
                self.create_mnemonic_wallet();
            }
            AddWalletStep::InputPrivateKey => {
                let pk = self.ui.input_buffer.trim().to_string();
                self.create_private_key_wallet(&pk);
            }
            AddWalletStep::InputAddress => {
                let addr = self.ui.input_buffer.trim().to_string();
                self.create_watch_wallet(&addr);
            }
            _ => {}
        }
    }

    fn handle_show_mnemonic_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Enter {
            self.create_mnemonic_wallet();
        } else if key.code == KeyCode::Esc {
            self.ui.add_wallet_step = AddWalletStep::InputName;
            self.ui.input_buffer = self.ui.wallet_name_buffer.clone();
            self.ui.screen = Screen::AddWallet;
        }
    }

    fn create_mnemonic_wallet(&mut self) {
        let phrase = &self.ui.mnemonic_buffer;
        let mut seed = match mnemonic::mnemonic_to_seed(phrase, "") {
            Ok(s) => s,
            Err(e) => {
                self.ui.set_status(format!("种子生成失败: {e}"));
                return;
            }
        };

        // 派生第一个 ETH 和 SOL 地址
        let eth_addr = match eth_keys::derive_eth_address(&seed, 0) {
            Ok(a) => a,
            Err(e) => {
                self.ui.set_status(format!("ETH 地址派生失败: {e}"));
                return;
            }
        };
        let sol_addr = match sol_keys::derive_sol_address(&seed, 0) {
            Ok(a) => a,
            Err(e) => {
                self.ui.set_status(format!("SOL 地址派生失败: {e}"));
                return;
            }
        };

        // 用主密码加密助记词（双层保护）
        let encrypted_mnemonic = match &self.password {
            Some(pw) => {
                let (salt, nonce, ct) =
                    match crate::crypto::encryption::encrypt(phrase.as_bytes(), pw) {
                        Ok(v) => v,
                        Err(e) => {
                            self.ui.set_status(format!("加密失败: {e}"));
                            return;
                        }
                    };
                // 编码为 hex: salt:nonce:ciphertext
                format!(
                    "{}:{}:{}",
                    hex::encode(&salt),
                    hex::encode(&nonce),
                    hex::encode(&ct)
                )
            }
            None => {
                self.ui.set_status("密码未设置");
                return;
            }
        };

        let wallet = Wallet {
            id: uuid::Uuid::new_v4().to_string(),
            name: self.ui.wallet_name_buffer.clone(),
            wallet_type: WalletType::Mnemonic {
                encrypted_mnemonic,
                eth_accounts: vec![DerivedAccount {
                    derivation_index: 0,
                    address: eth_addr,
                    label: None,
                    hidden: false,
                }],
                sol_accounts: vec![DerivedAccount {
                    derivation_index: 0,
                    address: sol_addr,
                    label: None,
                    hidden: false,
                }],
                next_eth_index: 1,
                next_sol_index: 1,
            },
            sort_order: self.next_sort_order(),
            hidden: false,
            created_at: chrono::Utc::now().timestamp(),
        };

        // 清零敏感数据
        seed.clear_sensitive();
        self.ui.mnemonic_buffer.clear_sensitive();
        self.add_wallet(wallet);
    }

    fn create_private_key_wallet(&mut self, pk: &str) {
        let chain_type = if self.ui.chain_type_selected == 0 {
            ChainType::Ethereum
        } else {
            ChainType::Solana
        };

        let address = match &chain_type {
            ChainType::Ethereum => match eth_keys::hex_private_key_to_address(pk) {
                Ok(a) => a,
                Err(e) => {
                    self.ui.set_status(format!("无效的 ETH 私钥: {e}"));
                    return;
                }
            },
            ChainType::Solana => match sol_keys::bs58_private_key_to_address(pk) {
                Ok(a) => a,
                Err(e) => {
                    self.ui.set_status(format!("无效的 SOL 私钥: {e}"));
                    return;
                }
            },
        };

        // 加密私钥
        let encrypted_pk = match &self.password {
            Some(pw) => {
                let (salt, nonce, ct) =
                    match crate::crypto::encryption::encrypt(pk.as_bytes(), pw) {
                        Ok(v) => v,
                        Err(e) => {
                            self.ui.set_status(format!("加密失败: {e}"));
                            return;
                        }
                    };
                format!(
                    "{}:{}:{}",
                    hex::encode(&salt),
                    hex::encode(&nonce),
                    hex::encode(&ct)
                )
            }
            None => {
                self.ui.set_status("密码未设置");
                return;
            }
        };

        let wallet = Wallet {
            id: uuid::Uuid::new_v4().to_string(),
            name: self.ui.wallet_name_buffer.clone(),
            wallet_type: WalletType::PrivateKey {
                chain_type,
                encrypted_private_key: encrypted_pk,
                address,
                label: None,
                hidden: false,
            },
            sort_order: self.next_sort_order(),
            hidden: false,
            created_at: chrono::Utc::now().timestamp(),
        };

        self.add_wallet(wallet);
    }

    fn create_watch_wallet(&mut self, addr: &str) {
        let chain_type = if self.ui.chain_type_selected == 0 {
            ChainType::Ethereum
        } else {
            ChainType::Solana
        };

        // 基本地址格式验证
        match &chain_type {
            ChainType::Ethereum => {
                if !addr.starts_with("0x")
                    || addr.len() != 42
                    || !addr[2..].chars().all(|c| c.is_ascii_hexdigit())
                {
                    self.ui.set_status("无效的 ETH 地址（应以 0x 开头，42 位十六进制字符）");
                    return;
                }
            }
            ChainType::Solana => {
                if bs58::decode(addr).into_vec().is_err() {
                    self.ui.set_status("无效的 SOL 地址（应为 Base58 编码）");
                    return;
                }
            }
        }

        let wallet = Wallet {
            id: uuid::Uuid::new_v4().to_string(),
            name: self.ui.wallet_name_buffer.clone(),
            wallet_type: WalletType::WatchOnly {
                chain_type,
                address: addr.to_string(),
                label: None,
                source: WatchOnlySource::Manual,
            },
            sort_order: self.next_sort_order(),
            hidden: false,
            created_at: chrono::Utc::now().timestamp(),
        };

        self.add_wallet(wallet);
    }

    fn add_wallet(&mut self, wallet: Wallet) {
        if let Some(ref mut store) = self.store {
            store.wallets.push(wallet);
            if let Err(e) = self.save_store() {
                self.ui.set_status(format!("保存失败: {e}"));
                return;
            }
        }
        self.ui.back_to_main();
        self.ui.set_status("钱包添加成功");
    }

    fn next_sort_order(&self) -> u32 {
        self.store
            .as_ref()
            .map(|s| s.wallets.len() as u32)
            .unwrap_or(0)
    }

    // ========== 操作菜单 ==========

    fn handle_action_menu_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                if self.ui.action_selected > 0 {
                    self.ui.action_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.ui.action_selected + 1 < self.ui.action_items.len() {
                    self.ui.action_selected += 1;
                }
            }
            KeyCode::Enter => {
                self.execute_action();
            }
            KeyCode::Esc => {
                self.ui.back_to_main();
            }
            _ => {}
        }
    }

    fn execute_action(&mut self) {
        let action = match self.ui.action_items.get(self.ui.action_selected) {
            Some(a) => a.clone(),
            None => return,
        };
        let context = match self.ui.action_context.clone() {
            Some(c) => c,
            None => return,
        };

        match action {
            ActionItem::Transfer => {
                self.start_transfer();
            }
            ActionItem::AddAddress => {
                if let ActionContext::Wallet { wallet_index } = context {
                    // 进入链类型选择
                    self.ui.transfer_wallet_index = wallet_index; // 复用字段暂存 wallet_index
                    self.ui.input_purpose = Some(InputPurpose::AddMnemonicAddress);
                    self.ui.chain_type_selected = 0;
                    self.ui.add_wallet_step = AddWalletStep::SelectChainType;
                    self.ui.screen = Screen::AddWallet;
                }
            }
            ActionItem::EditName | ActionItem::EditAddressLabel => {
                self.ui.input_buffer = self.get_current_label(&Some(context.clone())).unwrap_or_default();
                self.ui.input_purpose = Some(InputPurpose::EditLabel);
                self.ui.add_wallet_step = AddWalletStep::InputName;
                self.ui.screen = Screen::TextInput;
            }
            ActionItem::HideWallet => {
                let wi = match context {
                    ActionContext::Wallet { wallet_index } => wallet_index,
                    ActionContext::PrivateKeyAddress { wallet_index, .. } => wallet_index,
                    ActionContext::MultisigWallet { wallet_index } => wallet_index,
                    _ => return,
                };
                self.toggle_wallet_hidden(wi, true);
            }
            ActionItem::HideAddress => {
                match context {
                    ActionContext::MnemonicAddress {
                        wallet_index,
                        chain_type,
                        account_index,
                    } => {
                        self.toggle_address_hidden(wallet_index, &chain_type, account_index, true);
                    }
                    ActionContext::MultisigVault {
                        wallet_index,
                        vault_pos,
                    } => {
                        self.toggle_vault_hidden(wallet_index, vault_pos, true);
                    }
                    _ => {}
                }
            }
            ActionItem::DeleteWatchWallet => {
                if let ActionContext::WatchAddress { wallet_index } = context {
                    self.delete_wallet(wallet_index);
                }
            }
            ActionItem::MoveUp | ActionItem::MoveDown => {
                let wi = match context {
                    ActionContext::Wallet { wallet_index } => wallet_index,
                    ActionContext::MultisigWallet { wallet_index } => wallet_index,
                    _ => return,
                };
                self.move_wallet(wi, action == ActionItem::MoveUp);
            }
            ActionItem::CreateMultisig | ActionItem::CreateMultisigWithSeed => {
                self.ui.ms_create_use_seed = matches!(action, ActionItem::CreateMultisigWithSeed);
                // 从地址菜单触发时，记住当前地址作为创建者
                let creator_addr = match &context {
                    ActionContext::MnemonicAddress { wallet_index, account_index, .. } => {
                        self.get_sol_address(*wallet_index, *account_index)
                    }
                    ActionContext::PrivateKeyAddress { wallet_index, .. } => {
                        self.get_sol_address(*wallet_index, 0)
                    }
                    _ => None,
                };
                self.ui.ms_create_preset_creator = creator_addr;
                self.enter_chain_select(MsChainSelectPurpose::Create);
            }
            ActionItem::AddVault => {
                if let ActionContext::MultisigWallet { wallet_index } = context {
                    self.add_vault_to_multisig(wallet_index);
                }
            }
            ActionItem::DeleteMultisig => {
                if let ActionContext::MultisigWallet { wallet_index } = context {
                    self.ui.pending_delete_wallet = Some(wallet_index);
                    self.ui.delete_confirm_password.clear();
                    self.ui.screen = Screen::ConfirmDelete;
                    self.ui.clear_status();
                }
            }
            ActionItem::CreateVoteAccount => {
                match context {
                    ActionContext::MnemonicAddress { wallet_index, account_index, .. } => {
                        self.enter_create_vote(wallet_index, account_index);
                    }
                    ActionContext::PrivateKeyAddress { wallet_index, .. } => {
                        self.enter_create_vote(wallet_index, 0);
                    }
                    _ => {}
                }
            }
            ActionItem::CreateStakeAccount => {
                match context {
                    ActionContext::MnemonicAddress { wallet_index, account_index, .. } => {
                        self.enter_create_stake(wallet_index, account_index);
                    }
                    ActionContext::PrivateKeyAddress { wallet_index, .. } => {
                        self.enter_create_stake(wallet_index, 0);
                    }
                    _ => {}
                }
            }
        }
    }

    fn add_derived_address(&mut self, wallet_index: usize, chain_type: ChainType) {
        // 先提取加密助记词和索引（只读借用）
        let (encrypted_mnemonic, eth_idx, sol_idx) = {
            let store = match &self.store {
                Some(s) => s,
                None => return,
            };
            let wallet = match store.wallets.get(wallet_index) {
                Some(w) => w,
                None => return,
            };
            match &wallet.wallet_type {
                WalletType::Mnemonic {
                    encrypted_mnemonic,
                    next_eth_index,
                    next_sol_index,
                    ..
                } => (encrypted_mnemonic.clone(), *next_eth_index, *next_sol_index),
                _ => return,
            }
        };

        // 解密助记词（不持有 store 借用）
        let phrase = match self.decrypt_inner_secret(&encrypted_mnemonic) {
            Some(p) => p,
            None => {
                self.ui.set_status("解密助记词失败");
                self.ui.back_to_main();
                return;
            }
        };

        let mut seed = match mnemonic::mnemonic_to_seed(&phrase, "") {
            Ok(s) => s,
            Err(e) => {
                self.ui.set_status(format!("种子生成失败: {e}"));
                self.ui.back_to_main();
                return;
            }
        };

        // 按选择的链类型派生地址
        let (eth_addr, sol_addr) = match chain_type {
            ChainType::Ethereum => (eth_keys::derive_eth_address(&seed, eth_idx).ok(), None),
            ChainType::Solana => (None, sol_keys::derive_sol_address(&seed, sol_idx).ok()),
        };
        seed.clear_sensitive();

        // 修改 store（可变借用）
        if let Some(ref mut store) = self.store
            && let Some(wallet) = store.wallets.get_mut(wallet_index)
                && let WalletType::Mnemonic {
                    ref mut eth_accounts,
                    ref mut sol_accounts,
                    ref mut next_eth_index,
                    ref mut next_sol_index,
                    ..
                } = wallet.wallet_type
                {
                    if let Some(addr) = eth_addr {
                        eth_accounts.push(DerivedAccount {
                            derivation_index: eth_idx,
                            address: addr,
                            label: None,
                            hidden: false,
                        });
                        *next_eth_index = eth_idx + 1;
                    }
                    if let Some(addr) = sol_addr {
                        sol_accounts.push(DerivedAccount {
                            derivation_index: sol_idx,
                            address: addr,
                            label: None,
                            hidden: false,
                        });
                        *next_sol_index = sol_idx + 1;
                    }
                }

        if let Err(e) = self.save_store_inner() {
            self.ui.set_status(format!("保存失败: {e}"));
        } else {
            self.ui.set_status("地址添加成功");
        }
        self.ui.back_to_main();
    }

    fn toggle_wallet_hidden(&mut self, wallet_index: usize, hidden: bool) {
        if let Some(ref mut store) = self.store
            && let Some(w) = store.wallets.get_mut(wallet_index) {
                w.hidden = hidden;
                let _ = self.save_store_inner();
            }
        self.ui.back_to_main();
    }

    fn toggle_address_hidden(
        &mut self,
        wallet_index: usize,
        chain_type: &ChainType,
        account_index: usize,
        hidden: bool,
    ) {
        if let Some(ref mut store) = self.store
            && let Some(w) = store.wallets.get_mut(wallet_index) {
                if let WalletType::Mnemonic {
                    ref mut eth_accounts,
                    ref mut sol_accounts,
                    ..
                } = w.wallet_type
                {
                    let accounts = match chain_type {
                        ChainType::Ethereum => eth_accounts,
                        ChainType::Solana => sol_accounts,
                    };
                    if let Some(acc) = accounts.get_mut(account_index) {
                        acc.hidden = hidden;
                    }
                }
                let _ = self.save_store_inner();
            }
        self.ui.back_to_main();
    }

    fn move_wallet(&mut self, wallet_index: usize, up: bool) {
        if let Some(ref mut store) = self.store {
            let len = store.wallets.len();
            if up && wallet_index > 0 {
                store.wallets.swap(wallet_index, wallet_index - 1);
            } else if !up && wallet_index + 1 < len {
                store.wallets.swap(wallet_index, wallet_index + 1);
            }
            // 更新 sort_order
            for (i, w) in store.wallets.iter_mut().enumerate() {
                w.sort_order = i as u32;
            }
            let _ = self.save_store_inner();
        }
        self.ui.back_to_main();
    }

    /// 获取当前 context 对应的备注/名称
    fn get_current_label(&self, context: &Option<ActionContext>) -> Option<String> {
        let store = self.store.as_ref()?;
        match context {
            Some(ActionContext::Wallet { wallet_index })
            | Some(ActionContext::PrivateKeyAddress { wallet_index, .. })
            | Some(ActionContext::MultisigWallet { wallet_index }) => {
                store.wallets.get(*wallet_index).map(|w| w.name.clone())
            }
            Some(ActionContext::MnemonicAddress {
                wallet_index,
                chain_type,
                account_index,
            }) => {
                let w = store.wallets.get(*wallet_index)?;
                if let WalletType::Mnemonic { eth_accounts, sol_accounts, .. } = &w.wallet_type {
                    let accounts = match chain_type {
                        ChainType::Ethereum => eth_accounts,
                        ChainType::Solana => sol_accounts,
                    };
                    accounts.get(*account_index).and_then(|a| a.label.clone())
                } else {
                    None
                }
            }
            Some(ActionContext::WatchAddress { wallet_index }) => {
                let w = store.wallets.get(*wallet_index)?;
                if let WalletType::WatchOnly { label, .. } = &w.wallet_type {
                    label.clone()
                } else {
                    None
                }
            }
            Some(ActionContext::MultisigVault { wallet_index, vault_pos }) => {
                let w = store.wallets.get(*wallet_index)?;
                if let WalletType::Multisig { vaults, .. } = &w.wallet_type {
                    vaults.get(*vault_pos).and_then(|v| v.label.clone())
                } else {
                    None
                }
            }
            None => None,
        }
    }

    fn save_edited_label(&mut self) {
        let new_label = self.ui.input_buffer.trim().to_string();
        let context = self.ui.action_context.clone();
        self.ui.input_purpose = None;

        if let Some(ref mut store) = self.store {
            match context {
                Some(ActionContext::Wallet { wallet_index }) => {
                    if let Some(w) = store.wallets.get_mut(wallet_index) {
                        w.name = new_label;
                    }
                }
                Some(ActionContext::MnemonicAddress {
                    wallet_index,
                    chain_type,
                    account_index,
                }) => {
                    if let Some(w) = store.wallets.get_mut(wallet_index)
                        && let WalletType::Mnemonic {
                            ref mut eth_accounts,
                            ref mut sol_accounts,
                            ..
                        } = w.wallet_type
                        {
                            let accounts = match chain_type {
                                ChainType::Ethereum => eth_accounts,
                                ChainType::Solana => sol_accounts,
                            };
                            if let Some(acc) = accounts.get_mut(account_index) {
                                acc.label = Some(new_label);
                            }
                        }
                }
                Some(ActionContext::PrivateKeyAddress { wallet_index, .. }) => {
                    if let Some(w) = store.wallets.get_mut(wallet_index) {
                        w.name = new_label;
                    }
                }
                Some(ActionContext::WatchAddress { wallet_index }) => {
                    if let Some(w) = store.wallets.get_mut(wallet_index)
                        && let WalletType::WatchOnly { ref mut label, .. } = w.wallet_type {
                            *label = Some(new_label);
                        }
                }
                Some(ActionContext::MultisigWallet { wallet_index }) => {
                    if let Some(w) = store.wallets.get_mut(wallet_index) {
                        w.name = new_label;
                    }
                }
                Some(ActionContext::MultisigVault {
                    wallet_index,
                    vault_pos,
                }) => {
                    if let Some(w) = store.wallets.get_mut(wallet_index)
                        && let WalletType::Multisig { ref mut vaults, .. } = w.wallet_type
                        && let Some(v) = vaults.get_mut(vault_pos)
                    {
                        v.label = Some(new_label);
                    }
                }
                _ => {}
            }
            let _ = self.save_store_inner();
        }
        self.ui.back_to_main();
        self.ui.set_status("备注已更新");
    }

    /// 从多签详情页保存 vault 备注
    fn save_vault_label(&mut self) {
        let new_label = self.ui.input_buffer.trim().to_string();
        self.ui.input_purpose = None;

        let wi = self.ui.ms_current_wallet_index;
        let vault_idx = self.ui.ms_current_vault_index;

        if let Some(ref mut store) = self.store {
            if let Some(w) = store.wallets.get_mut(wi)
                && let WalletType::Multisig { ref mut vaults, .. } = w.wallet_type
                && let Some(v) = vaults.iter_mut().find(|v| v.vault_index == vault_idx)
            {
                let label = if new_label.is_empty() { None } else { Some(new_label.clone()) };
                v.label = label.clone();
                self.ui.ms_current_vault_label = label;
            }
            let _ = self.save_store_inner();
        }
        self.ui.screen = Screen::Multisig;
        self.ui.ms_step = MultisigStep::ViewDetail;
        self.ui.set_status("备注已更新");
    }

    fn delete_wallet(&mut self, wallet_index: usize) {
        if let Some(ref mut store) = self.store
            && wallet_index < store.wallets.len() {
                store.wallets.remove(wallet_index);
                let _ = self.save_store_inner();
            }
        self.ui.back_to_main();
    }

    fn render_confirm_delete(&self, frame: &mut ratatui::Frame) {
        use ratatui::layout::Alignment;
        use ratatui::widgets::{Block, Borders, Paragraph, Clear};
        use ratatui::style::{Color, Style, Modifier};
        use ratatui::text::{Line, Span};

        let area = frame.area();
        let popup_width = 50.min(area.width.saturating_sub(4));
        let popup_height = 7.min(area.height.saturating_sub(4));
        let x = (area.width.saturating_sub(popup_width)) / 2;
        let y = (area.height.saturating_sub(popup_height)) / 2;
        let popup_area = ratatui::layout::Rect::new(x, y, popup_width, popup_height);

        frame.render_widget(Clear, popup_area);

        let wallet_name = self.store.as_ref()
            .and_then(|s| self.ui.pending_delete_wallet.and_then(|i| s.wallets.get(i)))
            .map(|w| w.name.as_str())
            .unwrap_or("未知");

        let mask = "*".repeat(self.ui.delete_confirm_password.len());
        let status_line = self.ui.status_message.as_deref().unwrap_or("");

        let lines = vec![
            Line::from(Span::styled(
                format!("  确认删除: {wallet_name}"),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )),
            Line::from(format!("  密码: {mask}")),
            Line::from(""),
            Line::from(Span::styled(
                format!("  {status_line}"),
                Style::default().fg(Color::Yellow),
            )),
            Line::from(Span::styled(
                "  Enter 确认 | Esc 取消",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let block = Block::default()
            .title(" 删除钱包 ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red));

        frame.render_widget(Paragraph::new(lines).block(block), popup_area);
    }

    fn handle_confirm_delete_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Char(c) => {
                self.ui.clear_status();
                self.ui.delete_confirm_password.push(c);
            }
            KeyCode::Backspace => {
                self.ui.clear_status();
                self.ui.delete_confirm_password.pop();
            }
            KeyCode::Enter => {
                if self.ui.delete_confirm_password.is_empty() {
                    self.ui.set_status("请输入密码");
                    return;
                }
                // 验证密码
                let pw = match &self.password {
                    Some(pw) => pw.clone(),
                    None => {
                        self.ui.set_status("密码未设置");
                        return;
                    }
                };
                if self.ui.delete_confirm_password.as_bytes() != pw.as_slice() {
                    self.ui.set_status("密码错误");
                    self.ui.delete_confirm_password.clear();
                    return;
                }
                // 密码正确，执行删除
                if let Some(wallet_index) = self.ui.pending_delete_wallet.take() {
                    self.delete_wallet(wallet_index);
                    self.ui.set_status("钱包已删除");
                }
                self.ui.delete_confirm_password.clear();
            }
            KeyCode::Esc => {
                self.ui.pending_delete_wallet = None;
                self.ui.delete_confirm_password.clear();
                self.ui.back_to_main();
            }
            _ => {}
        }
    }

    fn handle_restore_hidden(&mut self, option: &AddWalletOption) {
        if let Some(ref mut store) = self.store {
            let mut restored = 0;
            match option {
                AddWalletOption::RestoreHiddenWallet => {
                    for w in &mut store.wallets {
                        if w.hidden {
                            w.hidden = false;
                            restored += 1;
                        }
                    }
                }
                AddWalletOption::RestoreHiddenAddress => {
                    for w in &mut store.wallets {
                        if let WalletType::Mnemonic {
                            ref mut eth_accounts,
                            ref mut sol_accounts,
                            ..
                        } = w.wallet_type
                        {
                            for acc in eth_accounts.iter_mut() {
                                if acc.hidden {
                                    acc.hidden = false;
                                    restored += 1;
                                }
                            }
                            for acc in sol_accounts.iter_mut() {
                                if acc.hidden {
                                    acc.hidden = false;
                                    restored += 1;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
            let _ = self.save_store_inner();
            self.ui.back_to_main();
            if restored > 0 {
                self.ui
                    .set_status(format!("已恢复 {restored} 个隐藏项目"));
            } else {
                self.ui.set_status("没有隐藏项目需要恢复");
            }
        }
    }

    // ========== 转账 ==========

    fn start_transfer(&mut self) {
        let context = match self.ui.action_context.clone() {
            Some(c) => c,
            None => return,
        };

        let (wallet_index, chain_type, address, label, account_index) = match &context {
            ActionContext::MnemonicAddress {
                wallet_index,
                chain_type,
                account_index,
            } => {
                let store = match &self.store {
                    Some(s) => s,
                    None => return,
                };
                let wallet = match store.wallets.get(*wallet_index) {
                    Some(w) => w,
                    None => return,
                };
                let (addr, lbl) = match (&wallet.wallet_type, chain_type) {
                    (
                        WalletType::Mnemonic {
                            eth_accounts, ..
                        },
                        ChainType::Ethereum,
                    ) => match eth_accounts.get(*account_index) {
                        Some(acc) => (acc.address.clone(), acc.label.clone()),
                        None => return,
                    },
                    (
                        WalletType::Mnemonic {
                            sol_accounts, ..
                        },
                        ChainType::Solana,
                    ) => match sol_accounts.get(*account_index) {
                        Some(acc) => (acc.address.clone(), acc.label.clone()),
                        None => return,
                    },
                    _ => return,
                };
                (*wallet_index, chain_type.clone(), addr, lbl, Some(*account_index))
            }
            ActionContext::PrivateKeyAddress { wallet_index, .. } => {
                let store = match &self.store {
                    Some(s) => s,
                    None => return,
                };
                let wallet = match store.wallets.get(*wallet_index) {
                    Some(w) => w,
                    None => return,
                };
                match &wallet.wallet_type {
                    WalletType::PrivateKey {
                        chain_type,
                        address,
                        label,
                        ..
                    } => (
                        *wallet_index,
                        chain_type.clone(),
                        address.clone(),
                        label.clone(),
                        None,
                    ),
                    _ => return,
                }
            }
            _ => return,
        };

        let assets = match chain_type {
            ChainType::Ethereum => {
                transfer::build_eth_assets(&self.config, &address, &self.balance_cache)
            }
            ChainType::Solana => {
                transfer::build_sol_assets(&self.config, &address, &self.balance_cache)
            }
        };

        if assets.is_empty() {
            self.ui.set_status("没有可转账的资产配置");
            self.ui.back_to_main();
            return;
        }

        self.ui.enter_transfer(
            address,
            label,
            chain_type,
            wallet_index,
            account_index,
            assets,
        );
    }

    fn handle_transfer_key(&mut self, key: KeyEvent) {
        match self.ui.transfer_step {
            TransferStep::SelectAsset => self.handle_transfer_select_asset(key),
            TransferStep::InputAddress => self.handle_transfer_input(key, TransferInputField::Address),
            TransferStep::InputAmount => self.handle_transfer_input(key, TransferInputField::Amount),
            TransferStep::Confirm => self.handle_transfer_confirm_input(key),
            TransferStep::Sending => {} // 忽略输入
            TransferStep::Result => {
                // 任意键返回主界面
                self.ui.back_to_main();
            }
        }
    }

    fn handle_transfer_select_asset(&mut self, key: KeyEvent) {
        let count = self.ui.transfer_assets.len();
        match key.code {
            KeyCode::Up => {
                if self.ui.transfer_asset_selected > 0 {
                    self.ui.transfer_asset_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.ui.transfer_asset_selected + 1 < count {
                    self.ui.transfer_asset_selected += 1;
                }
            }
            KeyCode::Enter => {
                if count > 0 {
                    self.ui.transfer_step = TransferStep::InputAddress;
                    self.ui.clear_status();
                }
            }
            KeyCode::Esc => {
                self.ui.back_to_main();
            }
            _ => {}
        }
    }

    fn handle_transfer_input(&mut self, key: KeyEvent, field: TransferInputField) {
        match key.code {
            KeyCode::Char(c) => {
                self.ui.clear_status();
                match field {
                    TransferInputField::Address => self.ui.transfer_to_address.push(c),
                    TransferInputField::Amount => self.ui.transfer_amount.push(c),
                }
            }
            KeyCode::Backspace => {
                self.ui.clear_status();
                match field {
                    TransferInputField::Address => { self.ui.transfer_to_address.pop(); }
                    TransferInputField::Amount => { self.ui.transfer_amount.pop(); }
                }
            }
            KeyCode::Enter => {
                let is_empty = match field {
                    TransferInputField::Address => self.ui.transfer_to_address.is_empty(),
                    TransferInputField::Amount => self.ui.transfer_amount.is_empty(),
                };
                if is_empty {
                    self.ui.set_status("不能为空");
                    return;
                }
                match field {
                    TransferInputField::Address => {
                        let valid = match self.ui.transfer_chain_type {
                            ChainType::Ethereum => {
                                self.ui.transfer_to_address.starts_with("0x")
                                    && self.ui.transfer_to_address.len() == 42
                                    && self.ui.transfer_to_address[2..]
                                        .chars()
                                        .all(|c| c.is_ascii_hexdigit())
                            }
                            ChainType::Solana => {
                                bs58::decode(&self.ui.transfer_to_address)
                                    .into_vec()
                                    .is_ok()
                            }
                        };
                        if !valid {
                            self.ui.set_status("无效的地址格式");
                            return;
                        }
                        self.ui.transfer_step = TransferStep::InputAmount;
                        self.ui.clear_status();
                    }
                    TransferInputField::Amount => {
                        let decimals = self
                            .ui
                            .transfer_assets
                            .get(self.ui.transfer_asset_selected)
                            .map(|a| a.decimals)
                            .unwrap_or(18);
                        if let Err(e) = transfer::parse_amount(&self.ui.transfer_amount, decimals) {
                            self.ui.set_status(e);
                            return;
                        }
                        self.ui.transfer_step = TransferStep::Confirm;
                        self.ui.transfer_confirm_password.clear();
                        self.ui.clear_status();
                    }
                }
            }
            KeyCode::Esc => {
                self.ui.clear_status();
                match field {
                    TransferInputField::Address => {
                        self.ui.transfer_step = TransferStep::SelectAsset;
                    }
                    TransferInputField::Amount => {
                        self.ui.transfer_step = TransferStep::InputAddress;
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_transfer_confirm_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) => {
                self.ui.clear_status();
                self.ui.transfer_confirm_password.push(c);
            }
            KeyCode::Backspace => {
                self.ui.clear_status();
                self.ui.transfer_confirm_password.pop();
            }
            KeyCode::Enter => {
                if self.ui.transfer_confirm_password.is_empty() {
                    self.ui.set_status("请输入密码");
                    return;
                }
                self.execute_transfer();
            }
            KeyCode::Esc => {
                self.ui.clear_status();
                self.ui.transfer_step = TransferStep::InputAmount;
            }
            _ => {}
        }
    }

    fn execute_transfer(&mut self) {
        // 验证密码
        let pw = match &self.password {
            Some(pw) => pw.clone(),
            None => {
                self.ui.set_status("密码未设置");
                return;
            }
        };
        if self.ui.transfer_confirm_password.as_bytes() != pw.as_slice() {
            self.ui.set_status("密码错误");
            self.ui.transfer_confirm_password.clear();
            return;
        }

        // 获取私钥
        let private_key = match self.get_transfer_private_key() {
            Some(pk) => pk,
            None => {
                self.ui.set_status("获取私钥失败");
                return;
            }
        };

        // 获取资产信息
        let asset = match self.ui.transfer_assets.get(self.ui.transfer_asset_selected) {
            Some(a) => a.clone(),
            None => {
                self.ui.set_status("资产信息缺失");
                return;
            }
        };

        // 解析金额
        let amount_raw = match transfer::parse_amount(&self.ui.transfer_amount, asset.decimals) {
            Ok(a) => a,
            Err(e) => {
                self.ui.set_status(e);
                return;
            }
        };

        let to_address = self.ui.transfer_to_address.clone();

        // 切换到发送中状态
        self.ui.transfer_step = TransferStep::Sending;
        self.ui.clear_status();

        // 后台执行转账
        let tx = self.bg_tx.clone();
        self.runtime.spawn(async move {
            let result = execute_transfer_async(private_key, asset, to_address, amount_raw).await;
            let _ = tx.send(match result {
                Ok(hash) => BgMessage::TransferComplete {
                    success: true,
                    message: hash,
                },
                Err(err) => BgMessage::TransferComplete {
                    success: false,
                    message: err,
                },
            });
        });
    }

    fn get_transfer_private_key(&mut self) -> Option<Vec<u8>> {
        // 先提取所需数据，释放 self.store 借用
        enum TransferKeySource {
            Mnemonic { encrypted: String, chain: ChainType, deriv_idx: u32 },
            PrivateKey { encrypted: String, chain: ChainType },
        }
        let source = {
            let store = self.store.as_ref()?;
            let wallet = store.wallets.get(self.ui.transfer_wallet_index)?;
            match &wallet.wallet_type {
                WalletType::Mnemonic {
                    encrypted_mnemonic,
                    eth_accounts,
                    sol_accounts,
                    ..
                } => {
                    let account_index = self.ui.transfer_account_index?;
                    let (chain, deriv_idx) = match self.ui.transfer_chain_type {
                        ChainType::Ethereum => {
                            (ChainType::Ethereum, eth_accounts.get(account_index)?.derivation_index)
                        }
                        ChainType::Solana => {
                            (ChainType::Solana, sol_accounts.get(account_index)?.derivation_index)
                        }
                    };
                    TransferKeySource::Mnemonic { encrypted: encrypted_mnemonic.clone(), chain, deriv_idx }
                }
                WalletType::PrivateKey {
                    encrypted_private_key,
                    chain_type,
                    ..
                } => TransferKeySource::PrivateKey { encrypted: encrypted_private_key.clone(), chain: chain_type.clone() },
                _ => return None,
            }
        };

        match source {
            TransferKeySource::Mnemonic { encrypted, chain, deriv_idx } => {
                let mut phrase = self.decrypt_inner_secret(&encrypted)?;
                let mut seed = mnemonic::mnemonic_to_seed(&phrase, "").ok()?;
                phrase.clear_sensitive();
                let result = match chain {
                    ChainType::Ethereum => eth_keys::derive_eth_private_key(&seed, deriv_idx).ok(),
                    ChainType::Solana => sol_keys::derive_sol_private_key(&seed, deriv_idx).ok(),
                };
                seed.clear_sensitive();
                result
            }
            TransferKeySource::PrivateKey { encrypted, chain } => {
                let mut pk_str = self.decrypt_inner_secret(&encrypted)?;
                let result = match chain {
                    ChainType::Ethereum => {
                        let clean = pk_str.strip_prefix("0x").unwrap_or(&pk_str);
                        hex::decode(clean).ok()
                    }
                    ChainType::Solana => {
                        let bytes = bs58::decode(&pk_str).into_vec().ok()?;
                        match bytes.len() {
                            64 => Some(bytes[..32].to_vec()),
                            32 => Some(bytes),
                            _ => None,
                        }
                    }
                };
                pk_str.clear_sensitive();
                result
            }
        }
    }

    // ========== 多签 ==========

    fn handle_multisig_key(&mut self, key: KeyEvent) {
        match self.ui.ms_step {
            MultisigStep::List => self.handle_ms_list_key(key),
            MultisigStep::SelectChain => self.handle_ms_select_chain_key(key),
            MultisigStep::InputAddress => self.handle_ms_input_address_key(key),
            MultisigStep::ViewDetail => self.handle_ms_detail_key(key),
            MultisigStep::ViewProposals => self.handle_ms_proposals_key(key),
            MultisigStep::ViewProposal => self.handle_ms_proposal_detail_key(key),
            MultisigStep::SelectProposalType => self.handle_ms_select_proposal_type_key(key),
            MultisigStep::InputTransferTo => self.handle_ms_text_input_key(key, MsInputField::TransferTo),
            MultisigStep::InputTransferAmount => self.handle_ms_text_input_key(key, MsInputField::TransferAmount),
            MultisigStep::InputUpgradeProgram => self.handle_ms_text_input_key(key, MsInputField::UpgradeProgram),
            MultisigStep::InputUpgradeBuffer => self.handle_ms_text_input_key(key, MsInputField::UpgradeBuffer),
            MultisigStep::SelectProgram => self.handle_ms_select_program_key(key),
            MultisigStep::SelectProgramInstruction => self.handle_ms_select_program_instruction_key(key),
            MultisigStep::InputProgramArgs => self.handle_ms_input_program_args_key(key),
            MultisigStep::SelectVoteStakeOp => self.handle_ms_select_vote_stake_op_key(key),
            MultisigStep::InputVoteStakeTarget => self.handle_ms_text_input_key(key, MsInputField::VsTarget),
            MultisigStep::InputVoteStakeParam => self.handle_ms_text_input_key(key, MsInputField::VsParam),
            MultisigStep::InputVoteStakeAmount => self.handle_ms_text_input_key(key, MsInputField::VsAmount),
            MultisigStep::ConfirmCreate | MultisigStep::ConfirmVote => {
                self.handle_ms_confirm_key(key);
            }
            MultisigStep::Submitting => {} // 忽略输入
            MultisigStep::Result => {
                // 如果有刚创建的多签待导入，直接导入
                if let Some((address, _tx_sig)) = self.ui.ms_created_address.take() {
                    self.ui.ms_input_address = address.clone();
                    self.ui.ms_step = MultisigStep::Submitting;
                    self.ui.set_status("正在导入多签...");
                    self.import_created_multisig(address);
                } else {
                    self.ui.clear_status();
                    if self.ui.ms_current_info.is_some() {
                        self.ui.ms_step = MultisigStep::ViewDetail;
                    } else {
                        self.ui.back_to_main();
                    }
                }
            }
            MultisigStep::CreateInputSeed => self.handle_ms_create_input_seed_key(key),
            MultisigStep::CreateSelectCreator => self.handle_ms_create_select_creator_key(key),
            MultisigStep::CreateInputMembers => self.handle_ms_create_input_members_key(key),
            MultisigStep::CreateInputThreshold => self.handle_ms_create_input_threshold_key(key),
            MultisigStep::CreateConfirm => self.handle_ms_create_confirm_key(key),
        }
    }

    fn handle_ms_list_key(&mut self, key: KeyEvent) {
        let visible_count = self
            .store
            .as_ref()
            .map(|s| s.multisigs.iter().filter(|m| !m.hidden).count())
            .unwrap_or(0);
        let total_items = visible_count + 3; // +1 创建Squads, +1 导入Squads, +1 导入Safe(占位)

        match key.code {
            KeyCode::Up => {
                if self.ui.ms_list_selected > 0 {
                    self.ui.ms_list_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.ui.ms_list_selected + 1 < total_items {
                    self.ui.ms_list_selected += 1;
                }
            }
            KeyCode::Enter => {
                if self.ui.ms_list_selected < visible_count {
                    // 选中一个已有多签，查看详情
                    self.view_multisig(self.ui.ms_list_selected);
                } else if self.ui.ms_list_selected == visible_count {
                    // "创建 Squads 多签" → 先选链
                    self.enter_chain_select(MsChainSelectPurpose::Create);
                } else if self.ui.ms_list_selected == visible_count + 1 {
                    // "导入 Squads 多签" → 先选链
                    self.enter_chain_select(MsChainSelectPurpose::Import);
                } else {
                    // ETH Safe placeholder
                    self.ui.set_status("Safe 多签功能正在开发中");
                }
            }
            KeyCode::Esc => {
                self.ui.back_to_main();
            }
            _ => {}
        }
    }

    fn handle_ms_input_address_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) => {
                self.ui.clear_status();
                self.ui.ms_input_address.push(c);
            }
            KeyCode::Backspace => {
                self.ui.clear_status();
                self.ui.ms_input_address.pop();
            }
            KeyCode::Enter => {
                if self.ui.ms_input_address.is_empty() {
                    self.ui.set_status("请输入多签地址");
                    return;
                }
                // 验证地址格式：bs58 + 32 字节
                match bs58::decode(&self.ui.ms_input_address).into_vec() {
                    Ok(bytes) if bytes.len() == 32 => {}
                    _ => {
                        self.ui.set_status("无效的地址格式");
                        return;
                    }
                }
                // 检查是否已导入
                if let Some(ref store) = self.store {
                    let addr = &self.ui.ms_input_address;
                    let exists = store.wallets.iter().any(|w| {
                        matches!(
                            &w.wallet_type,
                            WalletType::Multisig { multisig_address, .. } if multisig_address == addr
                        )
                    });
                    if exists {
                        self.ui.set_status("该多签已导入");
                        return;
                    }
                }
                self.import_multisig();
            }
            KeyCode::Esc => {
                self.ui.back_to_main();
            }
            _ => {}
        }
    }

    fn handle_ms_detail_key(&mut self, key: KeyEvent) {
        const DETAIL_MENU_COUNT: usize = 3;
        match key.code {
            KeyCode::Up => {
                if self.ui.ms_detail_selected > 0 {
                    self.ui.ms_detail_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.ui.ms_detail_selected + 1 < DETAIL_MENU_COUNT {
                    self.ui.ms_detail_selected += 1;
                }
            }
            KeyCode::Enter => {
                match self.ui.ms_detail_selected {
                    0 => {
                        // 查看提案
                        self.fetch_proposals();
                    }
                    1 => {
                        // 创建提案
                        self.ui.ms_step = MultisigStep::SelectProposalType;
                        self.ui.ms_proposal_type_selected = 0;
                        self.ui.clear_status();
                    }
                    2 => {
                        // 修改 Vault 备注
                        self.ui.input_buffer = self.ui.ms_current_vault_label.clone().unwrap_or_default();
                        self.ui.input_purpose = Some(InputPurpose::EditVaultLabel);
                        self.ui.add_wallet_step = AddWalletStep::InputName;
                        self.ui.screen = Screen::TextInput;
                    }
                    _ => {}
                }
            }
            KeyCode::Esc => {
                self.ui.back_to_main();
            }
            _ => {}
        }
    }

    fn handle_ms_proposals_key(&mut self, key: KeyEvent) {
        let count = self.ui.ms_proposals.len();
        match key.code {
            KeyCode::Up => {
                if self.ui.ms_proposal_selected > 0 {
                    self.ui.ms_proposal_selected -= 1;
                }
            }
            KeyCode::Down => {
                if count > 0 && self.ui.ms_proposal_selected + 1 < count {
                    self.ui.ms_proposal_selected += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(proposal) = self.ui.ms_proposals.get(self.ui.ms_proposal_selected).cloned() {
                    self.ui.ms_current_proposal = Some(proposal);
                    self.ui.ms_step = MultisigStep::ViewProposal;
                    self.ui.clear_status();
                }
            }
            KeyCode::Esc => {
                self.ui.ms_step = MultisigStep::ViewDetail;
                self.ui.clear_status();
            }
            _ => {}
        }
    }

    fn handle_ms_proposal_detail_key(&mut self, key: KeyEvent) {
        let status = self.ui.ms_current_proposal.as_ref().map(|p| p.status.clone());

        match key.code {
            KeyCode::Char('a') | KeyCode::Char('A') => {
                if status == Some(multisig::ProposalStatus::Active) {
                    self.ui.ms_vote_action = Some(VoteAction::Approve);
                    self.ui.ms_confirm_password.clear();
                    self.ui.ms_step = MultisigStep::ConfirmVote;
                    self.ui.clear_status();
                }
            }
            KeyCode::Char('r') | KeyCode::Char('R') => {
                if status == Some(multisig::ProposalStatus::Active) {
                    self.ui.ms_vote_action = Some(VoteAction::Reject);
                    self.ui.ms_confirm_password.clear();
                    self.ui.ms_step = MultisigStep::ConfirmVote;
                    self.ui.clear_status();
                }
            }
            KeyCode::Char('e') | KeyCode::Char('E') => {
                if status == Some(multisig::ProposalStatus::Approved) {
                    self.ui.ms_vote_action = Some(VoteAction::Execute);
                    self.ui.ms_confirm_password.clear();
                    self.ui.ms_step = MultisigStep::ConfirmVote;
                    self.ui.clear_status();
                }
            }
            KeyCode::Esc => {
                self.ui.ms_step = MultisigStep::ViewProposals;
                self.ui.clear_status();
            }
            _ => {}
        }
    }

    fn handle_ms_select_proposal_type_key(&mut self, key: KeyEvent) {
        let types = ProposalType::for_chain(&self.ui.ms_selected_chain_id);
        match key.code {
            KeyCode::Up => {
                if self.ui.ms_proposal_type_selected > 0 {
                    self.ui.ms_proposal_type_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.ui.ms_proposal_type_selected + 1 < types.len() {
                    self.ui.ms_proposal_type_selected += 1;
                }
            }
            KeyCode::Enter => {
                let selected = types.get(self.ui.ms_proposal_type_selected);
                match selected {
                    Some(ProposalType::ProgramUpgrade) => {
                        self.ui.ms_upgrade_program.clear();
                        self.ui.ms_upgrade_buffer.clear();
                        self.ui.ms_step = MultisigStep::InputUpgradeProgram;
                    }
                    Some(ProposalType::ProgramCall) => {
                        self.ui.ms_preset_program_selected = 0;
                        self.ui.ms_step = MultisigStep::SelectProgram;
                    }
                    Some(ProposalType::VoteManage) => {
                        self.ui.ms_vs_ops = multisig::MsVoteStakeOp::vote_ops();
                        self.ui.ms_vs_op_selected = 0;
                        self.ui.ms_vs_target.clear();
                        self.ui.ms_vs_param.clear();
                        self.ui.ms_vs_amount.clear();
                        self.ui.ms_step = MultisigStep::SelectVoteStakeOp;
                    }
                    Some(ProposalType::StakeManage) => {
                        self.ui.ms_vs_ops = multisig::MsVoteStakeOp::stake_ops();
                        self.ui.ms_vs_op_selected = 0;
                        self.ui.ms_vs_target.clear();
                        self.ui.ms_vs_param.clear();
                        self.ui.ms_vs_amount.clear();
                        self.ui.ms_step = MultisigStep::SelectVoteStakeOp;
                    }
                    _ => {
                        self.ui.ms_transfer_to.clear();
                        self.ui.ms_transfer_amount.clear();
                        self.ui.ms_transfer_mint.clear();
                        self.ui.ms_step = MultisigStep::InputTransferTo;
                    }
                }
                self.ui.clear_status();
            }
            KeyCode::Esc => {
                self.ui.ms_step = MultisigStep::ViewDetail;
                self.ui.clear_status();
            }
            _ => {}
        }
    }

    fn handle_ms_select_vote_stake_op_key(&mut self, key: KeyEvent) {
        let ops_len = self.ui.ms_vs_ops.len();
        match key.code {
            KeyCode::Up => {
                if self.ui.ms_vs_op_selected > 0 {
                    self.ui.ms_vs_op_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.ui.ms_vs_op_selected + 1 < ops_len {
                    self.ui.ms_vs_op_selected += 1;
                }
            }
            KeyCode::Enter => {
                self.ui.ms_vs_target.clear();
                self.ui.ms_vs_param.clear();
                self.ui.ms_vs_amount.clear();
                self.ui.ms_step = MultisigStep::InputVoteStakeTarget;
                self.ui.clear_status();
            }
            KeyCode::Esc => {
                self.ui.ms_step = MultisigStep::SelectProposalType;
                self.ui.clear_status();
            }
            _ => {}
        }
    }

    fn handle_ms_select_program_key(&mut self, key: KeyEvent) {
        let programs = multisig::presets::programs_for_chain(&self.ui.ms_selected_chain_id);
        match key.code {
            KeyCode::Up => {
                if self.ui.ms_preset_program_selected > 0 {
                    self.ui.ms_preset_program_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.ui.ms_preset_program_selected + 1 < programs.len() {
                    self.ui.ms_preset_program_selected += 1;
                }
            }
            KeyCode::Enter => {
                self.ui.ms_preset_instruction_selected = 0;
                self.ui.ms_step = MultisigStep::SelectProgramInstruction;
                self.ui.clear_status();
            }
            KeyCode::Esc => {
                self.ui.ms_step = MultisigStep::SelectProposalType;
                self.ui.clear_status();
            }
            _ => {}
        }
    }

    fn handle_ms_select_program_instruction_key(&mut self, key: KeyEvent) {
        let programs = multisig::presets::programs_for_chain(&self.ui.ms_selected_chain_id);
        let ix_count = programs
            .get(self.ui.ms_preset_program_selected)
            .map(|p| p.instructions.len())
            .unwrap_or(0);
        match key.code {
            KeyCode::Up => {
                if self.ui.ms_preset_instruction_selected > 0 {
                    self.ui.ms_preset_instruction_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.ui.ms_preset_instruction_selected + 1 < ix_count {
                    self.ui.ms_preset_instruction_selected += 1;
                }
            }
            KeyCode::Enter => {
                // 先检查链上 authority 是否与当前 vault 一致
                let program = match programs.get(self.ui.ms_preset_program_selected) {
                    Some(p) => p,
                    None => return,
                };
                let ms_info = match &self.ui.ms_current_info {
                    Some(i) => i.clone(),
                    None => {
                        self.ui.set_status("多签信息缺失");
                        return;
                    }
                };
                let (vault_pda, _) = multisig::derive_vault_pda(&ms_info.address, 0);
                let rpc_url = self.get_current_ms_rpc_url();
                let pid = program.program_id;

                self.ui.set_status("正在验证 authority...");

                let check_result = self.runtime.block_on(async {
                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(10))
                        .build()
                        .map_err(|e| format!("HTTP 客户端失败: {e}"))?;
                    verify_program_authority(&client, &rpc_url, &pid, &vault_pda).await
                });

                match check_result {
                    Ok(()) => {
                        self.ui.clear_status();
                        let has_args = program
                            .instructions
                            .get(self.ui.ms_preset_instruction_selected)
                            .map(|ix| !ix.args.is_empty())
                            .unwrap_or(false);
                        if has_args {
                            self.ui.ms_program_args.clear();
                            self.ui.ms_program_arg_index = 0;
                            self.ui.ms_program_arg_input.clear();
                            self.ui.ms_step = MultisigStep::InputProgramArgs;
                        } else {
                            self.ui.ms_program_args.clear();
                            self.ui.ms_confirm_password.clear();
                            self.ui.ms_step = MultisigStep::ConfirmCreate;
                        }
                    }
                    Err(e) => {
                        self.ui.set_status(e);
                    }
                }
            }
            KeyCode::Esc => {
                self.ui.ms_step = MultisigStep::SelectProgram;
                self.ui.clear_status();
            }
            _ => {}
        }
    }

    fn handle_ms_input_program_args_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) => {
                self.ui.clear_status();
                self.ui.ms_program_arg_input.push(c);
            }
            KeyCode::Backspace => {
                self.ui.clear_status();
                self.ui.ms_program_arg_input.pop();
            }
            KeyCode::Enter => {
                let input = self.ui.ms_program_arg_input.trim().to_string();
                if input.is_empty() {
                    self.ui.set_status("参数不能为空");
                    return;
                }

                // 验证参数格式
                let programs = multisig::presets::programs_for_chain(&self.ui.ms_selected_chain_id);
                let arg_def = programs
                    .get(self.ui.ms_preset_program_selected)
                    .and_then(|p| p.instructions.get(self.ui.ms_preset_instruction_selected))
                    .and_then(|ix| ix.args.get(self.ui.ms_program_arg_index));

                if let Some(arg) = arg_def {
                    match arg.arg_type {
                        multisig::presets::ArgType::Pubkey => {
                            if bs58::decode(&input)
                                .into_vec()
                                .map(|v| v.len() != 32)
                                .unwrap_or(true)
                            {
                                self.ui.set_status("无效的地址格式");
                                return;
                            }
                        }
                        multisig::presets::ArgType::U64 => {
                            if input.parse::<u64>().is_err() {
                                self.ui.set_status("无效的数值");
                                return;
                            }
                        }
                        multisig::presets::ArgType::U32 => {
                            if input.parse::<u32>().is_err() {
                                self.ui.set_status("无效的数值");
                                return;
                            }
                        }
                        multisig::presets::ArgType::I64 => {
                            if input.parse::<i64>().is_err() {
                                self.ui.set_status("无效的数值");
                                return;
                            }
                        }
                    }
                }

                self.ui.ms_program_args.push(input);
                self.ui.ms_program_arg_index += 1;
                self.ui.ms_program_arg_input.clear();
                self.ui.clear_status();

                // 检查是否所有参数已收集完
                let total_args = programs
                    .get(self.ui.ms_preset_program_selected)
                    .and_then(|p| p.instructions.get(self.ui.ms_preset_instruction_selected))
                    .map(|ix| ix.args.len())
                    .unwrap_or(0);

                if self.ui.ms_program_arg_index >= total_args {
                    self.ui.ms_confirm_password.clear();
                    self.ui.ms_step = MultisigStep::ConfirmCreate;
                }
            }
            KeyCode::Esc => {
                // 返回上一步
                if self.ui.ms_program_arg_index > 0 {
                    // 回退到上一个参数
                    self.ui.ms_program_arg_index -= 1;
                    self.ui.ms_program_arg_input = self.ui.ms_program_args.pop().unwrap_or_default();
                } else {
                    self.ui.ms_step = MultisigStep::SelectProgramInstruction;
                }
                self.ui.clear_status();
            }
            _ => {}
        }
    }

    fn handle_ms_text_input_key(&mut self, key: KeyEvent, field: MsInputField) {
        match key.code {
            KeyCode::Char(c) => {
                self.ui.clear_status();
                match field {
                    MsInputField::TransferTo => self.ui.ms_transfer_to.push(c),
                    MsInputField::TransferAmount => self.ui.ms_transfer_amount.push(c),
                    MsInputField::UpgradeProgram => self.ui.ms_upgrade_program.push(c),
                    MsInputField::UpgradeBuffer => self.ui.ms_upgrade_buffer.push(c),
                    MsInputField::VsTarget => self.ui.ms_vs_target.push(c),
                    MsInputField::VsParam => self.ui.ms_vs_param.push(c),
                    MsInputField::VsAmount => self.ui.ms_vs_amount.push(c),
                }
            }
            KeyCode::Backspace => {
                self.ui.clear_status();
                match field {
                    MsInputField::TransferTo => { self.ui.ms_transfer_to.pop(); }
                    MsInputField::TransferAmount => { self.ui.ms_transfer_amount.pop(); }
                    MsInputField::UpgradeProgram => { self.ui.ms_upgrade_program.pop(); }
                    MsInputField::UpgradeBuffer => { self.ui.ms_upgrade_buffer.pop(); }
                    MsInputField::VsTarget => { self.ui.ms_vs_target.pop(); }
                    MsInputField::VsParam => { self.ui.ms_vs_param.pop(); }
                    MsInputField::VsAmount => { self.ui.ms_vs_amount.pop(); }
                }
            }
            KeyCode::Enter => {
                match field {
                    MsInputField::TransferTo => {
                        if self.ui.ms_transfer_to.is_empty() {
                            self.ui.set_status("地址不能为空");
                            return;
                        }
                        if bs58::decode(&self.ui.ms_transfer_to).into_vec().is_err() {
                            self.ui.set_status("无效的 Solana 地址");
                            return;
                        }
                        self.ui.ms_step = MultisigStep::InputTransferAmount;
                        self.ui.clear_status();
                    }
                    MsInputField::TransferAmount => {
                        if self.ui.ms_transfer_amount.is_empty() {
                            self.ui.set_status("数量不能为空");
                            return;
                        }
                        // SOL 用 9 位小数
                        if let Err(e) = crate::transfer::parse_amount(&self.ui.ms_transfer_amount, 9) {
                            self.ui.set_status(e);
                            return;
                        }
                        self.ui.ms_confirm_password.clear();
                        self.ui.ms_step = MultisigStep::ConfirmCreate;
                        self.ui.clear_status();
                    }
                    MsInputField::UpgradeProgram => {
                        if self.ui.ms_upgrade_program.is_empty() {
                            self.ui.set_status("程序地址不能为空");
                            return;
                        }
                        if bs58::decode(&self.ui.ms_upgrade_program)
                            .into_vec()
                            .map(|v| v.len() != 32)
                            .unwrap_or(true)
                        {
                            self.ui.set_status("无效的 Solana 地址");
                            return;
                        }
                        self.ui.ms_step = MultisigStep::InputUpgradeBuffer;
                        self.ui.clear_status();
                    }
                    MsInputField::UpgradeBuffer => {
                        if self.ui.ms_upgrade_buffer.is_empty() {
                            self.ui.set_status("Buffer 地址不能为空");
                            return;
                        }
                        if bs58::decode(&self.ui.ms_upgrade_buffer)
                            .into_vec()
                            .map(|v| v.len() != 32)
                            .unwrap_or(true)
                        {
                            self.ui.set_status("无效的 Solana 地址");
                            return;
                        }
                        self.ui.ms_confirm_password.clear();
                        self.ui.ms_step = MultisigStep::ConfirmCreate;
                        self.ui.clear_status();
                    }
                    MsInputField::VsTarget => {
                        if self.ui.ms_vs_target.is_empty() {
                            self.ui.set_status("地址不能为空");
                            return;
                        }
                        if bs58::decode(&self.ui.ms_vs_target)
                            .into_vec()
                            .map(|v| v.len() != 32)
                            .unwrap_or(true)
                        {
                            self.ui.set_status("无效的 Solana 地址");
                            return;
                        }
                        let op = self.ui.ms_vs_ops.get(self.ui.ms_vs_op_selected).cloned();
                        match op {
                            Some(ref o) if o.needs_param() => {
                                self.ui.ms_step = MultisigStep::InputVoteStakeParam;
                            }
                            Some(ref o) if o.needs_amount() => {
                                self.ui.ms_step = MultisigStep::InputVoteStakeAmount;
                            }
                            _ => {
                                // StakeDeactivate: 只需 target，直接确认
                                self.ui.ms_confirm_password.clear();
                                self.ui.ms_step = MultisigStep::ConfirmCreate;
                            }
                        }
                        self.ui.clear_status();
                    }
                    MsInputField::VsParam => {
                        if self.ui.ms_vs_param.is_empty() {
                            self.ui.set_status("参数不能为空");
                            return;
                        }
                        if bs58::decode(&self.ui.ms_vs_param)
                            .into_vec()
                            .map(|v| v.len() != 32)
                            .unwrap_or(true)
                        {
                            self.ui.set_status("无效的 Solana 地址");
                            return;
                        }
                        let op = self.ui.ms_vs_ops.get(self.ui.ms_vs_op_selected).cloned();
                        if op.as_ref().is_some_and(|o| o.needs_amount()) {
                            self.ui.ms_step = MultisigStep::InputVoteStakeAmount;
                        } else {
                            self.ui.ms_confirm_password.clear();
                            self.ui.ms_step = MultisigStep::ConfirmCreate;
                        }
                        self.ui.clear_status();
                    }
                    MsInputField::VsAmount => {
                        if self.ui.ms_vs_amount.is_empty() {
                            self.ui.set_status("数量不能为空");
                            return;
                        }
                        if let Err(e) = crate::transfer::parse_amount(&self.ui.ms_vs_amount, 9) {
                            self.ui.set_status(e);
                            return;
                        }
                        self.ui.ms_confirm_password.clear();
                        self.ui.ms_step = MultisigStep::ConfirmCreate;
                        self.ui.clear_status();
                    }
                }
            }
            KeyCode::Esc => {
                self.ui.clear_status();
                match field {
                    MsInputField::TransferTo => {
                        self.ui.ms_step = MultisigStep::SelectProposalType;
                    }
                    MsInputField::TransferAmount => {
                        self.ui.ms_step = MultisigStep::InputTransferTo;
                    }
                    MsInputField::UpgradeProgram => {
                        self.ui.ms_step = MultisigStep::SelectProposalType;
                    }
                    MsInputField::UpgradeBuffer => {
                        self.ui.ms_step = MultisigStep::InputUpgradeProgram;
                    }
                    MsInputField::VsTarget => {
                        self.ui.ms_step = MultisigStep::SelectVoteStakeOp;
                    }
                    MsInputField::VsParam => {
                        self.ui.ms_step = MultisigStep::InputVoteStakeTarget;
                    }
                    MsInputField::VsAmount => {
                        self.ui.ms_step = MultisigStep::InputVoteStakeParam;
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_ms_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) => {
                self.ui.clear_status();
                self.ui.ms_confirm_password.push(c);
            }
            KeyCode::Backspace => {
                self.ui.clear_status();
                self.ui.ms_confirm_password.pop();
            }
            KeyCode::Enter => {
                if self.ui.ms_confirm_password.is_empty() {
                    self.ui.set_status("请输入密码");
                    return;
                }
                match self.ui.ms_step.clone() {
                    MultisigStep::ConfirmCreate => self.execute_create_proposal(),
                    MultisigStep::ConfirmVote => self.execute_vote(),
                    _ => {}
                }
            }
            KeyCode::Esc => {
                self.ui.clear_status();
                match self.ui.ms_step {
                    MultisigStep::ConfirmCreate => {
                        let types = ProposalType::for_chain(&self.ui.ms_selected_chain_id);
                        match types.get(self.ui.ms_proposal_type_selected) {
                            Some(ProposalType::ProgramUpgrade) => {
                                self.ui.ms_step = MultisigStep::InputUpgradeBuffer;
                            }
                            _ => {
                                self.ui.ms_step = MultisigStep::InputTransferAmount;
                            }
                        }
                    }
                    MultisigStep::ConfirmVote => {
                        self.ui.ms_step = MultisigStep::ViewProposal;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// 进入链选择步骤
    fn enter_chain_select(&mut self, purpose: MsChainSelectPurpose) {
        let chains: Vec<(String, String, String)> = self
            .config
            .chains
            .solana
            .iter()
            .map(|c| (c.id.clone(), c.name.clone(), c.rpc_url.clone()))
            .collect();

        if chains.is_empty() {
            self.ui.set_status("没有配置 Solana 系列链");
            return;
        }

        self.ui.screen = Screen::Multisig;

        // 只有一条链时跳过选择
        if chains.len() == 1 {
            let (id, name, rpc) = chains[0].clone();
            self.ui.ms_selected_chain_id = id;
            self.ui.ms_selected_chain_name = name;
            self.ui.ms_selected_rpc_url = rpc;
            match purpose {
                MsChainSelectPurpose::Import => {
                    self.ui.ms_step = MultisigStep::InputAddress;
                    self.ui.ms_input_address.clear();
                    self.ui.clear_status();
                }
                MsChainSelectPurpose::Create => {
                    self.enter_create_multisig();
                }
            }
            return;
        }

        self.ui.ms_solana_chains = chains;
        self.ui.ms_chain_selected = 0;
        self.ui.ms_chain_select_purpose = purpose;
        self.ui.ms_step = MultisigStep::SelectChain;
        self.ui.clear_status();
    }

    fn handle_ms_select_chain_key(&mut self, key: KeyEvent) {
        let count = self.ui.ms_solana_chains.len();
        match key.code {
            KeyCode::Up => {
                if self.ui.ms_chain_selected > 0 {
                    self.ui.ms_chain_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.ui.ms_chain_selected + 1 < count {
                    self.ui.ms_chain_selected += 1;
                }
            }
            KeyCode::Enter => {
                if count == 0 {
                    return;
                }
                let (id, name, rpc) = self.ui.ms_solana_chains[self.ui.ms_chain_selected].clone();
                self.ui.ms_selected_chain_id = id;
                self.ui.ms_selected_chain_name = name;
                self.ui.ms_selected_rpc_url = rpc;

                match self.ui.ms_chain_select_purpose {
                    MsChainSelectPurpose::Import => {
                        self.ui.ms_step = MultisigStep::InputAddress;
                        self.ui.ms_input_address.clear();
                        self.ui.clear_status();
                    }
                    MsChainSelectPurpose::Create => {
                        self.enter_create_multisig();
                    }
                }
            }
            KeyCode::Esc => {
                self.ui.back_to_main();
            }
            _ => {}
        }
    }

    /// 创建成功后直接导入多签
    fn import_created_multisig(&mut self, address: String) {
        let rpc_url = self.ui.ms_selected_rpc_url.clone();
        let tx = self.bg_tx.clone();
        self.runtime.spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap();

            match multisig::squads::fetch_multisig(&client, &rpc_url, &address).await {
                Ok(info) => { let _ = tx.send(BgMessage::MultisigFetched(info)); }
                Err(err) => { let _ = tx.send(BgMessage::MultisigFetchError(err)); }
            }
        });
    }

    /// 导入多签
    fn import_multisig(&mut self) {
        let address = self.ui.ms_input_address.clone();
        let rpc_url = self.ui.ms_selected_rpc_url.clone();

        self.ui.set_status("正在获取多签信息...");

        let tx = self.bg_tx.clone();
        self.runtime.spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap();

            match multisig::squads::fetch_multisig(&client, &rpc_url, &address).await {
                Ok(info) => {
                    let _ = tx.send(BgMessage::MultisigFetched(info));
                }
                Err(err) => {
                    let _ = tx.send(BgMessage::MultisigFetchError(err));
                }
            }
        });
    }

    /// 查看已导入的多签
    fn view_multisig(&mut self, list_index: usize) {
        let store = match &self.store {
            Some(s) => s,
            None => return,
        };

        let visible: Vec<_> = store.multisigs.iter().enumerate().filter(|(_, m)| !m.hidden).collect();
        let (real_index, ms) = match visible.get(list_index) {
            Some((i, m)) => (*i, (*m).clone()),
            None => return,
        };

        self.ui.ms_current_index = real_index;

        // 后台刷新链上信息
        let address = ms.address.clone();
        let rpc_url = ms.rpc_url.clone();
        let tx = self.bg_tx.clone();

        self.ui.set_status("正在加载...");

        self.runtime.spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap();

            match multisig::squads::fetch_multisig(&client, &rpc_url, &address).await {
                Ok(info) => {
                    let _ = tx.send(BgMessage::MultisigFetched(info));
                }
                Err(err) => {
                    let _ = tx.send(BgMessage::MultisigFetchError(err));
                }
            }
        });
    }

    /// 获取提案列表
    fn fetch_proposals(&mut self) {
        let info = match &self.ui.ms_current_info {
            Some(i) => i.clone(),
            None => return,
        };

        let rpc_url = self.get_current_ms_rpc_url();
        let address = info.address.to_string();
        let tx = self.bg_tx.clone();

        self.ui.set_status("正在获取提案...");

        self.runtime.spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap();

            // 先刷新 multisig info 以获取最新的 transaction_index
            let latest_info = match multisig::squads::fetch_multisig(&client, &rpc_url, &address).await {
                Ok(i) => i,
                Err(_) => info, // 刷新失败则用缓存
            };

            match multisig::squads::fetch_active_proposals(&client, &rpc_url, &latest_info).await {
                Ok(proposals) => {
                    let _ = tx.send(BgMessage::ProposalsFetched(proposals));
                }
                Err(err) => {
                    let _ = tx.send(BgMessage::ProposalsFetchError(err));
                }
            }
        });
    }

    /// 执行创建提案
    fn execute_create_proposal(&mut self) {
        // 验证密码
        let pw = match &self.password {
            Some(pw) => pw.clone(),
            None => {
                self.ui.set_status("密码未设置");
                return;
            }
        };
        if self.ui.ms_confirm_password.as_bytes() != pw.as_slice() {
            self.ui.set_status("密码错误");
            self.ui.ms_confirm_password.clear();
            return;
        }

        // 获取多签地址
        let ms_info = match &self.ui.ms_current_info {
            Some(i) => i.clone(),
            None => {
                self.ui.set_status("多签信息缺失");
                return;
            }
        };

        // 获取签名用的 SOL 私钥
        let private_key = match self.get_multisig_signer_key(&ms_info) {
            Some(pk) => pk,
            None => {
                self.ui.set_status("未找到匹配的签名私钥（请确保你的钱包中有此多签的成员地址）");
                return;
            }
        };

        let to_address = self.ui.ms_transfer_to.clone();
        let amount_str = self.ui.ms_transfer_amount.clone();
        let upgrade_program = self.ui.ms_upgrade_program.clone();
        let upgrade_buffer = self.ui.ms_upgrade_buffer.clone();
        let proposal_type_idx = self.ui.ms_proposal_type_selected;
        let preset_program_idx = self.ui.ms_preset_program_selected;
        let preset_instruction_idx = self.ui.ms_preset_instruction_selected;
        let preset_args = self.ui.ms_program_args.clone();
        let chain_id = self.ui.ms_selected_chain_id.clone();
        let vault_index = self.ui.ms_current_vault_index;
        let rpc_url = self.get_current_ms_rpc_url();
        let vs_op = self.ui.ms_vs_ops.get(self.ui.ms_vs_op_selected).cloned();
        let vs_target = self.ui.ms_vs_target.clone();
        let vs_param = self.ui.ms_vs_param.clone();
        let vs_amount = self.ui.ms_vs_amount.clone();

        // 切换到提交中
        self.ui.ms_step = MultisigStep::Submitting;
        self.ui.clear_status();

        let tx = self.bg_tx.clone();
        self.runtime.spawn(async move {
            let result = execute_create_proposal_async(
                &rpc_url,
                &private_key,
                &ms_info.address.to_string(),
                proposal_type_idx,
                &to_address,
                &amount_str,
                &upgrade_program,
                &upgrade_buffer,
                preset_program_idx,
                preset_instruction_idx,
                &preset_args,
                &chain_id,
                vault_index,
                vs_op.as_ref(),
                &vs_target,
                &vs_param,
                &vs_amount,
            )
            .await;

            let _ = tx.send(match result {
                Ok(sig) => BgMessage::MultisigOpComplete {
                    success: true,
                    message: sig,
                },
                Err(err) => BgMessage::MultisigOpComplete {
                    success: false,
                    message: err,
                },
            });
        });
    }

    /// 执行投票操作
    fn execute_vote(&mut self) {
        // 验证密码
        let pw = match &self.password {
            Some(pw) => pw.clone(),
            None => {
                self.ui.set_status("密码未设置");
                return;
            }
        };
        if self.ui.ms_confirm_password.as_bytes() != pw.as_slice() {
            self.ui.set_status("密码错误");
            self.ui.ms_confirm_password.clear();
            return;
        }

        let ms_info = match &self.ui.ms_current_info {
            Some(i) => i.clone(),
            None => {
                self.ui.set_status("多签信息缺失");
                return;
            }
        };

        let proposal = match &self.ui.ms_current_proposal {
            Some(p) => p.clone(),
            None => {
                self.ui.set_status("提案信息缺失");
                return;
            }
        };

        let vote_action = match &self.ui.ms_vote_action {
            Some(a) => a.clone(),
            None => return,
        };

        let private_key = match self.get_multisig_signer_key(&ms_info) {
            Some(pk) => pk,
            None => {
                self.ui.set_status("未找到匹配的签名私钥");
                return;
            }
        };

        let rpc_url = self.get_current_ms_rpc_url();
        let multisig_address = ms_info.address.to_string();
        let tx_index = proposal.transaction_index;
        let vault_index = self.ui.ms_current_vault_index;

        self.ui.ms_step = MultisigStep::Submitting;
        self.ui.clear_status();

        let tx = self.bg_tx.clone();
        self.runtime.spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .map_err(|e| format!("创建 HTTP 客户端失败: {e}"));

            let result = match client {
                Ok(client) => match vote_action {
                    VoteAction::Approve => {
                        multisig::squads::approve_proposal(
                            &client,
                            &rpc_url,
                            &private_key,
                            &multisig_address,
                            tx_index,
                        )
                        .await
                    }
                    VoteAction::Reject => {
                        multisig::squads::reject_proposal(
                            &client,
                            &rpc_url,
                            &private_key,
                            &multisig_address,
                            tx_index,
                        )
                        .await
                    }
                    VoteAction::Execute => {
                        multisig::squads::execute_vault_transaction(
                            &client,
                            &rpc_url,
                            &private_key,
                            &multisig_address,
                            tx_index,
                            vault_index,
                        )
                        .await
                    }
                },
                Err(e) => Err(e),
            };

            let _ = tx.send(match result {
                Ok(sig) => BgMessage::MultisigOpComplete {
                    success: true,
                    message: sig,
                },
                Err(err) => BgMessage::MultisigOpComplete {
                    success: false,
                    message: err,
                },
            });
        });
    }

    // ========== 创建多签 ==========

    /// 进入创建多签流程：收集本地 SOL 地址
    fn enter_create_multisig(&mut self) {
        let sol_addresses = self.collect_local_sol_addresses();
        if sol_addresses.is_empty() {
            self.ui.set_status("没有可用的 SOL 地址，请先添加钱包");
            return;
        }
        self.ui.ms_create_sol_addresses = sol_addresses;
        self.ui.ms_create_creator_selected = 0;
        self.ui.ms_create_members.clear();
        self.ui.ms_create_member_input.clear();
        self.ui.ms_create_threshold_input.clear();
        self.ui.ms_create_seed_input.clear();
        self.ui.ms_confirm_password.clear();

        // 如果有预设创建者地址，跳过创建者选择
        if let Some(ref preset) = self.ui.ms_create_preset_creator
            && let Some(idx) = self.ui.ms_create_sol_addresses.iter().position(|(addr, _)| addr == preset) {
                self.ui.ms_create_creator_selected = idx;
                let creator_addr = preset.clone();
                self.ui.ms_create_members = vec![creator_addr];
                self.ui.ms_create_preset_creator = None;
                if self.ui.ms_create_use_seed {
                    self.ui.ms_step = MultisigStep::CreateInputSeed;
                } else {
                    self.ui.ms_step = MultisigStep::CreateInputMembers;
                }
                self.ui.clear_status();
                return;
            }

        if self.ui.ms_create_use_seed {
            self.ui.ms_step = MultisigStep::CreateInputSeed;
        } else {
            self.ui.ms_step = MultisigStep::CreateSelectCreator;
        }
        self.ui.clear_status();
    }

    /// 收集本地钱包中的 SOL 地址
    fn collect_local_sol_addresses(&self) -> Vec<(String, String)> {
        let mut result = Vec::new();
        let store = match &self.store {
            Some(s) => s,
            None => return result,
        };

        for wallet in &store.wallets {
            if wallet.hidden {
                continue;
            }
            match &wallet.wallet_type {
                WalletType::Mnemonic { sol_accounts, .. } => {
                    for acc in sol_accounts {
                        if !acc.hidden {
                            let label = acc
                                .label
                                .clone()
                                .unwrap_or_else(|| wallet.name.clone());
                            result.push((acc.address.clone(), label));
                        }
                    }
                }
                WalletType::PrivateKey {
                    chain_type: ChainType::Solana,
                    address,
                    ..
                } => {
                    result.push((address.clone(), wallet.name.clone()));
                }
                _ => {}
            }
        }
        result
    }

    fn handle_ms_create_input_seed_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) => {
                self.ui.clear_status();
                self.ui.ms_create_seed_input.push(c);
            }
            KeyCode::Backspace => {
                self.ui.clear_status();
                self.ui.ms_create_seed_input.pop();
            }
            KeyCode::Enter => {
                let seed = self.ui.ms_create_seed_input.trim().to_string();
                if seed.is_empty() {
                    self.ui.set_status("请输入种子私钥");
                    return;
                }
                // 验证 base58 解码后为 64 字节（keypair）或 32 字节（纯私钥）
                match bs58::decode(&seed).into_vec() {
                    Ok(bytes) if bytes.len() == 64 || bytes.len() == 32 => {
                        // 取前 32 字节作为私钥
                        let seed_bytes: [u8; 32] = bytes[..32].try_into().unwrap();
                        let keypair = solana_sdk::signer::keypair::Keypair::new_from_array(seed_bytes);
                        let create_key_pubkey = solana_sdk::signer::Signer::pubkey(&keypair);
                        let (multisig_pda, _) = crate::multisig::derive_multisig_pda(&create_key_pubkey);
                        self.ui.set_status(format!("对应多签地址: {multisig_pda}"));
                        // 统一存储前 32 字节私钥的 base58
                        self.ui.ms_create_seed_input = bs58::encode(&bytes[..32]).into_string();
                        // 有预设创建者时跳过选择
                        if let Some(ref preset) = self.ui.ms_create_preset_creator
                            && let Some(idx) = self.ui.ms_create_sol_addresses.iter().position(|(addr, _)| addr == preset) {
                                self.ui.ms_create_creator_selected = idx;
                                self.ui.ms_create_members = vec![preset.clone()];
                                self.ui.ms_create_preset_creator = None;
                                self.ui.ms_step = MultisigStep::CreateInputMembers;
                                return;
                            }
                        self.ui.ms_step = MultisigStep::CreateSelectCreator;
                    }
                    Ok(bytes) => {
                        self.ui.set_status(format!(
                            "私钥长度错误: {} 字节（需要 64 或 32 字节）",
                            bytes.len()
                        ));
                    }
                    Err(_) => {
                        self.ui.set_status("无效的 base58 编码");
                    }
                }
            }
            KeyCode::Esc => {
                self.ui.ms_step = MultisigStep::SelectChain;
                self.ui.clear_status();
            }
            _ => {}
        }
    }

    fn handle_ms_create_select_creator_key(&mut self, key: KeyEvent) {
        let count = self.ui.ms_create_sol_addresses.len();
        match key.code {
            KeyCode::Up => {
                if self.ui.ms_create_creator_selected > 0 {
                    self.ui.ms_create_creator_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.ui.ms_create_creator_selected + 1 < count {
                    self.ui.ms_create_creator_selected += 1;
                }
            }
            KeyCode::Enter => {
                if count == 0 {
                    return;
                }
                // 创建者自动作为第一个成员
                let (creator_addr, _) = self.ui.ms_create_sol_addresses
                    [self.ui.ms_create_creator_selected]
                    .clone();
                self.ui.ms_create_members = vec![creator_addr];
                self.ui.ms_create_member_input.clear();
                self.ui.ms_step = MultisigStep::CreateInputMembers;
                self.ui.clear_status();
            }
            KeyCode::Esc => {
                if self.ui.ms_create_use_seed {
                    self.ui.ms_step = MultisigStep::CreateInputSeed;
                } else {
                    self.ui.ms_step = MultisigStep::SelectChain;
                }
                self.ui.clear_status();
            }
            _ => {}
        }
    }

    fn handle_ms_create_input_members_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('d') | KeyCode::Char('D')
                if self.ui.ms_create_member_input.is_empty() =>
            {
                // 完成添加成员，进入阈值设置
                if self.ui.ms_create_members.len() < 2 {
                    self.ui.set_status("至少需要 2 个成员");
                    return;
                }
                self.ui.ms_create_threshold_input.clear();
                self.ui.ms_step = MultisigStep::CreateInputThreshold;
                self.ui.clear_status();
            }
            KeyCode::Char(c) => {
                self.ui.clear_status();
                self.ui.ms_create_member_input.push(c);
            }
            KeyCode::Backspace => {
                self.ui.clear_status();
                self.ui.ms_create_member_input.pop();
            }
            KeyCode::Enter => {
                let addr = self.ui.ms_create_member_input.trim().to_string();
                if addr.is_empty() {
                    return;
                }
                // 验证 base58
                if bs58::decode(&addr).into_vec().map(|v| v.len()).unwrap_or(0) != 32 {
                    self.ui.set_status("无效的 Solana 地址");
                    return;
                }
                // 检查重复
                if self.ui.ms_create_members.contains(&addr) {
                    self.ui.set_status("该地址已添加");
                    return;
                }
                self.ui.ms_create_members.push(addr);
                self.ui.ms_create_member_input.clear();
                self.ui.clear_status();
            }
            KeyCode::Esc => {
                self.ui.ms_step = MultisigStep::CreateSelectCreator;
                self.ui.clear_status();
            }
            _ => {}
        }
    }

    fn handle_ms_create_input_threshold_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) if c.is_ascii_digit() => {
                self.ui.clear_status();
                self.ui.ms_create_threshold_input.push(c);
            }
            KeyCode::Backspace => {
                self.ui.clear_status();
                self.ui.ms_create_threshold_input.pop();
            }
            KeyCode::Enter => {
                let member_count = self.ui.ms_create_members.len();
                let threshold: u16 = match self.ui.ms_create_threshold_input.parse() {
                    Ok(v) => v,
                    Err(_) => {
                        self.ui.set_status("请输入有效数字");
                        return;
                    }
                };
                if threshold == 0 || threshold as usize > member_count {
                    self.ui
                        .set_status(format!("阈值必须在 1-{member_count} 之间"));
                    return;
                }
                self.ui.ms_confirm_password.clear();
                self.ui.ms_step = MultisigStep::CreateConfirm;
                self.ui.clear_status();
            }
            KeyCode::Esc => {
                self.ui.ms_step = MultisigStep::CreateInputMembers;
                self.ui.clear_status();
            }
            _ => {}
        }
    }

    fn handle_ms_create_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) => {
                self.ui.clear_status();
                self.ui.ms_confirm_password.push(c);
            }
            KeyCode::Backspace => {
                self.ui.clear_status();
                self.ui.ms_confirm_password.pop();
            }
            KeyCode::Enter => {
                self.execute_create_multisig();
            }
            KeyCode::Esc => {
                self.ui.ms_confirm_password.clear();
                self.ui.ms_step = MultisigStep::CreateInputThreshold;
                self.ui.clear_status();
            }
            _ => {}
        }
    }

    /// 执行创建多签
    fn execute_create_multisig(&mut self) {
        // 验证密码
        let pw = match &self.password {
            Some(pw) => pw.clone(),
            None => {
                self.ui.set_status("密码未设置");
                return;
            }
        };
        if self.ui.ms_confirm_password.as_bytes() != pw.as_slice() {
            self.ui.set_status("密码错误");
            self.ui.ms_confirm_password.clear();
            return;
        }

        // 获取创建者私钥
        let (creator_address, _) = match self
            .ui
            .ms_create_sol_addresses
            .get(self.ui.ms_create_creator_selected)
        {
            Some(a) => a.clone(),
            None => {
                self.ui.set_status("未选择创建者");
                return;
            }
        };

        let private_key = match self.get_sol_private_key(&creator_address) {
            Some(pk) => pk,
            None => {
                self.ui.set_status("无法获取创建者私钥");
                return;
            }
        };

        let threshold: u16 = match self.ui.ms_create_threshold_input.parse() {
            Ok(v) => v,
            Err(_) => {
                self.ui.set_status("无效的阈值");
                return;
            }
        };

        let members: Vec<solana_sdk::pubkey::Pubkey> = match self
            .ui
            .ms_create_members
            .iter()
            .map(|addr| {
                solana_sdk::pubkey::Pubkey::from_str(addr)
                    .map_err(|e| format!("无效的成员地址 {addr}: {e}"))
            })
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(m) => m,
            Err(e) => {
                self.ui.set_status(e);
                return;
            }
        };

        let rpc_url = self.ui.ms_selected_rpc_url.clone();

        // 如果使用种子模式，解析种子私钥
        let seed_key = if self.ui.ms_create_use_seed {
            let seed = self.ui.ms_create_seed_input.trim().to_string();
            match bs58::decode(&seed).into_vec() {
                Ok(bytes) if bytes.len() == 32 => Some(bytes),
                _ => {
                    self.ui.set_status("种子私钥无效");
                    return;
                }
            }
        } else {
            None
        };

        self.ui.ms_step = MultisigStep::Submitting;
        self.ui.clear_status();

        let tx = self.bg_tx.clone();
        self.runtime.spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .map_err(|e| format!("创建 HTTP 客户端失败: {e}"));

            let result = match client {
                Ok(client) => {
                    multisig::squads::create_multisig_v2(
                        &client,
                        &rpc_url,
                        &private_key,
                        &members,
                        threshold,
                        seed_key.as_deref(),
                    )
                    .await
                }
                Err(e) => Err(e),
            };

            let _ = tx.send(match result {
                Ok(result_str) => {
                    // result_str 格式: "multisig_pda|tx_sig"
                    let mut parts = result_str.splitn(2, '|');
                    let address = parts.next().unwrap_or(&result_str).to_string();
                    let tx_sig = parts.next().unwrap_or("").to_string();
                    BgMessage::MultisigCreated { address, tx_sig }
                }
                Err(err) => BgMessage::MultisigOpComplete {
                    success: false,
                    message: err,
                },
            });
        });
    }

    /// 获取指定 SOL 地址的私钥
    fn get_sol_private_key(&mut self, address: &str) -> Option<Vec<u8>> {
        // 先提取加密数据，释放 store 借用
        enum SolKeySource {
            Mnemonic { encrypted: String, derivation_index: u32 },
            PrivateKey { encrypted: String },
        }
        let source = {
            let store = self.store.as_ref()?;
            let mut found = None;
            for wallet in &store.wallets {
                match &wallet.wallet_type {
                    WalletType::Mnemonic { encrypted_mnemonic, sol_accounts, .. } => {
                        for acc in sol_accounts {
                            if acc.address == address {
                                found = Some(SolKeySource::Mnemonic {
                                    encrypted: encrypted_mnemonic.clone(),
                                    derivation_index: acc.derivation_index,
                                });
                                break;
                            }
                        }
                    }
                    WalletType::PrivateKey {
                        chain_type: ChainType::Solana,
                        encrypted_private_key,
                        address: pk_address,
                        ..
                    } => {
                        if pk_address == address {
                            found = Some(SolKeySource::PrivateKey {
                                encrypted: encrypted_private_key.clone(),
                            });
                        }
                    }
                    _ => {}
                }
                if found.is_some() { break; }
            }
            found?
        };

        match source {
            SolKeySource::Mnemonic { encrypted, derivation_index } => {
                let mut phrase = self.decrypt_inner_secret(&encrypted)?;
                let mut seed = mnemonic::mnemonic_to_seed(&phrase, "").ok()?;
                phrase.clear_sensitive();
                let result = sol_keys::derive_sol_private_key(&seed, derivation_index).ok();
                seed.clear_sensitive();
                result
            }
            SolKeySource::PrivateKey { encrypted } => {
                let mut pk_str = self.decrypt_inner_secret(&encrypted)?;
                let mut bytes = bs58::decode(&pk_str).into_vec().ok()?;
                pk_str.clear_sensitive();
                let result = match bytes.len() {
                    64 => Some(bytes[..32].to_vec()),
                    32 => Some(bytes.clone()),
                    _ => None,
                };
                bytes.clear_sensitive();
                result
            }
        }
    }

    /// 获取多签签名用的私钥
    /// 遍历钱包，找到是多签成员的 SOL 地址对应的私钥
    fn get_multisig_signer_key(&mut self, ms_info: &multisig::MultisigInfo) -> Option<Vec<u8>> {
        // 先收集成员地址列表
        let member_addrs: Vec<String> = ms_info
            .members
            .iter()
            .map(|m| m.address())
            .collect();

        // 找到匹配的本地地址
        let matched_address = {
            let store = self.store.as_ref()?;
            let mut found = None;
            for wallet in &store.wallets {
                match &wallet.wallet_type {
                    WalletType::Mnemonic { sol_accounts, .. } => {
                        for acc in sol_accounts {
                            if member_addrs.contains(&acc.address) {
                                found = Some(acc.address.clone());
                                break;
                            }
                        }
                    }
                    WalletType::PrivateKey {
                        chain_type: ChainType::Solana,
                        address,
                        ..
                    } => {
                        if member_addrs.contains(address) {
                            found = Some(address.clone());
                        }
                    }
                    _ => {}
                }
                if found.is_some() { break; }
            }
            found?
        };

        self.get_sol_private_key(&matched_address)
    }

    /// 保存多签到 store（新版：创建 WalletType::Multisig 钱包）
    fn save_multisig_to_store(&mut self, info: &multisig::MultisigInfo) {
        let info_address_str = info.address.to_string();

        // 检查是否已存在（新格式）
        if let Some(ref store) = self.store {
            let exists = store.wallets.iter().any(|w| {
                matches!(
                    &w.wallet_type,
                    WalletType::Multisig { multisig_address, .. } if multisig_address == &info_address_str
                )
            });
            if exists {
                return;
            }
        }

        let (vault_pda, _) = multisig::derive_vault_pda(&info.address, 0);
        let vault_address = vault_pda.to_string();

        let rpc_url = if self.ui.ms_selected_rpc_url.is_empty() {
            self.get_solana_rpc_url()
        } else {
            self.ui.ms_selected_rpc_url.clone()
        };
        let chain_id = self.ui.ms_selected_chain_id.clone();
        let chain_name = self.ui.ms_selected_chain_name.clone();

        if let Some(ref mut store) = self.store {
            let sort_order = store.wallets.len() as u32;
            let wallet_index = store.wallets.len();
            store.wallets.push(Wallet {
                id: uuid::Uuid::new_v4().to_string(),
                name: format!("Multisig {}", &info_address_str[..8]),
                wallet_type: WalletType::Multisig {
                    multisig_address: info_address_str,
                    rpc_url,
                    chain_id,
                    chain_name,
                    threshold: info.threshold,
                    member_addresses: info.members.iter().map(|m| m.address()).collect(),
                    vaults: vec![VaultAccount {
                        vault_index: 0,
                        address: vault_address,
                        label: None,
                        hidden: false,
                    }],
                },
                sort_order,
                hidden: false,
                created_at: chrono::Utc::now().timestamp(),
            });
            self.ui.ms_current_wallet_index = wallet_index;
            let _ = self.save_store_inner();
        }
    }

    /// 隐藏/显示多签 vault
    fn toggle_vault_hidden(&mut self, wallet_index: usize, vault_pos: usize, hidden: bool) {
        if let Some(ref mut store) = self.store
            && let Some(w) = store.wallets.get_mut(wallet_index)
            && let WalletType::Multisig { ref mut vaults, .. } = w.wallet_type
            && let Some(v) = vaults.get_mut(vault_pos)
        {
            v.hidden = hidden;
            let _ = self.save_store_inner();
        }
        self.ui.back_to_main();
    }

    /// 添加 vault 到多签钱包
    fn add_vault_to_multisig(&mut self, wallet_index: usize) {
        if let Some(ref mut store) = self.store
            && let Some(w) = store.wallets.get_mut(wallet_index)
            && let WalletType::Multisig {
                ref multisig_address,
                ref mut vaults,
                ..
            } = w.wallet_type
        {
            let next_index = vaults.iter().map(|v| v.vault_index).max().unwrap_or(0) + 1;
            let ms_pubkey = multisig_address
                .parse::<multisig::Pubkey>()
                .expect("invalid multisig address");
            let (vault_pda, _) = multisig::derive_vault_pda(&ms_pubkey, next_index);
            vaults.push(VaultAccount {
                vault_index: next_index,
                address: vault_pda.to_string(),
                label: None,
                hidden: false,
            });
            let _ = self.save_store_inner();
        }
        self.ui.back_to_main();
    }

    /// 获取 Solana RPC URL（用于多签操作）
    fn get_solana_rpc_url(&self) -> String {
        self.config
            .chains
            .solana
            .first()
            .map(|c| c.rpc_url.clone())
            .unwrap_or_else(|| "https://api.mainnet-beta.solana.com".to_string())
    }

    /// 获取当前多签的 RPC URL
    fn get_current_ms_rpc_url(&self) -> String {
        // 先尝试新格式（WalletType::Multisig）
        if let Some(ref store) = self.store
            && let Some(w) = store.wallets.get(self.ui.ms_current_wallet_index)
            && let WalletType::Multisig { ref rpc_url, .. } = w.wallet_type
        {
            return rpc_url.clone();
        }
        // 回退到旧格式（兼容）
        self.store
            .as_ref()
            .and_then(|s| s.multisigs.get(self.ui.ms_current_index))
            .map(|m| m.rpc_url.clone())
            .unwrap_or_else(|| self.get_solana_rpc_url())
    }

    // ========== 辅助方法 ==========

    fn save_store(&self) -> Result<(), crate::error::StorageError> {
        if let (Some(store), Some(pw)) = (&self.store, &self.password) {
            encrypted::save(store, pw, &self.data_path)?;
        }
        Ok(())
    }

    fn save_store_inner(&self) -> Result<(), crate::error::StorageError> {
        self.save_store()
    }

    /// 解密内层加密的秘密（助记词/私钥）
    fn decrypt_inner_secret(&self, encrypted: &str) -> Option<String> {
        let pw = self.password.as_ref()?;
        let parts: Vec<&str> = encrypted.split(':').collect();
        if parts.len() != 3 {
            return None;
        }
        let salt = hex::decode(parts[0]).ok()?;
        let nonce = hex::decode(parts[1]).ok()?;
        let ciphertext = hex::decode(parts[2]).ok()?;
        let plaintext =
            crate::crypto::encryption::decrypt(&ciphertext, pw, &salt, &nonce).ok()?;
        String::from_utf8(plaintext).ok()
    }
}

/// 选中目标类型
enum SelectionTarget {
    Wallet(usize),
    MnemonicAddress {
        wallet_index: usize,
        chain_type: ChainType,
        account_index: usize,
    },
    PrivateKeyAddress(usize),
    WatchAddress(usize),
    MultisigVault {
        wallet_index: usize,
        #[allow(dead_code)]
        vault_pos: usize,
    },
    AddWallet,
}

/// 转账输入字段类型
enum TransferInputField {
    Address,
    Amount,
}

/// 多签输入字段类型
enum MsInputField {
    TransferTo,
    TransferAmount,
    UpgradeProgram,
    UpgradeBuffer,
    VsTarget,
    VsParam,
    VsAmount,
}

/// 后台执行转账
async fn execute_transfer_async(
    private_key: Vec<u8>,
    asset: TransferableAsset,
    to_address: String,
    amount_raw: u128,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    match (&asset.chain_type, &asset.asset_kind) {
        (ChainType::Ethereum, AssetKind::Native) => {
            let chain_id = asset.evm_chain_id.ok_or("缺少 chain_id")?;
            transfer::eth_transfer::send_eth_native(
                &client,
                &asset.rpc_url,
                chain_id,
                &private_key,
                &to_address,
                amount_raw,
            )
            .await
        }
        (ChainType::Ethereum, AssetKind::Erc20 { contract_address }) => {
            let chain_id = asset.evm_chain_id.ok_or("缺少 chain_id")?;
            transfer::eth_transfer::send_erc20(
                &client,
                &asset.rpc_url,
                chain_id,
                &private_key,
                contract_address,
                &to_address,
                amount_raw,
            )
            .await
        }
        (ChainType::Solana, AssetKind::Native) => {
            let amount_u64: u64 = amount_raw
                .try_into()
                .map_err(|_| "SOL 转账数量超出范围".to_string())?;
            transfer::sol_transfer::send_sol_native(
                &client,
                &asset.rpc_url,
                &private_key,
                &to_address,
                amount_u64,
            )
            .await
        }
        (ChainType::Solana, AssetKind::SplToken { mint_address, is_token_2022 }) => {
            let amount_u64: u64 = amount_raw
                .try_into()
                .map_err(|_| "SPL 转账数量超出范围".to_string())?;
            let token_program = if *is_token_2022 {
                "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb"
            } else {
                "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"
            };
            transfer::sol_transfer::send_spl_token(
                &client,
                &asset.rpc_url,
                &private_key,
                mint_address,
                &to_address,
                amount_u64,
                token_program,
            )
            .await
        }
        _ => Err("不支持的转账类型".into()),
    }
}

/// 后台执行创建提案
#[allow(clippy::too_many_arguments)]
async fn execute_create_proposal_async(
    rpc_url: &str,
    private_key: &[u8],
    multisig_address: &str,
    proposal_type_idx: usize,
    to_address: &str,
    amount_str: &str,
    upgrade_program: &str,
    upgrade_buffer: &str,
    preset_program_idx: usize,
    preset_instruction_idx: usize,
    preset_args: &[String],
    chain_id: &str,
    vault_index: u8,
    vs_op: Option<&multisig::MsVoteStakeOp>,
    vs_target: &str,
    vs_param: &str,
    vs_amount: &str,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    let multisig_pubkey = solana_sdk::pubkey::Pubkey::from_str(multisig_address)
        .map_err(|e| format!("无效的多签地址: {e}"))?;

    let (vault_pda, _) = multisig::derive_vault_pda(&multisig_pubkey, vault_index);

    let proposal_types = ProposalType::for_chain(chain_id);
    let proposal_type = proposal_types
        .get(proposal_type_idx)
        .ok_or("无效的提案类型")?;

    let inner_instructions = match proposal_type {
        ProposalType::SolTransfer => {
            let to_pubkey: [u8; 32] = bs58::decode(to_address)
                .into_vec()
                .map_err(|e| format!("无效的目标地址: {e}"))?
                .try_into()
                .map_err(|_| "目标地址长度无效".to_string())?;

            let amount_raw = crate::transfer::parse_amount(amount_str, 9)?;
            let lamports: u64 = amount_raw
                .try_into()
                .map_err(|_| "SOL 数量超出范围".to_string())?;

            vec![multisig::proposals::build_sol_transfer_instruction(
                &vault_pda.to_bytes(),
                &to_pubkey,
                lamports,
            )]
        }
        ProposalType::TokenTransfer => {
            return Err("Token 转账提案暂未实现，请使用 SOL 转账".into());
        }
        ProposalType::ProgramCall => {
            let programs = multisig::presets::programs_for_chain(chain_id);
            let program = programs
                .get(preset_program_idx)
                .ok_or("无效的预制程序")?;
            let instruction = program
                .instructions
                .get(preset_instruction_idx)
                .ok_or("无效的预制指令")?;

            (instruction.build)(
                &vault_pda.to_bytes(),
                &program.program_id,
                preset_args,
            )?
        }
        ProposalType::ProgramUpgrade => {
            let program_bytes: [u8; 32] = bs58::decode(upgrade_program)
                .into_vec()
                .map_err(|e| format!("无效的程序地址: {e}"))?
                .try_into()
                .map_err(|_| "程序地址长度无效".to_string())?;

            let buffer_bytes: [u8; 32] = bs58::decode(upgrade_buffer)
                .into_vec()
                .map_err(|e| format!("无效的 Buffer 地址: {e}"))?
                .try_into()
                .map_err(|_| "Buffer 地址长度无效".to_string())?;

            // 检查程序的 upgrade authority 是否为当前 vault
            verify_upgrade_authority(&client, rpc_url, &program_bytes, &vault_pda).await?;

            // 检查 buffer 账户是否存在
            verify_buffer_exists(&client, rpc_url, upgrade_buffer).await?;

            multisig::proposals::build_program_upgrade_instructions(
                &program_bytes,
                &buffer_bytes,
                &vault_pda.to_bytes(), // spill = vault
                &vault_pda.to_bytes(), // authority = vault
            )
        }
        ProposalType::VoteManage | ProposalType::StakeManage => {
            let op = vs_op.ok_or("未选择操作类型")?;
            let target_bytes: [u8; 32] = multisig::proposals::decode_bs58_pubkey(vs_target)
                .ok_or_else(|| format!("无效的目标地址: {vs_target}"))?;
            let vault_bytes = vault_pda.to_bytes();

            use multisig::MsVoteStakeOp;
            match op {
                MsVoteStakeOp::VoteAuthorizeVoter => {
                    let new_auth = multisig::proposals::decode_bs58_pubkey(vs_param)
                        .ok_or("无效的新权限地址")?;
                    vec![multisig::proposals::build_vote_authorize_instruction(
                        &target_bytes, &vault_bytes, &new_auth, 0,
                    )]
                }
                MsVoteStakeOp::VoteAuthorizeWithdrawer => {
                    let new_auth = multisig::proposals::decode_bs58_pubkey(vs_param)
                        .ok_or("无效的新权限地址")?;
                    vec![multisig::proposals::build_vote_authorize_instruction(
                        &target_bytes, &vault_bytes, &new_auth, 1,
                    )]
                }
                MsVoteStakeOp::VoteWithdraw => {
                    let to_bytes = multisig::proposals::decode_bs58_pubkey(vs_param)
                        .ok_or("无效的提取目标地址")?;
                    let lamports: u64 = crate::transfer::parse_amount(vs_amount, 9)?
                        .try_into()
                        .map_err(|_| "SOL 数量超出范围".to_string())?;
                    vec![multisig::proposals::build_vote_withdraw_instruction(
                        &target_bytes, &to_bytes, &vault_bytes, lamports,
                    )]
                }
                MsVoteStakeOp::StakeAuthorizeStaker => {
                    let new_auth = multisig::proposals::decode_bs58_pubkey(vs_param)
                        .ok_or("无效的新权限地址")?;
                    vec![multisig::proposals::build_stake_authorize_instruction(
                        &target_bytes, &vault_bytes, &new_auth, 0,
                    )]
                }
                MsVoteStakeOp::StakeAuthorizeWithdrawer => {
                    let new_auth = multisig::proposals::decode_bs58_pubkey(vs_param)
                        .ok_or("无效的新权限地址")?;
                    vec![multisig::proposals::build_stake_authorize_instruction(
                        &target_bytes, &vault_bytes, &new_auth, 1,
                    )]
                }
                MsVoteStakeOp::StakeDelegate => {
                    let vote_account = multisig::proposals::decode_bs58_pubkey(vs_param)
                        .ok_or("无效的 Vote 账户地址")?;
                    vec![multisig::proposals::build_stake_delegate_instruction(
                        &target_bytes, &vote_account, &vault_bytes,
                    )]
                }
                MsVoteStakeOp::StakeDeactivate => {
                    vec![multisig::proposals::build_stake_deactivate_instruction(
                        &target_bytes, &vault_bytes,
                    )]
                }
                MsVoteStakeOp::StakeWithdraw => {
                    let to_bytes = multisig::proposals::decode_bs58_pubkey(vs_param)
                        .ok_or("无效的提取目标地址")?;
                    let lamports: u64 = crate::transfer::parse_amount(vs_amount, 9)?
                        .try_into()
                        .map_err(|_| "SOL 数量超出范围".to_string())?;
                    vec![multisig::proposals::build_stake_withdraw_instruction(
                        &target_bytes, &to_bytes, &vault_bytes, lamports,
                    )]
                }
            }
        }
    };

    multisig::squads::create_proposal_and_approve(
        &client,
        rpc_url,
        private_key,
        multisig_address,
        vault_index,
        inner_instructions,
    )
    .await
}

/// 检查程序的 upgrade authority 是否为指定的 vault PDA
async fn verify_upgrade_authority(
    client: &reqwest::Client,
    rpc_url: &str,
    program_bytes: &[u8; 32],
    vault_pda: &solana_sdk::pubkey::Pubkey,
) -> Result<(), String> {
    use solana_sdk::pubkey::Pubkey;

    // 推导 ProgramData PDA
    let program_pk = Pubkey::new_from_array(*program_bytes);
    let bpf_loader_id = Pubkey::from_str("BPFLoaderUpgradeab1e11111111111111111111111")
        .map_err(|e| format!("BPF Loader 地址解析失败: {e}"))?;
    let (programdata_pda, _) = Pubkey::find_program_address(&[program_pk.as_ref()], &bpf_loader_id);

    // 获取 ProgramData 账户数据
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getAccountInfo",
        "params": [programdata_pda.to_string(), {"encoding": "base64", "commitment": "confirmed"}],
        "id": 1
    });
    let resp = crate::transfer::sol_transfer::rpc_call(client, rpc_url, &body).await?;

    let value = resp
        .get("result")
        .and_then(|r| r.get("value"))
        .ok_or("无法获取 ProgramData 账户")?;

    if value.is_null() {
        return Err("ProgramData 账户不存在，请确认程序地址正确".into());
    }

    let data_arr = value
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or("ProgramData 缺少 data 字段")?;

    let base64_str = data_arr
        .first()
        .and_then(|v| v.as_str())
        .ok_or("ProgramData 数据格式无效")?;

    let data = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        base64_str,
    )
    .map_err(|e| format!("ProgramData base64 解码失败: {e}"))?;

    // ProgramData 布局 (bincode): variant(4) + slot(8) + option(1) + authority(32)
    if data.len() < 45 {
        return Err("ProgramData 账户数据过短".into());
    }

    // variant 应为 3 (ProgramData)
    let variant = u32::from_le_bytes(data[0..4].try_into().unwrap());
    if variant != 3 {
        return Err(format!("不是有效的 ProgramData 账户 (variant={})", variant));
    }

    // offset 12: Option<Pubkey> 的 discriminator
    if data[12] == 0 {
        return Err("程序不可升级（upgrade authority 已撤销）".into());
    }

    let authority = Pubkey::try_from(&data[13..45])
        .map_err(|_| "解析 upgrade authority 失败")?;

    if authority != *vault_pda {
        return Err(format!(
            "upgrade authority 不匹配\n当前 vault: {}\n链上 authority: {}",
            vault_pda, authority
        ));
    }

    Ok(())
}

/// 检查 buffer 账户是否存在
async fn verify_buffer_exists(
    client: &reqwest::Client,
    rpc_url: &str,
    buffer_address: &str,
) -> Result<(), String> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "getAccountInfo",
        "params": [buffer_address, {"encoding": "base64", "commitment": "confirmed"}],
        "id": 1
    });
    let resp = crate::transfer::sol_transfer::rpc_call(client, rpc_url, &body).await?;

    let value = resp
        .get("result")
        .and_then(|r| r.get("value"));

    if value.is_none() || value.unwrap().is_null() {
        return Err(format!("Buffer 账户 {} 不存在，请先执行 solana program write-buffer", buffer_address));
    }

    Ok(())
}

/// 验证预制程序的 config PDA 中的 authority/admin 是否为当前 vault
async fn verify_program_authority(
    client: &reqwest::Client,
    rpc_url: &str,
    program_id: &[u8; 32],
    vault_pda: &solana_sdk::pubkey::Pubkey,
) -> Result<(), String> {
    use solana_sdk::pubkey::Pubkey;

    let pid = Pubkey::new_from_array(*program_id);

    // 尝试 "config" PDA（大部分预制程序使用此 seed）
    let config_pda = {
        let (pda, _) = Pubkey::find_program_address(&[b"config"], &pid);
        pda
    };

    // 也尝试 "quest_config"（Quest 程序使用此 seed）
    let quest_config_pda = {
        let (pda, _) = Pubkey::find_program_address(&[b"quest_config"], &pid);
        pda
    };

    // 先尝试 "config"，再尝试 "quest_config"
    for config_addr in &[config_pda, quest_config_pda] {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "getAccountInfo",
            "params": [config_addr.to_string(), {"encoding": "base64", "commitment": "confirmed"}],
            "id": 1
        });
        let resp = crate::transfer::sol_transfer::rpc_call(client, rpc_url, &body).await?;

        let value = resp
            .get("result")
            .and_then(|r| r.get("value"));

        if value.is_none() || value.unwrap().is_null() {
            continue;
        }

        // 解析 base64 数据
        let data_b64 = value
            .unwrap()
            .get("data")
            .and_then(|d| d.as_array())
            .and_then(|arr| arr.first())
            .and_then(|v| v.as_str())
            .ok_or("无法解析 config 账户数据")?;

        use base64::Engine;
        let data = base64::engine::general_purpose::STANDARD
            .decode(data_b64)
            .map_err(|e| format!("base64 解码失败: {e}"))?;

        // Anchor 账户结构: 8 字节 discriminator + admin/authority pubkey (32 bytes)
        if data.len() < 40 {
            return Err("config 账户数据太短".to_string());
        }

        let authority_bytes: [u8; 32] = data[8..40]
            .try_into()
            .map_err(|_| "无法读取 authority 字段")?;
        let authority = Pubkey::new_from_array(authority_bytes);

        if authority != *vault_pda {
            return Err(format!(
                "authority 不匹配\n当前 vault: {}\n链上 authority: {}",
                vault_pda, authority
            ));
        }

        return Ok(());
    }

    Err("未找到 config 账户，该程序可能尚未初始化".to_string())
}

// ========== Staking (Vote/Stake) ==========

impl App {
    fn get_sol_address(&self, wallet_index: usize, account_index: usize) -> Option<String> {
        let store = self.store.as_ref()?;
        let wallet = store.wallets.get(wallet_index)?;
        match &wallet.wallet_type {
            WalletType::Mnemonic { sol_accounts, .. } => {
                sol_accounts.get(account_index).map(|a| a.address.clone())
            }
            WalletType::PrivateKey {
                chain_type: ChainType::Solana,
                address,
                ..
            } => Some(address.clone()),
            _ => None,
        }
    }

    fn enter_vote_detail(&mut self, wallet_index: usize, account_index: usize, address: &str) {
        self.ui.screen = Screen::Staking;
        self.ui.stk_step = StakingStep::VoteDetail;
        self.ui.stk_from_address = address.to_string();
        self.ui.stk_wallet_index = wallet_index;
        self.ui.stk_account_index = account_index;
        self.ui.stk_rpc_url = self.get_rpc_url_for_address(address);
        self.ui.stk_native_symbol = self.get_native_symbol_for_address(address);
        self.ui.stk_vote_info = None;
        self.ui.stk_fetch_error = None;
        self.ui.stk_detail_selected = 0;
        self.fetch_vote_account_info(address);
    }

    fn enter_stake_detail(&mut self, wallet_index: usize, account_index: usize, address: &str) {
        self.ui.screen = Screen::Staking;
        self.ui.stk_step = StakingStep::StakeDetail;
        self.ui.stk_from_address = address.to_string();
        self.ui.stk_wallet_index = wallet_index;
        self.ui.stk_account_index = account_index;
        self.ui.stk_rpc_url = self.get_rpc_url_for_address(address);
        self.ui.stk_native_symbol = self.get_native_symbol_for_address(address);
        self.ui.stk_stake_info = None;
        self.ui.stk_fetch_error = None;
        self.ui.stk_detail_selected = 0;
        self.fetch_stake_account_info(address);
    }

    fn enter_create_vote(&mut self, wallet_index: usize, account_index: usize) {
        self.enter_staking_chain_select(wallet_index, account_index, StakingCreateType::Vote);
    }

    fn enter_create_stake(&mut self, wallet_index: usize, account_index: usize) {
        self.enter_staking_chain_select(wallet_index, account_index, StakingCreateType::Stake);
    }

    /// 进入 staking 创建流程的链选择步骤
    fn enter_staking_chain_select(
        &mut self,
        wallet_index: usize,
        account_index: usize,
        create_type: StakingCreateType,
    ) {
        let address = match self.get_sol_address(wallet_index, account_index) {
            Some(a) => a,
            None => return,
        };

        let chains: Vec<(String, String, String, String)> = self
            .config
            .chains
            .solana
            .iter()
            .map(|c| (c.id.clone(), c.name.clone(), c.rpc_url.clone(), c.native_symbol.clone()))
            .collect();

        if chains.is_empty() {
            self.ui.set_status("没有配置 Solana 系列链");
            return;
        }

        self.ui.stk_from_address = address;
        self.ui.stk_wallet_index = wallet_index;
        self.ui.stk_account_index = account_index;
        self.ui.stk_create_type = create_type;
        self.ui.stk_result = None;

        // 只有一条链时跳过选择
        if chains.len() == 1 {
            let (_id, _name, rpc, symbol) = chains[0].clone();
            self.ui.stk_rpc_url = rpc;
            self.ui.stk_native_symbol = symbol;
            self.proceed_after_chain_select();
            return;
        }

        self.ui.screen = Screen::Staking;
        self.ui.stk_solana_chains = chains;
        self.ui.stk_chain_selected = 0;
        self.ui.stk_step = StakingStep::SelectChain;
        self.ui.clear_status();
    }

    /// 选择链后，检查地址为空，然后进入 fee payer 选择
    fn proceed_after_chain_select(&mut self) {
        let address = &self.ui.stk_from_address;

        // 检查账户 owner —— 必须是空的系统账户
        let owner = self
            .balance_cache
            .get(address)
            .and_then(|p| p.account_owner.as_deref())
            .unwrap_or(crate::chain::solana::SYSTEM_PROGRAM_STR);
        if owner != crate::chain::solana::SYSTEM_PROGRAM_STR && !owner.is_empty() {
            let type_name = match self.ui.stk_create_type {
                StakingCreateType::Vote => "Vote",
                StakingCreateType::Stake => "Stake",
            };
            self.ui.screen = Screen::Staking;
            self.ui.stk_step = StakingStep::Result;
            self.ui.stk_result =
                Some((false, format!("该地址已被其他程序占用，无法创建 {type_name} 账户")));
            return;
        }

        // 检查地址必须无余额（空地址才能创建账户）
        let has_balance = self
            .balance_cache
            .get(address)
            .map(|p| p.chains.iter().any(|c| c.native_balance > 0))
            .unwrap_or(false);
        if has_balance {
            self.ui.screen = Screen::Staking;
            self.ui.stk_step = StakingStep::Result;
            self.ui.stk_result =
                Some((false, "该地址有余额，不能用于创建新账户。请选择一个空地址".to_string()));
            return;
        }

        // 构建有余额的 SOL 地址列表作为 fee payer 候选
        let fee_payer_list = self.build_fee_payer_list();
        if fee_payer_list.is_empty() {
            self.ui.screen = Screen::Staking;
            self.ui.stk_step = StakingStep::Result;
            self.ui.stk_result =
                Some((false, "没有可用的 SOL 地址作为 Fee Payer（需要有余额的地址）".to_string()));
            return;
        }

        self.ui.screen = Screen::Staking;
        self.ui.stk_fee_payer_list = fee_payer_list;
        self.ui.stk_fee_payer_selected = 0;
        self.ui.stk_pending_step = None; // creation flow uses stk_create_type
        self.ui.stk_step = StakingStep::SelectFeePayer;
        self.ui.clear_status();
    }

    /// 构建有余额的 SOL 地址列表（用于 fee payer 选择）
    fn build_fee_payer_list(&self) -> Vec<(String, String, u128, usize, usize)> {
        let mut list = Vec::new();
        let store = match self.store.as_ref() {
            Some(s) => s,
            None => return list,
        };
        for (wi, wallet) in store.wallets.iter().enumerate() {
            if wallet.hidden {
                continue;
            }
            // 收集该钱包的 SOL 地址列表: (address, label, account_index)
            let sol_addrs: Vec<(String, String, usize)> = match &wallet.wallet_type {
                WalletType::Mnemonic { sol_accounts, .. } => {
                    sol_accounts.iter().enumerate()
                        .filter(|(_, acc)| !acc.hidden)
                        .map(|(ai, acc)| {
                            let label = acc.label.as_deref().unwrap_or(&wallet.name).to_string();
                            (acc.address.clone(), label, ai)
                        })
                        .collect()
                }
                WalletType::PrivateKey {
                    chain_type: ChainType::Solana,
                    address,
                    label,
                    hidden,
                    ..
                } => {
                    if *hidden { vec![] } else {
                        let lbl = label.as_deref().unwrap_or(&wallet.name).to_string();
                        vec![(address.clone(), lbl, 0)]
                    }
                }
                _ => vec![],
            };
            for (addr, label, ai) in sol_addrs {
                if addr == self.ui.stk_from_address {
                    continue;
                }
                // 排除 Vote/Stake 账户
                let is_special_account = self
                    .balance_cache
                    .get(&addr)
                    .and_then(|p| p.account_owner.as_deref())
                    .is_some_and(|o| o == crate::chain::solana::VOTE_PROGRAM || o == crate::chain::solana::STAKE_PROGRAM);
                if is_special_account {
                    continue;
                }
                let has_balance = self
                    .balance_cache
                    .get(&addr)
                    .map(|p| p.chains.iter().any(|c| c.native_balance > 0))
                    .unwrap_or(false);
                if has_balance {
                    let balance_lamports = self
                        .balance_cache
                        .get(&addr)
                        .and_then(|p| p.chains.iter().find(|c| c.native_balance > 0))
                        .map(|c| c.native_balance)
                        .unwrap_or(0);
                    list.push((addr, label, balance_lamports, wi, ai));
                }
            }
        }
        list
    }

    /// fee payer 选择后，进入具体创建步骤
    /// 进入 fee payer 选择，完成后进入 next_step
    fn enter_fee_payer_select(&mut self, next_step: StakingStep) {
        let list = self.build_fee_payer_list();
        if list.is_empty() {
            self.ui.set_status("没有可用的 Fee Payer（需要有余额的 SOL 地址）");
            return;
        }
        if list.len() == 1 {
            // 只有一个，自动选择
            let (_, _, _, wi, ai) = list[0].clone();
            self.ui.stk_fee_payer_wallet_index = wi;
            self.ui.stk_fee_payer_account_index = ai;
            self.ui.stk_fee_payer_list = list;
            self.ui.stk_pending_step = None;
            self.ui.stk_step = next_step;
            self.ui.clear_status();
        } else {
            self.ui.stk_fee_payer_list = list;
            self.ui.stk_fee_payer_selected = 0;
            self.ui.stk_pending_step = Some(next_step);
            self.ui.stk_step = StakingStep::SelectFeePayer;
            self.ui.clear_status();
        }
    }

    fn proceed_after_fee_payer_select(&mut self) {
        let (_, _, _, wi, ai) = self.ui.stk_fee_payer_list[self.ui.stk_fee_payer_selected].clone();
        self.ui.stk_fee_payer_wallet_index = wi;
        self.ui.stk_fee_payer_account_index = ai;

        if let Some(step) = self.ui.stk_pending_step.take() {
            self.ui.stk_step = step;
            self.ui.clear_status();
            return;
        }

        // Legacy: creation flow
        match self.ui.stk_create_type {
            StakingCreateType::Vote => {
                self.ui.stk_step = StakingStep::CreateVoteInputIdentity;
                self.ui.stk_identity_input.clear();
                self.ui.stk_withdrawer_input.clear();
                self.ui.stk_confirm_password.clear();
            }
            StakingCreateType::Stake => {
                self.ui.stk_step = StakingStep::CreateStakeInputAmount;
                self.ui.stk_amount_input.clear();
                self.ui.stk_lockup_days_input.clear();
                self.ui.stk_confirm_password.clear();
            }
        }
        self.ui.clear_status();
    }

    fn fetch_vote_account_info(&self, address: &str) {
        let rpc_url = self.ui.stk_rpc_url.clone();
        let addr = address.to_string();
        let tx = self.bg_tx.clone();
        self.runtime.spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap();
            match crate::staking::sol_staking::fetch_vote_account(&client, &rpc_url, &addr).await {
                Ok(info) => { let _ = tx.send(BgMessage::VoteAccountFetched(info)); }
                Err(e) => { let _ = tx.send(BgMessage::StakingFetchError(e)); }
            }
        });
    }

    fn fetch_stake_account_info(&self, address: &str) {
        let rpc_url = self.ui.stk_rpc_url.clone();
        let addr = address.to_string();
        let tx = self.bg_tx.clone();
        self.runtime.spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap();
            match crate::staking::sol_staking::fetch_stake_account(&client, &rpc_url, &addr).await {
                Ok(info) => { let _ = tx.send(BgMessage::StakeAccountFetched(info)); }
                Err(e) => { let _ = tx.send(BgMessage::StakingFetchError(e)); }
            }
        });
    }

    /// 获取默认 SOL RPC URL（取第一条 Solana 链的配置）
    fn get_sol_rpc_url(&self) -> String {
        self.config
            .chains
            .solana
            .first()
            .map(|c| c.rpc_url.clone())
            .unwrap_or_default()
    }

    /// 根据地址的 balance_cache 中的 chain_id 找到对应的 RPC URL
    /// 根据 account_owner 所在链找 RPC URL（Vote/Stake 账户只属于一条链）
    fn get_rpc_url_for_address(&self, address: &str) -> String {
        if let Some(portfolio) = self.balance_cache.get(address) {
            // 优先使用 account_owner 来源链
            if let Some(chain_id) = &portfolio.account_owner_chain_id
                && let Some(cfg) = self.config.chains.solana.iter().find(|c| &c.id == chain_id) {
                    return cfg.rpc_url.clone();
                }
        }
        self.get_sol_rpc_url()
    }

    /// 根据 account_owner 所在链找 native_symbol
    fn get_native_symbol_for_address(&self, address: &str) -> String {
        if let Some(portfolio) = self.balance_cache.get(address)
            && let Some(chain_id) = &portfolio.account_owner_chain_id
                && let Some(chain_bal) = portfolio.chains.iter().find(|c| c.chain_id == *chain_id) {
                    return chain_bal.native_symbol.clone();
                }
        self.config.chains.solana.first().map(|c| c.native_symbol.clone()).unwrap_or_else(|| "SOL".to_string())
    }

    /// 获取 fee payer 的 SOL 私钥
    fn get_fee_payer_private_key(&mut self) -> Option<Vec<u8>> {
        self.get_sol_private_key_by_index(
            self.ui.stk_fee_payer_wallet_index,
            self.ui.stk_fee_payer_account_index,
        )
    }

    /// 获取 staking 操作的 SOL 私钥
    fn get_staking_private_key(&mut self) -> Option<Vec<u8>> {
        self.get_sol_private_key_by_index(
            self.ui.stk_wallet_index,
            self.ui.stk_account_index,
        )
    }

    /// 通用：通过 wallet_index + account_index 获取 SOL 私钥（支持助记词和私钥钱包）
    fn get_sol_private_key_by_index(&mut self, wallet_index: usize, account_index: usize) -> Option<Vec<u8>> {
        enum KeySource {
            Mnemonic { encrypted: String, derivation_index: u32 },
            PrivateKey { encrypted: String },
        }
        let source = {
            let store = self.store.as_ref()?;
            let wallet = store.wallets.get(wallet_index)?;
            match &wallet.wallet_type {
                WalletType::Mnemonic { encrypted_mnemonic, sol_accounts, .. } => {
                    let acc = sol_accounts.get(account_index)?;
                    KeySource::Mnemonic {
                        encrypted: encrypted_mnemonic.clone(),
                        derivation_index: acc.derivation_index,
                    }
                }
                WalletType::PrivateKey {
                    chain_type: ChainType::Solana,
                    encrypted_private_key,
                    ..
                } => KeySource::PrivateKey { encrypted: encrypted_private_key.clone() },
                _ => return None,
            }
        };
        match source {
            KeySource::Mnemonic { encrypted, derivation_index } => {
                let mut phrase = self.decrypt_inner_secret(&encrypted)?;
                let mut seed = mnemonic::mnemonic_to_seed(&phrase, "").ok()?;
                phrase.clear_sensitive();
                let result = sol_keys::derive_sol_private_key(&seed, derivation_index).ok();
                seed.clear_sensitive();
                result
            }
            KeySource::PrivateKey { encrypted } => {
                let mut pk_str = self.decrypt_inner_secret(&encrypted)?;
                let mut bytes = bs58::decode(&pk_str).into_vec().ok()?;
                pk_str.clear_sensitive();
                let result = match bytes.len() {
                    64 => Some(bytes[..32].to_vec()),
                    32 => Some(bytes.clone()),
                    _ => None,
                };
                bytes.clear_sensitive();
                result
            }
        }
    }

    fn handle_staking_key(&mut self, key: KeyEvent) {
        match self.ui.stk_step {
            StakingStep::SelectChain => self.handle_stk_select_chain(key),
            StakingStep::SelectFeePayer => self.handle_stk_select_fee_payer(key),
            StakingStep::CreateVoteInputIdentity => self.handle_stk_text_input(key, StakingTextField::Identity),
            StakingStep::CreateVoteInputWithdrawer => self.handle_stk_text_input(key, StakingTextField::Withdrawer),
            StakingStep::CreateVoteConfirm => self.handle_stk_confirm(key),
            StakingStep::CreateStakeInputAmount => self.handle_stk_text_input(key, StakingTextField::Amount),
            StakingStep::CreateStakeInputLockup => self.handle_stk_text_input(key, StakingTextField::LockupDays),
            StakingStep::CreateStakeConfirm => self.handle_stk_confirm(key),
            StakingStep::VoteDetail => self.handle_vote_detail_key(key),
            StakingStep::StakeDetail => self.handle_stake_detail_key(key),
            StakingStep::VoteAuthorize | StakingStep::StakeAuthorize => {
                self.handle_stk_text_input(key, StakingTextField::NewAuthority);
            }
            StakingStep::StakeDelegateInput => {
                self.handle_stk_text_input(key, StakingTextField::VoteAccount);
            }
            StakingStep::StakeDeactivateConfirm | StakingStep::Confirm => {
                self.handle_stk_confirm(key);
            }
            StakingStep::VoteWithdrawInput | StakingStep::StakeWithdrawInput => {
                self.handle_stk_text_input(key, StakingTextField::WithdrawAmount);
            }
            StakingStep::Submitting => {} // 等待后台
            StakingStep::Result => {
                // 任意键返回主页
                self.ui.back_to_main();
                self.trigger_balance_refresh();
            }
        }
    }

    fn handle_stk_select_chain(&mut self, key: KeyEvent) {
        let count = self.ui.stk_solana_chains.len();
        match key.code {
            KeyCode::Up => {
                if self.ui.stk_chain_selected > 0 {
                    self.ui.stk_chain_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.ui.stk_chain_selected + 1 < count {
                    self.ui.stk_chain_selected += 1;
                }
            }
            KeyCode::Enter => {
                if count == 0 {
                    return;
                }
                let (_id, _name, rpc, symbol) =
                    self.ui.stk_solana_chains[self.ui.stk_chain_selected].clone();
                self.ui.stk_rpc_url = rpc;
                self.ui.stk_native_symbol = symbol;
                self.proceed_after_chain_select();
            }
            KeyCode::Esc => {
                self.ui.back_to_main();
            }
            _ => {}
        }
    }

    fn handle_stk_select_fee_payer(&mut self, key: KeyEvent) {
        let count = self.ui.stk_fee_payer_list.len();
        match key.code {
            KeyCode::Up => {
                if self.ui.stk_fee_payer_selected > 0 {
                    self.ui.stk_fee_payer_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.ui.stk_fee_payer_selected + 1 < count {
                    self.ui.stk_fee_payer_selected += 1;
                }
            }
            KeyCode::Enter => {
                if count == 0 {
                    return;
                }
                self.proceed_after_fee_payer_select();
            }
            KeyCode::Esc => {
                self.ui.back_to_main();
            }
            _ => {}
        }
    }

    fn handle_vote_detail_key(&mut self, key: KeyEvent) {
        let menu_count: usize = 4; // 修改Voter, 修改Withdrawer, 提取, 修改备注
        match key.code {
            KeyCode::Esc => self.ui.back_to_main(),
            KeyCode::Char('r') => {
                let addr = self.ui.stk_from_address.clone();
                self.ui.stk_vote_info = None;
                self.ui.stk_fetch_error = None;
                self.fetch_vote_account_info(&addr);
            }
            KeyCode::Up => {
                if self.ui.stk_detail_selected > 0 {
                    self.ui.stk_detail_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.ui.stk_detail_selected + 1 < menu_count {
                    self.ui.stk_detail_selected += 1;
                }
            }
            KeyCode::Enter if self.ui.stk_vote_info.is_some() => {
                match self.ui.stk_detail_selected {
                    0 => {
                        self.ui.stk_authorize_type = 0;
                        self.ui.stk_new_authority_input.clear();
                        self.ui.stk_step = StakingStep::VoteAuthorize;
                    }
                    1 => {
                        self.ui.stk_authorize_type = 1;
                        self.ui.stk_new_authority_input.clear();
                        self.ui.stk_step = StakingStep::VoteAuthorize;
                    }
                    2 => {
                        self.ui.stk_target_address = self.ui.stk_from_address.clone();
                        self.ui.stk_amount_input.clear();
                        self.ui.stk_step = StakingStep::VoteWithdrawInput;
                    }
                    3 => self.enter_staking_edit_label(),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn handle_stake_detail_key(&mut self, key: KeyEvent) {
        let menu_count: usize = 6; // Staker, Withdrawer, Delegate, Deactivate, Withdraw, 修改备注
        match key.code {
            KeyCode::Esc => self.ui.back_to_main(),
            KeyCode::Char('r') => {
                let addr = self.ui.stk_from_address.clone();
                self.ui.stk_stake_info = None;
                self.ui.stk_fetch_error = None;
                self.fetch_stake_account_info(&addr);
            }
            KeyCode::Up => {
                if self.ui.stk_detail_selected > 0 {
                    self.ui.stk_detail_selected -= 1;
                }
            }
            KeyCode::Down => {
                if self.ui.stk_detail_selected + 1 < menu_count {
                    self.ui.stk_detail_selected += 1;
                }
            }
            KeyCode::Enter if self.ui.stk_stake_info.is_some() => {
                match self.ui.stk_detail_selected {
                    0 => {
                        self.ui.stk_authorize_type = 0;
                        self.ui.stk_new_authority_input.clear();
                        self.ui.stk_step = StakingStep::StakeAuthorize;
                    }
                    1 => {
                        self.ui.stk_authorize_type = 1;
                        self.ui.stk_new_authority_input.clear();
                        self.ui.stk_step = StakingStep::StakeAuthorize;
                    }
                    2 => {
                        self.ui.stk_vote_account_input.clear();
                        self.ui.stk_step = StakingStep::StakeDelegateInput;
                    }
                    3 => {
                        self.ui.stk_confirm_password.clear();
                        self.enter_fee_payer_select(StakingStep::StakeDeactivateConfirm);
                    }
                    4 => {
                        self.ui.stk_target_address = self.ui.stk_from_address.clone();
                        self.ui.stk_amount_input.clear();
                        self.ui.stk_step = StakingStep::StakeWithdrawInput;
                    }
                    5 => self.enter_staking_edit_label(),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn enter_staking_edit_label(&mut self) {
        use crate::tui::state::{ActionContext, AddWalletStep, InputPurpose};
        let wi = self.ui.stk_wallet_index;
        let ai = self.ui.stk_account_index;
        self.ui.action_context = Some(ActionContext::MnemonicAddress {
            wallet_index: wi,
            chain_type: crate::storage::data::ChainType::Solana,
            account_index: ai,
        });
        self.ui.input_buffer = self.get_current_label(&self.ui.action_context.clone()).unwrap_or_default();
        self.ui.input_purpose = Some(InputPurpose::EditLabel);
        self.ui.add_wallet_step = AddWalletStep::InputName;
        self.ui.screen = Screen::TextInput;
    }

    fn handle_stk_text_input(&mut self, key: KeyEvent, field: StakingTextField) {
        match key.code {
            KeyCode::Esc => {
                // 返回上一步
                match field {
                    StakingTextField::Identity => self.ui.back_to_main(),
                    StakingTextField::Withdrawer => {
                        self.ui.stk_step = StakingStep::CreateVoteInputIdentity;
                    }
                    StakingTextField::Amount => self.ui.back_to_main(),
                    StakingTextField::LockupDays => {
                        self.ui.stk_step = StakingStep::CreateStakeInputAmount;
                    }
                    StakingTextField::NewAuthority => {
                        // 返回对应详情页
                        if self.ui.stk_step == StakingStep::VoteAuthorize {
                            self.ui.stk_step = StakingStep::VoteDetail;
                        } else {
                            self.ui.stk_step = StakingStep::StakeDetail;
                        }
                    }
                    StakingTextField::VoteAccount => {
                        self.ui.stk_step = StakingStep::StakeDetail;
                    }
                    StakingTextField::WithdrawAmount => {
                        if self.ui.stk_step == StakingStep::VoteWithdrawInput {
                            self.ui.stk_step = StakingStep::VoteDetail;
                        } else {
                            self.ui.stk_step = StakingStep::StakeDetail;
                        }
                    }
                }
            }
            KeyCode::Enter => {
                match field {
                    StakingTextField::Identity => {
                        if self.ui.stk_identity_input.is_empty() {
                            self.ui.set_status("请输入 Identity 私钥");
                            return;
                        }
                        self.ui.stk_step = StakingStep::CreateVoteInputWithdrawer;
                        self.ui.clear_status();
                    }
                    StakingTextField::Withdrawer => {
                        // 空则默认当前地址
                        if self.ui.stk_withdrawer_input.is_empty() {
                            self.ui.stk_withdrawer_input = self.ui.stk_from_address.clone();
                        }
                        self.ui.stk_confirm_password.clear();
                        self.ui.stk_step = StakingStep::CreateVoteConfirm;
                        self.ui.clear_status();
                    }
                    StakingTextField::Amount => {
                        if self.ui.stk_amount_input.is_empty() {
                            self.ui.set_status("请输入质押数量");
                            return;
                        }
                        self.ui.stk_lockup_days_input.clear();
                        self.ui.stk_step = StakingStep::CreateStakeInputLockup;
                        self.ui.clear_status();
                    }
                    StakingTextField::LockupDays => {
                        // 空或0都表示不锁仓，直接进入确认
                        self.ui.stk_confirm_password.clear();
                        self.ui.stk_step = StakingStep::CreateStakeConfirm;
                        self.ui.clear_status();
                    }
                    StakingTextField::NewAuthority => {
                        if self.ui.stk_new_authority_input.is_empty() {
                            self.ui.set_status("请输入新的权限地址");
                            return;
                        }
                        self.ui.stk_confirm_password.clear();
                        self.ui.stk_confirm_op = if self.ui.stk_step == StakingStep::VoteAuthorize {
                            StakingOp::VoteAuthorize
                        } else {
                            StakingOp::StakeAuthorize
                        };
                        self.enter_fee_payer_select(StakingStep::Confirm);
                    }
                    StakingTextField::VoteAccount => {
                        if self.ui.stk_vote_account_input.is_empty() {
                            self.ui.set_status("请输入 Vote Account 地址");
                            return;
                        }
                        self.ui.stk_confirm_password.clear();
                        self.ui.stk_confirm_op = StakingOp::StakeDelegate;
                        self.enter_fee_payer_select(StakingStep::Confirm);
                    }
                    StakingTextField::WithdrawAmount => {
                        if self.ui.stk_amount_input.is_empty() {
                            self.ui.set_status("请输入提取数量");
                            return;
                        }
                        self.ui.stk_confirm_password.clear();
                        self.ui.stk_confirm_op = if self.ui.stk_step == StakingStep::VoteWithdrawInput {
                            StakingOp::VoteWithdraw
                        } else {
                            StakingOp::StakeWithdraw
                        };
                        self.enter_fee_payer_select(StakingStep::Confirm);
                    }
                }
            }
            KeyCode::Char(c) => {
                self.ui.clear_status();
                let buf = match field {
                    StakingTextField::Identity => &mut self.ui.stk_identity_input,
                    StakingTextField::Withdrawer => &mut self.ui.stk_withdrawer_input,
                    StakingTextField::Amount | StakingTextField::WithdrawAmount => &mut self.ui.stk_amount_input,
                    StakingTextField::LockupDays => &mut self.ui.stk_lockup_days_input,
                    StakingTextField::NewAuthority => &mut self.ui.stk_new_authority_input,
                    StakingTextField::VoteAccount => &mut self.ui.stk_vote_account_input,
                };
                buf.push(c);
            }
            KeyCode::Backspace => {
                let buf = match field {
                    StakingTextField::Identity => &mut self.ui.stk_identity_input,
                    StakingTextField::Withdrawer => &mut self.ui.stk_withdrawer_input,
                    StakingTextField::Amount | StakingTextField::WithdrawAmount => &mut self.ui.stk_amount_input,
                    StakingTextField::LockupDays => &mut self.ui.stk_lockup_days_input,
                    StakingTextField::NewAuthority => &mut self.ui.stk_new_authority_input,
                    StakingTextField::VoteAccount => &mut self.ui.stk_vote_account_input,
                };
                buf.pop();
            }
            _ => {}
        }
    }

    fn handle_stk_confirm(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                // 返回上一步
                match self.ui.stk_step {
                    StakingStep::CreateVoteConfirm => {
                        self.ui.stk_step = StakingStep::CreateVoteInputWithdrawer;
                    }
                    StakingStep::CreateStakeConfirm => {
                        self.ui.stk_step = StakingStep::CreateStakeInputAmount;
                    }
                    StakingStep::StakeDeactivateConfirm => {
                        self.ui.stk_step = StakingStep::StakeDetail;
                    }
                    _ => {
                        // Confirm (authorize/delegate/withdraw) → 返回详情
                        self.ui.stk_step = StakingStep::StakeDetail;
                    }
                }
            }
            KeyCode::Enter => {
                // 密码验证
                let password = self.ui.stk_confirm_password.clone();
                if password.is_empty() {
                    self.ui.set_status("请输入密码");
                    return;
                }
                // 验证密码正确性
                if let Some(saved_pw) = &self.password {
                    if password.as_bytes() != saved_pw.as_slice() {
                        self.ui.set_status("密码错误");
                        self.ui.stk_confirm_password.clear();
                        return;
                    }
                } else {
                    self.ui.set_status("密码未设置");
                    return;
                }
                // 获取账户私钥
                let private_key = match self.get_staking_private_key() {
                    Some(pk) => pk,
                    None => {
                        self.ui.set_status("无法获取私钥");
                        return;
                    }
                };
                // 所有操作都需要 fee payer（program-owned 账户不能付手续费）
                let fee_payer_key = match self.get_fee_payer_private_key() {
                    Some(pk) => pk,
                    None => {
                        self.ui.set_status("无法获取 Fee Payer 私钥");
                        return;
                    }
                };

                let current_step = self.ui.stk_step.clone();
                self.ui.stk_step = StakingStep::Submitting;

                let rpc_url = self.ui.stk_rpc_url.clone();
                let tx = self.bg_tx.clone();
                let identity_input = self.ui.stk_identity_input.clone();
                let withdrawer_input = self.ui.stk_withdrawer_input.clone();
                let amount_input = self.ui.stk_amount_input.clone();
                let lockup_days_input = self.ui.stk_lockup_days_input.clone();
                let new_authority_input = self.ui.stk_new_authority_input.clone();
                let authorize_type = self.ui.stk_authorize_type;
                let vote_account_input = self.ui.stk_vote_account_input.clone();
                let target_address = self.ui.stk_target_address.clone();
                let confirm_op = self.ui.stk_confirm_op.clone();

                self.runtime.spawn(async move {
                    let client = reqwest::Client::builder()
                        .timeout(std::time::Duration::from_secs(30))
                        .build()
                        .unwrap();

                    let result = match current_step {
                        StakingStep::CreateVoteConfirm => {
                            crate::staking::sol_staking::create_vote_account(
                                &client,
                                &rpc_url,
                                &private_key,
                                &fee_payer_key,
                                &identity_input,
                                &withdrawer_input,
                            )
                            .await
                        }
                        StakingStep::CreateStakeConfirm => {
                            let lockup_days: u64 = lockup_days_input.trim().parse().unwrap_or(0);
                            crate::staking::sol_staking::create_stake_account(
                                &client,
                                &rpc_url,
                                &private_key,
                                &fee_payer_key,
                                &amount_input,
                                lockup_days,
                            )
                            .await
                        }
                        StakingStep::StakeDeactivateConfirm => {
                            crate::staking::sol_staking::stake_deactivate(
                                &client,
                                &rpc_url,
                                &private_key,
                                &fee_payer_key,
                            )
                            .await
                        }
                        StakingStep::Confirm => match confirm_op {
                            StakingOp::VoteAuthorize => {
                                crate::staking::sol_staking::vote_authorize(
                                    &client,
                                    &rpc_url,
                                    &private_key,
                                    &fee_payer_key,
                                    &new_authority_input,
                                    authorize_type,
                                )
                                .await
                            }
                            StakingOp::VoteWithdraw => {
                                crate::staking::sol_staking::vote_withdraw(
                                    &client,
                                    &rpc_url,
                                    &private_key,
                                    &fee_payer_key,
                                    &target_address,
                                    &amount_input,
                                )
                                .await
                            }
                            StakingOp::StakeAuthorize => {
                                crate::staking::sol_staking::stake_authorize(
                                    &client,
                                    &rpc_url,
                                    &private_key,
                                    &fee_payer_key,
                                    &new_authority_input,
                                    authorize_type,
                                )
                                .await
                            }
                            StakingOp::StakeDelegate => {
                                crate::staking::sol_staking::stake_delegate(
                                    &client,
                                    &rpc_url,
                                    &private_key,
                                    &fee_payer_key,
                                    &vote_account_input,
                                )
                                .await
                            }
                            StakingOp::StakeWithdraw => {
                                crate::staking::sol_staking::stake_withdraw(
                                    &client,
                                    &rpc_url,
                                    &private_key,
                                    &fee_payer_key,
                                    &target_address,
                                    &amount_input,
                                )
                                .await
                            }
                        },
                        _ => Err("未知操作".to_string()),
                    };

                    let (success, message) = match result {
                        Ok(sig) => (true, format!("交易成功!\n签名: {sig}")),
                        Err(e) => (false, format!("交易失败: {e}")),
                    };
                    let _ = tx.send(BgMessage::StakingOpComplete { success, message });
                });
            }
            KeyCode::Char(c) => {
                self.ui.clear_status();
                self.ui.stk_confirm_password.push(c);
            }
            KeyCode::Backspace => {
                self.ui.stk_confirm_password.pop();
            }
            _ => {}
        }
    }
}

enum StakingTextField {
    Identity,
    Withdrawer,
    Amount,
    LockupDays,
    NewAuthority,
    VoteAccount,
    WithdrawAmount,
}

/// 空存储，用于渲染时的默认值
static EMPTY_STORE: std::sync::LazyLock<WalletStore> =
    std::sync::LazyLock::new(WalletStore::new);
