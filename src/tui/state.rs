/// 当前屏幕
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    /// 密码解锁/首次设置
    Unlock,
    /// 主界面：钱包列表+资产
    Main,
}

/// 解锁界面状态
#[derive(Debug, Clone, PartialEq)]
pub enum UnlockMode {
    /// 首次使用，需要创建密码
    CreatePassword,
    /// 已有数据，输入密码解锁
    EnterPassword,
    /// 创建密码时确认密码
    ConfirmPassword,
}

/// UI 状态
#[derive(Debug)]
pub struct UiState {
    pub screen: Screen,
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
}

impl UiState {
    pub fn new(has_existing_data: bool) -> Self {
        Self {
            screen: Screen::Unlock,
            unlock_mode: if has_existing_data {
                UnlockMode::EnterPassword
            } else {
                UnlockMode::CreatePassword
            },
            password_input: String::new(),
            password_first: None,
            status_message: None,
            selected_index: 0,
            should_quit: false,
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some(msg.into());
    }

    pub fn clear_status(&mut self) {
        self.status_message = None;
    }
}
