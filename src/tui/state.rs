use crate::multisig::{MultisigInfo, ProposalInfo};
use crate::storage::data::ChainType;
use crate::transfer::TransferableAsset;

/// 当前屏幕
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    /// 密码解锁/首次设置
    Unlock,
    /// 主界面：钱包列表+资产
    Main,
    /// 添加钱包
    AddWallet,
    /// 操作菜单（弹出）
    ActionMenu,
    /// 文本输入（通用输入表单）
    TextInput,
    /// 显示助记词
    ShowMnemonic,
    /// 转账流程
    Transfer,
    /// 多签管理
    Multisig,
    /// DEX/Swap（占位）
    Dex,
    /// Staking（Vote/Stake 账户）
    Staking,
    /// 删除确认（需密码）
    ConfirmDelete,
}

/// 解锁界面状态
#[derive(Debug, Clone, PartialEq)]
pub enum UnlockMode {
    Create,
    Enter,
    Confirm,
}

/// 添加钱包菜单选项
#[derive(Debug, Clone, PartialEq)]
pub enum AddWalletOption {
    CreateMnemonic,
    ImportMnemonic,
    ImportPrivateKey,
    ImportWatchOnly,
    CreateMultisig,
    CreateMultisigWithSeed,
    ImportMultisig,
    RestoreHiddenWallet,
    RestoreHiddenAddress,
}

impl AddWalletOption {
    pub fn all() -> Vec<Self> {
        vec![
            Self::CreateMnemonic,
            Self::ImportMnemonic,
            Self::ImportPrivateKey,
            Self::ImportWatchOnly,
            Self::CreateMultisig,
            Self::CreateMultisigWithSeed,
            Self::ImportMultisig,
            Self::RestoreHiddenWallet,
            Self::RestoreHiddenAddress,
        ]
    }

    pub fn label(&self) -> &str {
        match self {
            Self::CreateMnemonic => "创建助记词钱包",
            Self::ImportMnemonic => "导入助记词钱包",
            Self::ImportPrivateKey => "导入私钥钱包",
            Self::ImportWatchOnly => "导入观察钱包",
            Self::CreateMultisig => "创建多签钱包（Squads）",
            Self::CreateMultisigWithSeed => "创建多签钱包（Squads使用种子）",
            Self::ImportMultisig => "导入多签钱包（Squads）",
            Self::RestoreHiddenWallet => "恢复隐藏钱包",
            Self::RestoreHiddenAddress => "恢复隐藏地址",
        }
    }
}

/// 添加钱包流程步骤
#[derive(Debug, Clone, PartialEq)]
pub enum AddWalletStep {
    /// 选择操作类型
    SelectType,
    /// 输入钱包名称
    InputName,
    /// 输入助记词（导入时）
    InputMnemonic,
    /// 显示生成的助记词（创建时）
    ShowMnemonic,
    /// 选择链类型（私钥/观察钱包时）
    SelectChainType,
    /// 输入私钥
    InputPrivateKey,
    /// 输入地址（观察钱包）
    InputAddress,
    /// 选择要恢复的钱包/地址（预留）
    #[allow(dead_code)]
    SelectHiddenItem,
}

/// 操作菜单上下文
#[derive(Debug, Clone, PartialEq)]
pub enum ActionContext {
    /// 选中了一个钱包
    Wallet { wallet_index: usize },
    /// 选中了助记词钱包下的一个地址
    MnemonicAddress {
        wallet_index: usize,
        chain_type: ChainType,
        account_index: usize,
    },
    /// 选中了私钥钱包的地址
    PrivateKeyAddress {
        wallet_index: usize,
        chain_type: ChainType,
    },
    /// 选中了观察钱包的地址
    WatchAddress { wallet_index: usize },
    /// 选中了多签钱包标题行
    MultisigWallet { wallet_index: usize },
    /// 选中了多签钱包下的 vault
    #[allow(dead_code)]
    MultisigVault {
        wallet_index: usize,
        vault_pos: usize,
    },
}

/// 转账流程步骤
#[derive(Debug, Clone, PartialEq)]
pub enum TransferStep {
    /// 选择资产（链+币种）
    SelectAsset,
    /// 输入目标地址
    InputAddress,
    /// 输入数量
    InputAmount,
    /// 确认（显示摘要+输入密码）
    Confirm,
    /// 发送中
    Sending,
    /// 结果
    Result,
}

