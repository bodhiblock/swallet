use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;
use zeroize::Zeroize;

use crate::chain::{registry, BalanceCache};
use crate::config::AppConfig;
use crate::crypto::{eth_keys, mnemonic, sol_keys};
use crate::multisig::{self, ProposalType};
use crate::storage::data::{
    ChainType, DerivedAccount, MultisigAccount, Wallet, WalletStore, WalletType, WatchOnlySource,
};
use crate::storage::encrypted;
use crate::transfer::{self, AssetKind, TransferableAsset};
use crate::tui::event;
use crate::tui::screens::{
    action_menu, add_wallet, dex as dex_screen, main_screen,
    multisig as multisig_screen, transfer as transfer_screen, unlock,
};
use crate::tui::state::{
    ActionContext, ActionItem, AddWalletOption, AddWalletStep, InputPurpose, MultisigStep, Screen,
    TransferStep, UiState, UnlockMode, VoteAction,
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
}

pub struct App {
    pub config: AppConfig,
    pub store: Option<WalletStore>,
    pub ui: UiState,
    pub balance_cache: BalanceCache,
    /// 解锁后保存密码用于后续数据保存
    password: Option<Vec<u8>>,
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
    pub fn new(config: AppConfig) -> Self {
        let has_data = encrypted::data_file_exists();
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
                multisig_screen::render(frame, &self.ui, multisigs);
            }
            Screen::Dex => {
                dex_screen::render(frame);
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
                    // 如果是首次导入（从 InputAddress 步骤来的），保存到 store
                    if self.ui.ms_step == MultisigStep::InputAddress
                        || self.ui.ms_step == MultisigStep::List
                    {
                        self.save_multisig_to_store(&info);
                    }
                    self.ui.ms_current_info = Some(info);
                    self.ui.ms_step = MultisigStep::ViewDetail;
                    self.ui.clear_status();
                }
                BgMessage::MultisigFetchError(err) => {
                    self.ui.set_status(format!("获取多签信息失败: {err}"));
                    self.ui.ms_step = MultisigStep::List;
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
                match encrypted::save(&store, &pw) {
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
                match encrypted::load(&pw) {
                    Ok(store) => {
                        self.store = Some(store);
                        self.password = Some(pw);
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
                self.ui.set_status("正在刷新余额...");
            }
            KeyCode::Char('m') => {
                self.ui.enter_multisig();
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
                self.ui
                    .enter_action_menu(ActionContext::Wallet { wallet_index: wi });
            }
            SelectionTarget::MnemonicAddress {
                wallet_index,
                chain_type,
                account_index,
            } => {
                self.ui.enter_action_menu(ActionContext::MnemonicAddress {
                    wallet_index,
                    chain_type,
                    account_index,
                });
            }
            SelectionTarget::PrivateKeyAddress(wi) => {
                self.ui
                    .enter_action_menu(ActionContext::PrivateKeyAddress { wallet_index: wi });
            }
            SelectionTarget::WatchAddress(wi) => {
                self.ui
                    .enter_action_menu(ActionContext::WatchAddress { wallet_index: wi });
            }
            SelectionTarget::AddWallet => {
                self.ui.enter_add_wallet();
            }
        }
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
                // 选完链类型后输入名称
                self.ui.add_wallet_step = AddWalletStep::InputName;
                self.ui.input_buffer.clear();
            }
            KeyCode::Esc => {
                self.ui.add_wallet_step = AddWalletStep::SelectType;
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
                // 编辑标签时按 Esc 直接返回主界面
                if self.ui.input_purpose == Some(InputPurpose::EditLabel) {
                    self.ui.input_purpose = None;
                    self.ui.back_to_main();
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
        seed.zeroize();
        self.ui.mnemonic_buffer.zeroize();
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
                    self.add_derived_address(wallet_index);
                }
            }
            ActionItem::EditName | ActionItem::EditAddressLabel => {
                self.ui.input_buffer.clear();
                self.ui.input_purpose = Some(InputPurpose::EditLabel);
                self.ui.add_wallet_step = AddWalletStep::InputName;
                self.ui.screen = Screen::TextInput;
            }
            ActionItem::HideWallet => {
                let wi = match context {
                    ActionContext::Wallet { wallet_index } => wallet_index,
                    ActionContext::PrivateKeyAddress { wallet_index } => wallet_index,
                    _ => return,
                };
                self.toggle_wallet_hidden(wi, true);
            }
            ActionItem::HideAddress => {
                if let ActionContext::MnemonicAddress {
                    wallet_index,
                    chain_type,
                    account_index,
                } = context
                {
                    self.toggle_address_hidden(wallet_index, &chain_type, account_index, true);
                }
            }
            ActionItem::DeleteWatchWallet => {
                if let ActionContext::WatchAddress { wallet_index } = context {
                    self.delete_wallet(wallet_index);
                }
            }
            ActionItem::MoveUp | ActionItem::MoveDown => {
                if let ActionContext::Wallet { wallet_index } = context {
                    self.move_wallet(wallet_index, action == ActionItem::MoveUp);
                }
            }
        }
    }

    fn add_derived_address(&mut self, wallet_index: usize) {
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
        let mut phrase = match self.decrypt_inner_secret(&encrypted_mnemonic) {
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

        phrase.zeroize();

        // 派生新地址
        let eth_addr = eth_keys::derive_eth_address(&seed, eth_idx).ok();
        let sol_addr = sol_keys::derive_sol_address(&seed, sol_idx).ok();
        seed.zeroize();

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
                Some(ActionContext::PrivateKeyAddress { wallet_index }) => {
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
                _ => {}
            }
            let _ = self.save_store_inner();
        }
        self.ui.back_to_main();
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
            ActionContext::PrivateKeyAddress { wallet_index } => {
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

    fn get_transfer_private_key(&self) -> Option<Vec<u8>> {
        let store = self.store.as_ref()?;
        let wallet = store.wallets.get(self.ui.transfer_wallet_index)?;

        match &wallet.wallet_type {
            WalletType::Mnemonic {
                encrypted_mnemonic,
                eth_accounts,
                sol_accounts,
                ..
            } => {
                let mut phrase = self.decrypt_inner_secret(encrypted_mnemonic)?;
                let mut seed = mnemonic::mnemonic_to_seed(&phrase, "").ok()?;
                phrase.zeroize();

                let account_index = self.ui.transfer_account_index?;
                let result = match self.ui.transfer_chain_type {
                    ChainType::Ethereum => {
                        let derivation_index =
                            eth_accounts.get(account_index)?.derivation_index;
                        eth_keys::derive_eth_private_key(&seed, derivation_index).ok()
                    }
                    ChainType::Solana => {
                        let derivation_index =
                            sol_accounts.get(account_index)?.derivation_index;
                        sol_keys::derive_sol_private_key(&seed, derivation_index).ok()
                    }
                };
                seed.zeroize();
                result
            }
            WalletType::PrivateKey {
                encrypted_private_key,
                chain_type,
                ..
            } => {
                let mut pk_str = self.decrypt_inner_secret(encrypted_private_key)?;
                let result = match chain_type {
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
                pk_str.zeroize();
                result
            }
            _ => None,
        }
    }

    // ========== 多签 ==========

    fn handle_multisig_key(&mut self, key: KeyEvent) {
        match self.ui.ms_step {
            MultisigStep::List => self.handle_ms_list_key(key),
            MultisigStep::InputAddress => self.handle_ms_input_address_key(key),
            MultisigStep::ViewDetail => self.handle_ms_detail_key(key),
            MultisigStep::ViewProposals => self.handle_ms_proposals_key(key),
            MultisigStep::ViewProposal => self.handle_ms_proposal_detail_key(key),
            MultisigStep::SelectProposalType => self.handle_ms_select_proposal_type_key(key),
            MultisigStep::InputTransferTo => self.handle_ms_text_input_key(key, MsInputField::TransferTo),
            MultisigStep::InputTransferAmount => self.handle_ms_text_input_key(key, MsInputField::TransferAmount),
            MultisigStep::ConfirmCreate | MultisigStep::ConfirmVote => {
                self.handle_ms_confirm_key(key);
            }
            MultisigStep::Submitting => {} // 忽略输入
            MultisigStep::Result => {
                // 任意键返回多签详情
                if self.ui.ms_current_info.is_some() {
                    self.ui.ms_step = MultisigStep::ViewDetail;
                } else {
                    self.ui.ms_step = MultisigStep::List;
                }
                self.ui.clear_status();
            }
        }
    }

    fn handle_ms_list_key(&mut self, key: KeyEvent) {
        let visible_count = self
            .store
            .as_ref()
            .map(|s| s.multisigs.iter().filter(|m| !m.hidden).count())
            .unwrap_or(0);
        let total_items = visible_count + 2; // +1 导入Squads, +1 导入Safe(占位)

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
                    // "导入 Squads 多签"
                    self.ui.ms_step = MultisigStep::InputAddress;
                    self.ui.ms_input_address.clear();
                    self.ui.clear_status();
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
                // 验证是有效的 base58 地址
                if bs58::decode(&self.ui.ms_input_address).into_vec().is_err() {
                    self.ui.set_status("无效的 Solana 地址");
                    return;
                }
                self.import_multisig();
            }
            KeyCode::Esc => {
                self.ui.ms_step = MultisigStep::List;
                self.ui.clear_status();
            }
            _ => {}
        }
    }

    fn handle_ms_detail_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('p') | KeyCode::Char('P') => {
                // 查看提案
                self.fetch_proposals();
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                // 创建提案
                self.ui.ms_step = MultisigStep::SelectProposalType;
                self.ui.ms_proposal_type_selected = 0;
                self.ui.clear_status();
            }
            KeyCode::Esc => {
                self.ui.ms_step = MultisigStep::List;
                self.ui.clear_status();
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
        let types = ProposalType::all();
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
                self.ui.ms_transfer_to.clear();
                self.ui.ms_transfer_amount.clear();
                self.ui.ms_transfer_mint.clear();
                self.ui.ms_step = MultisigStep::InputTransferTo;
                self.ui.clear_status();
            }
            KeyCode::Esc => {
                self.ui.ms_step = MultisigStep::ViewDetail;
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
                }
            }
            KeyCode::Backspace => {
                self.ui.clear_status();
                match field {
                    MsInputField::TransferTo => { self.ui.ms_transfer_to.pop(); }
                    MsInputField::TransferAmount => { self.ui.ms_transfer_amount.pop(); }
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
                        self.ui.ms_step = MultisigStep::InputTransferAmount;
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

    /// 导入多签
    fn import_multisig(&mut self) {
        let address = self.ui.ms_input_address.clone();
        let rpc_url = self.get_solana_rpc_url();

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
        let tx = self.bg_tx.clone();

        self.ui.set_status("正在获取提案...");

        self.runtime.spawn(async move {
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap();

            match multisig::squads::fetch_active_proposals(&client, &rpc_url, &info).await {
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
        let proposal_type_idx = self.ui.ms_proposal_type_selected;
        let rpc_url = self.get_current_ms_rpc_url();

        // 切换到提交中
        self.ui.ms_step = MultisigStep::Submitting;
        self.ui.clear_status();

        let tx = self.bg_tx.clone();
        self.runtime.spawn(async move {
            let result = execute_create_proposal_async(
                &rpc_url,
                &private_key,
                &ms_info.address,
                proposal_type_idx,
                &to_address,
                &amount_str,
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
        let multisig_address = ms_info.address.clone();
        let tx_index = proposal.transaction_index;

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
                            0, // vault_index = 0 (default)
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

    /// 获取多签签名用的私钥
    /// 遍历钱包，找到是多签成员的 SOL 地址对应的私钥
    fn get_multisig_signer_key(&self, ms_info: &multisig::MultisigInfo) -> Option<Vec<u8>> {
        let store = self.store.as_ref()?;
        let member_addrs: Vec<String> = ms_info
            .members
            .iter()
            .map(|m| m.address())
            .collect();

        for wallet in &store.wallets {
            match &wallet.wallet_type {
                WalletType::Mnemonic {
                    encrypted_mnemonic,
                    sol_accounts,
                    ..
                } => {
                    for acc in sol_accounts {
                        if member_addrs.contains(&acc.address) {
                            let mut phrase = self.decrypt_inner_secret(encrypted_mnemonic)?;
                            let mut seed = mnemonic::mnemonic_to_seed(&phrase, "").ok()?;
                            phrase.zeroize();
                            let result = sol_keys::derive_sol_private_key(&seed, acc.derivation_index).ok();
                            seed.zeroize();
                            return result;
                        }
                    }
                }
                WalletType::PrivateKey {
                    chain_type: ChainType::Solana,
                    encrypted_private_key,
                    address,
                    ..
                } => {
                    if member_addrs.contains(address) {
                        let mut pk_str = self.decrypt_inner_secret(encrypted_private_key)?;
                        let mut bytes = bs58::decode(&pk_str).into_vec().ok()?;
                        pk_str.zeroize();
                        let result = match bytes.len() {
                            64 => Some(bytes[..32].to_vec()),
                            32 => Some(bytes.clone()),
                            _ => None,
                        };
                        bytes.zeroize();
                        return result;
                    }
                }
                _ => {}
            }
        }

        None
    }

    /// 保存多签到 store
    fn save_multisig_to_store(&mut self, info: &multisig::MultisigInfo) {
        // 检查是否已存在
        if let Some(ref store) = self.store
            && store.multisigs.iter().any(|m| m.address == info.address) {
                return; // 已存在，不重复添加
            }

        let ms_pubkey: [u8; 32] = match bs58::decode(&info.address)
            .into_vec()
            .ok()
            .and_then(|v| v.try_into().ok())
        {
            Some(pk) => pk,
            None => return,
        };

        let vault_address = multisig::derive_vault_pda(&ms_pubkey, 0)
            .map(|(pda, _)| bs58::encode(&pda).into_string())
            .unwrap_or_default();

        let rpc_url = self.get_solana_rpc_url();

        let ms_account = MultisigAccount {
            id: uuid::Uuid::new_v4().to_string(),
            name: format!("Multisig {}", &info.address[..8]),
            address: info.address.clone(),
            vault_address: vault_address.clone(),
            rpc_url,
            threshold: info.threshold,
            member_addresses: info.members.iter().map(|m| m.address()).collect(),
            hidden: false,
            created_at: chrono::Utc::now().timestamp(),
        };

        if let Some(ref mut store) = self.store {
            store.multisigs.push(ms_account);

            // 自动添加 vault 为观察钱包
            if !vault_address.is_empty() {
                let vault_exists = store.wallets.iter().any(|w| {
                    matches!(
                        &w.wallet_type,
                        WalletType::WatchOnly { address, .. } if address == &vault_address
                    )
                });
                if !vault_exists {
                    store.wallets.push(Wallet {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: format!("Vault ({})", &info.address[..8]),
                        wallet_type: WalletType::WatchOnly {
                            chain_type: ChainType::Solana,
                            address: vault_address,
                            label: None,
                            source: WatchOnlySource::SquadsVault {
                                multisig_id: info.address.clone(),
                            },
                        },
                        sort_order: store.wallets.len() as u32,
                        hidden: false,
                        created_at: chrono::Utc::now().timestamp(),
                    });
                }
            }

            let _ = self.save_store_inner();
        }
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
        self.store
            .as_ref()
            .and_then(|s| s.multisigs.get(self.ui.ms_current_index))
            .map(|m| m.rpc_url.clone())
            .unwrap_or_else(|| self.get_solana_rpc_url())
    }

    // ========== 辅助方法 ==========

    fn save_store(&self) -> Result<(), crate::error::StorageError> {
        if let (Some(store), Some(pw)) = (&self.store, &self.password) {
            encrypted::save(store, pw)?;
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
async fn execute_create_proposal_async(
    rpc_url: &str,
    private_key: &[u8],
    multisig_address: &str,
    proposal_type_idx: usize,
    to_address: &str,
    amount_str: &str,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;

    let multisig_pubkey: [u8; 32] = bs58::decode(multisig_address)
        .into_vec()
        .map_err(|e| format!("无效的多签地址: {e}"))?
        .try_into()
        .map_err(|_| "多签地址长度无效".to_string())?;

    let (vault_pda, _) = multisig::derive_vault_pda(&multisig_pubkey, 0)?;

    let to_pubkey: [u8; 32] = bs58::decode(to_address)
        .into_vec()
        .map_err(|e| format!("无效的目标地址: {e}"))?
        .try_into()
        .map_err(|_| "目标地址长度无效".to_string())?;

    let proposal_types = ProposalType::all();
    let proposal_type = proposal_types
        .get(proposal_type_idx)
        .ok_or("无效的提案类型")?;

    let inner_instructions = match proposal_type {
        ProposalType::SolTransfer => {
            let amount_raw = crate::transfer::parse_amount(amount_str, 9)?;
            let lamports: u64 = amount_raw
                .try_into()
                .map_err(|_| "SOL 数量超出范围".to_string())?;

            vec![multisig::proposals::build_sol_transfer_instruction(
                &vault_pda,
                &to_pubkey,
                lamports,
            )]
        }
        ProposalType::TokenTransfer => {
            // 暂时只支持 SOL 转账，Token 转账需要额外的 mint 地址参数
            return Err("Token 转账提案暂未实现，请使用 SOL 转账".into());
        }
    };

    multisig::squads::create_proposal_and_approve(
        &client,
        rpc_url,
        private_key,
        multisig_address,
        0, // vault_index = 0
        inner_instructions,
    )
    .await
}

/// 空存储，用于渲染时的默认值
static EMPTY_STORE: std::sync::LazyLock<WalletStore> =
    std::sync::LazyLock::new(WalletStore::new);