/// 链选择后的下一步
#[derive(Debug, Clone, PartialEq)]
pub enum MsChainSelectPurpose {
    Import,
    Create,
}

/// 多签操作步骤
#[derive(Debug, Clone, PartialEq)]
pub enum MultisigStep {
    /// 多签列表（显示已导入的多签）
    List,
    /// 选择 Solana 系列链（导入/创建前）
    SelectChain,
    /// 输入多签地址（导入新多签）
    InputAddress,
    /// 查看多签详情（成员、阈值等）
    ViewDetail,
    /// 查看提案列表
    ViewProposals,
    /// 查看单个提案详情
    ViewProposal,
    /// 选择提案类型
    SelectProposalType,
    /// 输入转账地址
    InputTransferTo,
    /// 输入转账数量
    InputTransferAmount,
    // ---- 程序升级提案流程 ----
    /// 输入要升级的程序地址
    InputUpgradeProgram,
    /// 输入 buffer 地址
    InputUpgradeBuffer,
    // ---- 预制程序调用流程 ----
    /// 选择预制程序
    SelectProgram,
    /// 选择预制指令
    SelectProgramInstruction,
    /// 输入预制指令参数（逐个）
    InputProgramArgs,
    // ---- Vote/Stake 管理提案流程 ----
    /// 选择 Vote/Stake 操作类型
    SelectVoteStakeOp,
    /// 输入目标 Vote/Stake 账户地址
    InputVoteStakeTarget,
    /// 输入参数（new authority / vote account / to address）
    InputVoteStakeParam,
    /// 输入金额（仅 Withdraw）
    InputVoteStakeAmount,
    /// 选择 Fee Payer（创建提案/投票前）
    SelectMsFeePayer,
    /// 确认创建提案（输入密码）
    ConfirmCreate,
    /// 确认投票（输入密码）
    ConfirmVote,
    /// 提交中
    Submitting,
    /// 操作结果
    Result,
    // ---- 创建多签流程 ----
    /// 输入种子私钥（使用种子创建时）
    CreateInputSeed,
    /// 选择创建者（本地 SOL 地址）
    CreateSelectCreator,
    /// 添加成员地址
    CreateInputMembers,
    /// 设置阈值
    CreateInputThreshold,
    /// 确认创建（输入密码）
    CreateConfirm,
}

/// 多签投票类型
#[derive(Debug, Clone, PartialEq)]
pub enum VoteAction {
    Approve,
    Reject,
    Execute,
}

impl VoteAction {
    pub fn label(&self) -> &str {
        match self {
            Self::Approve => "审批通过",
            Self::Reject => "拒绝",
            Self::Execute => "执行",
        }
    }
}

/// Staking 流程步骤（Vote/Stake 账户管理）
#[derive(Debug, Clone, PartialEq)]
pub enum StakingStep {
    // 选择网络
    SelectChain,
    // 选择 fee payer（有余额的地址）
    SelectFeePayer,
    // 创建 Vote Account
    CreateVoteInputIdentity,
    CreateVoteInputWithdrawer,
    CreateVoteConfirm,
    // 创建 Stake Account
    CreateStakeInputAmount,
    CreateStakeInputLockup,
    CreateStakeConfirm,
    // Vote Account 详情
    VoteDetail,
    VoteAuthorize,
    VoteWithdrawInput,
    // Stake Account 详情
    StakeDetail,
    StakeAuthorize,
    StakeDelegateInput,
    StakeDeactivateConfirm,
    StakeWithdrawInput,
    // 通用
    Confirm,
    Submitting,
    Result,
}

/// Staking 创建类型（用于 SelectChain 后区分流程）
#[derive(Debug, Clone, PartialEq)]
pub enum StakingCreateType {
    Vote,
    Stake,
}

/// Staking 操作类型（用于 Confirm 步骤区分）
#[derive(Debug, Clone, PartialEq)]
pub enum StakingOp {
    VoteAuthorize,
    VoteWithdraw,
    StakeAuthorize,
    StakeDelegate,
    StakeWithdraw,
}

/// 操作菜单项
#[derive(Debug, Clone, PartialEq)]
pub enum ActionItem {
    Transfer,
    AddAddress,
    EditName,
    MoveUp,
    MoveDown,
    HideWallet,
    EditAddressLabel,
    HideAddress,
    DeleteWatchWallet,
    CreateMultisig,
    CreateMultisigWithSeed,
    CreateVoteAccount,
    CreateStakeAccount,
    AddVault,
    DeleteMultisig,
}

impl ActionItem {
    pub fn label(&self) -> &str {
        match self {
            Self::Transfer => "转账",
            Self::AddAddress => "添加地址",
            Self::EditName => "修改备注",
            Self::MoveUp => "上移",
            Self::MoveDown => "下移",
            Self::HideWallet => "隐藏钱包",
            Self::EditAddressLabel => "修改备注",
            Self::HideAddress => "隐藏地址",
            Self::DeleteWatchWallet => "删除钱包",
            Self::CreateMultisig => "创建多签地址（随机）",
            Self::CreateMultisigWithSeed => "创建多签地址（指定种子）",
            Self::CreateVoteAccount => "创建 Vote 账户",
            Self::CreateStakeAccount => "创建 Stake 账户",
            Self::AddVault => "添加 Vault",
            Self::DeleteMultisig => "删除多签钱包",
        }
    }

    pub fn for_wallet() -> Vec<Self> {
        vec![Self::AddAddress, Self::EditName, Self::MoveUp, Self::MoveDown, Self::HideWallet]
    }

    pub fn for_mnemonic_address() -> Vec<Self> {
        vec![Self::Transfer, Self::EditAddressLabel, Self::HideAddress]
    }

    pub fn for_mnemonic_sol_address() -> Vec<Self> {
        vec![Self::Transfer, Self::CreateVoteAccount, Self::CreateStakeAccount, Self::EditAddressLabel, Self::HideAddress, Self::CreateMultisig, Self::CreateMultisigWithSeed]
    }

    pub fn for_private_key_address() -> Vec<Self> {
        vec![Self::Transfer, Self::EditName, Self::HideWallet]
    }

    pub fn for_private_key_sol_address() -> Vec<Self> {
        vec![Self::Transfer, Self::CreateVoteAccount, Self::CreateStakeAccount, Self::EditName, Self::HideWallet, Self::CreateMultisig, Self::CreateMultisigWithSeed]
    }

    pub fn for_watch_address() -> Vec<Self> {
        vec![Self::EditAddressLabel, Self::DeleteWatchWallet]
    }

    pub fn for_multisig_wallet() -> Vec<Self> {
        vec![Self::AddVault, Self::EditName, Self::MoveUp, Self::MoveDown, Self::HideWallet, Self::DeleteMultisig]
    }

    pub fn for_multisig_vault() -> Vec<Self> {
        vec![Self::EditAddressLabel, Self::HideAddress]
    }
}

/// UI 状态
#[derive(Debug)]
pub struct UiState {
    pub screen: Screen,
    pub prev_screen: Option<Screen>,
    pub unlock_mode: UnlockMode,
    /// 密码输入缓冲
    pub password_input: String,
    /// 首次创建时暂存第一次输入的密码
    pub password_first: Option<String>,
    /// 状态消息
    pub status_message: Option<String>,
    /// 主界面选中行索引
    pub selected_index: usize,
    /// 是否应退出
    pub should_quit: bool,

    // 添加钱包流程
    pub add_wallet_step: AddWalletStep,
    pub add_wallet_selected: usize,
    pub add_wallet_option: Option<AddWalletOption>,
    pub input_buffer: String,
    pub wallet_name_buffer: String,
    pub mnemonic_buffer: String,
    pub chain_type_selected: usize,

    // 操作菜单
    pub action_context: Option<ActionContext>,
    pub action_items: Vec<ActionItem>,
    pub action_selected: usize,

    // 文本输入回调标记
    pub input_purpose: Option<InputPurpose>,

    // 多签流程
    pub ms_step: MultisigStep,
    pub ms_list_selected: usize,
    pub ms_current_info: Option<MultisigInfo>,
    pub ms_current_index: usize, // 在 store.multisigs 中的索引（旧，兼容）
    pub ms_current_wallet_index: usize, // 在 store.wallets 中的索引（新）
    pub ms_current_vault_index: u8,       // 当前选中的 vault index
    pub ms_current_vault_address: String, // 当前选中的 vault 地址
    pub ms_current_vault_label: Option<String>, // 当前选中的 vault 备注
    pub ms_detail_selected: usize,  // 详情页菜单选中项
    pub ms_proposals: Vec<ProposalInfo>,
    pub ms_proposal_selected: usize,
    pub ms_current_proposal: Option<ProposalInfo>,
    pub ms_proposal_type_selected: usize,
    pub ms_input_address: String,
    // 链选择
    pub ms_chain_select_purpose: MsChainSelectPurpose,
    pub ms_solana_chains: Vec<(String, String, String)>, // (chain_id, chain_name, rpc_url)
    pub ms_chain_selected: usize,
    pub ms_selected_chain_id: String,
    pub ms_selected_chain_name: String,
    pub ms_selected_rpc_url: String,
    pub ms_transfer_to: String,
    pub ms_transfer_amount: String,
    pub ms_transfer_mint: String, // SPL token mint (空=SOL)
    pub ms_upgrade_program: String, // 程序升级：程序地址
    pub ms_upgrade_buffer: String,  // 程序升级：buffer 地址
    // 预制程序调用
    pub ms_preset_program_selected: usize,
    pub ms_preset_instruction_selected: usize,
    pub ms_program_args: Vec<String>,     // 已收集的参数值
    pub ms_program_arg_index: usize,      // 当前输入的参数索引
    pub ms_program_arg_input: String,     // 当前参数输入缓冲
    pub ms_confirm_password: String,
    pub ms_vote_action: Option<VoteAction>,
    pub ms_result: Option<(bool, String)>,
    // Vote/Stake 管理提案
    pub ms_vs_ops: Vec<crate::multisig::MsVoteStakeOp>,
    pub ms_vs_op_selected: usize,
    pub ms_vs_target: String,    // 目标 vote/stake 账户地址
    pub ms_vs_param: String,     // 参数（new authority / vote account / to address）
    pub ms_vs_amount: String,    // 金额（仅 withdraw）
    // Fee Payer 选择（提案/投票）
    pub ms_fee_payer_list: Vec<(String, String, u128, usize, usize)>, // (address, label, balance, wi, ai)
    pub ms_fee_payer_selected: usize,
    pub ms_fee_payer_wallet_index: usize,
    pub ms_fee_payer_account_index: usize,
    pub ms_fee_payer_next_step: MultisigStep,
    // 创建多签流程
    pub ms_create_use_seed: bool,           // 是否使用种子模式
    pub ms_create_preset_creator: Option<String>, // 从地址菜单预设的创建者地址
    pub ms_create_seed_input: String,       // 种子私钥输入（base58）
    pub ms_create_sol_addresses: Vec<(String, String)>, // (address, label)
    pub ms_create_creator_selected: usize,
    pub ms_create_members: Vec<String>,     // 已添加的成员地址
    pub ms_create_member_input: String,     // 当前输入
    pub ms_create_threshold_input: String,
    /// 创建成功后待导入的多签地址和交易签名
    pub ms_created_address: Option<(String, String)>,

    // 转账流程
    pub transfer_step: TransferStep,
    pub transfer_from_address: String,
    pub transfer_from_label: Option<String>,
    pub transfer_chain_type: ChainType,
    pub transfer_wallet_index: usize,
    pub transfer_account_index: Option<usize>,
    pub transfer_assets: Vec<TransferableAsset>,
    pub transfer_asset_selected: usize,
    pub transfer_to_address: String,
    pub transfer_amount: String,
    pub transfer_confirm_password: String,
    pub transfer_result: Option<(bool, String)>,

    // Staking (Vote/Stake 账户)
    pub stk_step: StakingStep,
    pub stk_from_address: String,
    pub stk_wallet_index: usize,
    pub stk_account_index: usize,
    pub stk_rpc_url: String,
    pub stk_solana_chains: Vec<(String, String, String, String)>, // (id, name, rpc_url, native_symbol)
    pub stk_chain_selected: usize,
    pub stk_create_type: StakingCreateType,
    pub stk_native_symbol: String,
    /// fee payer 选择后要进入的步骤
    pub stk_pending_step: Option<StakingStep>,
    // Fee Payer 选择
    pub stk_fee_payer_list: Vec<(String, String, u128, usize, usize)>, // (address, label, balance_lamports, wallet_index, account_index)
    pub stk_fee_payer_selected: usize,
    pub stk_fee_payer_wallet_index: usize,
    pub stk_fee_payer_account_index: usize,
    pub stk_identity_input: String,
    pub stk_withdrawer_input: String,
    pub stk_amount_input: String,
    pub stk_lockup_days_input: String,
    pub stk_vote_account_input: String,
    pub stk_confirm_password: String,
    pub stk_result: Option<(bool, String)>,
    pub stk_vote_info: Option<crate::staking::VoteAccountInfo>,
    pub stk_stake_info: Option<crate::staking::StakeAccountInfo>,
    pub stk_fetch_error: Option<String>,
    pub stk_detail_selected: usize,
    pub stk_target_address: String,
    pub stk_authorize_type: u32,
    pub stk_new_authority_input: String,
    /// Confirm 步骤前的操作类型（用于区分不同确认流程）
    pub stk_confirm_op: StakingOp,

    // 删除钱包确认
    pub pending_delete_wallet: Option<usize>,
    pub delete_confirm_password: String,
}

/// 文本输入的用途
#[derive(Debug, Clone, PartialEq)]
pub enum InputPurpose {
    EditLabel,
    EditVaultLabel,
    /// 助记词钱包添加地址（选链后派生）
    AddMnemonicAddress,
}

impl UiState {
    pub fn new(has_existing_data: bool) -> Self {
        Self {
            screen: Screen::Unlock,
            prev_screen: None,
            unlock_mode: if has_existing_data {
                UnlockMode::Enter
            } else {
                UnlockMode::Create
            },
            password_input: String::new(),
            password_first: None,
            status_message: None,
            selected_index: 0,
            should_quit: false,

            add_wallet_step: AddWalletStep::SelectType,
            add_wallet_selected: 0,
            add_wallet_option: None,
            input_buffer: String::new(),
            wallet_name_buffer: String::new(),
            mnemonic_buffer: String::new(),
            chain_type_selected: 0,

            action_context: None,
            action_items: Vec::new(),
            action_selected: 0,

            input_purpose: None,

            ms_step: MultisigStep::List,
            ms_list_selected: 0,
            ms_current_info: None,
            ms_current_index: 0,
            ms_current_wallet_index: 0,
            ms_current_vault_index: 0,
            ms_current_vault_address: String::new(),
            ms_current_vault_label: None,
            ms_detail_selected: 0,
            ms_proposals: Vec::new(),
            ms_proposal_selected: 0,
            ms_current_proposal: None,
            ms_proposal_type_selected: 0,
            ms_input_address: String::new(),
            ms_chain_select_purpose: MsChainSelectPurpose::Import,
            ms_solana_chains: Vec::new(),
            ms_chain_selected: 0,
            ms_selected_chain_id: String::new(),
            ms_selected_chain_name: String::new(),
            ms_selected_rpc_url: String::new(),
            ms_transfer_to: String::new(),
            ms_transfer_amount: String::new(),
            ms_transfer_mint: String::new(),
            ms_upgrade_program: String::new(),
            ms_upgrade_buffer: String::new(),
            ms_preset_program_selected: 0,
            ms_preset_instruction_selected: 0,
            ms_program_args: Vec::new(),
            ms_program_arg_index: 0,
            ms_program_arg_input: String::new(),
            ms_confirm_password: String::new(),
            ms_vote_action: None,
            ms_result: None,
            ms_vs_ops: Vec::new(),
            ms_vs_op_selected: 0,
            ms_vs_target: String::new(),
            ms_vs_param: String::new(),
            ms_vs_amount: String::new(),
            ms_fee_payer_list: Vec::new(),
            ms_fee_payer_selected: 0,
            ms_fee_payer_wallet_index: 0,
            ms_fee_payer_account_index: 0,
            ms_fee_payer_next_step: MultisigStep::ConfirmCreate,
            ms_create_use_seed: false,
            ms_create_preset_creator: None,
            ms_create_seed_input: String::new(),
            ms_create_sol_addresses: Vec::new(),
            ms_create_creator_selected: 0,
            ms_create_members: Vec::new(),
            ms_create_member_input: String::new(),
            ms_create_threshold_input: String::new(),
            ms_created_address: None,

            transfer_step: TransferStep::SelectAsset,
            transfer_from_address: String::new(),
            transfer_from_label: None,
            transfer_chain_type: ChainType::Ethereum,
            transfer_wallet_index: 0,
            transfer_account_index: None,
            transfer_assets: Vec::new(),
            transfer_asset_selected: 0,
            transfer_to_address: String::new(),
            transfer_amount: String::new(),
            transfer_confirm_password: String::new(),
            transfer_result: None,
            stk_step: StakingStep::VoteDetail,
            stk_from_address: String::new(),
            stk_wallet_index: 0,
            stk_account_index: 0,
            stk_rpc_url: String::new(),
            stk_solana_chains: Vec::new(),
            stk_chain_selected: 0,
            stk_create_type: StakingCreateType::Vote,
            stk_native_symbol: String::new(),
            stk_pending_step: None,
            stk_fee_payer_list: Vec::new(),
            stk_fee_payer_selected: 0,
            stk_fee_payer_wallet_index: 0,
            stk_fee_payer_account_index: 0,
            stk_identity_input: String::new(),
            stk_withdrawer_input: String::new(),
            stk_amount_input: String::new(),
            stk_lockup_days_input: String::new(),
            stk_vote_account_input: String::new(),
            stk_confirm_password: String::new(),
            stk_result: None,
            stk_vote_info: None,
            stk_stake_info: None,
            stk_fetch_error: None,
            stk_detail_selected: 0,
            stk_target_address: String::new(),
            stk_authorize_type: 0,
            stk_new_authority_input: String::new(),
            stk_confirm_op: StakingOp::VoteAuthorize,

            pending_delete_wallet: None,
            delete_confirm_password: String::new(),
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }

    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    /// 进入添加钱包流程
    pub fn enter_add_wallet(&mut self) {
        self.screen = Screen::AddWallet;
        self.add_wallet_step = AddWalletStep::SelectType;
        self.add_wallet_selected = 0;
        self.add_wallet_option = None;
        self.input_buffer.clear();
        self.wallet_name_buffer.clear();
        self.mnemonic_buffer.clear();
        self.chain_type_selected = 0;
        self.clear_status();
    }

    /// 进入操作菜单
    pub fn enter_action_menu(&mut self, context: ActionContext) {
        let items = match &context {
            ActionContext::Wallet { .. } => ActionItem::for_wallet(),
            ActionContext::MnemonicAddress { chain_type, .. } => {
                if *chain_type == ChainType::Solana {
                    ActionItem::for_mnemonic_sol_address()
                } else {
                    ActionItem::for_mnemonic_address()
                }
            }
            ActionContext::PrivateKeyAddress { chain_type, .. } => {
                if *chain_type == ChainType::Solana {
                    ActionItem::for_private_key_sol_address()
                } else {
                    ActionItem::for_private_key_address()
                }
            }
            ActionContext::WatchAddress { .. } => ActionItem::for_watch_address(),
            ActionContext::MultisigWallet { .. } => ActionItem::for_multisig_wallet(),
            ActionContext::MultisigVault { .. } => ActionItem::for_multisig_vault(),
        };
        self.action_context = Some(context);
        self.action_items = items;
        self.action_selected = 0;
        self.prev_screen = Some(self.screen.clone());
        self.screen = Screen::ActionMenu;
    }

    /// 进入转账流程
    pub fn enter_transfer(
        &mut self,
        from_address: String,
        from_label: Option<String>,
        chain_type: ChainType,
        wallet_index: usize,
        account_index: Option<usize>,
        assets: Vec<TransferableAsset>,
    ) {
        self.screen = Screen::Transfer;
        self.transfer_step = TransferStep::SelectAsset;
        self.transfer_from_address = from_address;
        self.transfer_from_label = from_label;
        self.transfer_chain_type = chain_type;
        self.transfer_wallet_index = wallet_index;
        self.transfer_account_index = account_index;
        self.transfer_assets = assets;
        self.transfer_asset_selected = 0;
        self.transfer_to_address.clear();
        self.transfer_amount.clear();
        self.transfer_confirm_password.clear();
        self.transfer_result = None;
        self.clear_status();
    }

    /// 返回主界面
    pub fn back_to_main(&mut self) {
        self.screen = Screen::Main;
        self.clear_status();
    }
}
