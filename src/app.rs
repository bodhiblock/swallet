use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::Frame;

use crate::config::AppConfig;
use crate::storage::data::WalletStore;
use crate::storage::encrypted;
use crate::tui::event;
use crate::tui::screens::{main_screen, unlock};
use crate::tui::state::{Screen, UiState, UnlockMode};

pub struct App {
    pub config: AppConfig,
    pub store: Option<WalletStore>,
    pub ui: UiState,
}

impl App {
    pub fn new(config: AppConfig) -> Self {
        let has_data = encrypted::data_file_exists();
        Self {
            config,
            store: None,
            ui: UiState::new(has_data),
        }
    }

    /// 主事件循环
    pub fn run(&mut self, terminal: &mut crate::tui::Tui) -> anyhow::Result<()> {
        while !self.ui.should_quit {
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
                main_screen::render(frame, &self.ui, store);
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        match self.ui.screen {
            Screen::Unlock => self.handle_unlock_key(key),
            Screen::Main => self.handle_main_key(key),
        }
    }

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
                // 在确认密码阶段按 Esc 返回创建密码
                if self.ui.unlock_mode == UnlockMode::ConfirmPassword {
                    self.ui.unlock_mode = UnlockMode::CreatePassword;
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
            UnlockMode::CreatePassword => {
                if self.ui.password_input.len() < 8 {
                    self.ui.set_status("密码至少8位");
                    return;
                }
                // 保存第一次输入，进入确认阶段
                self.ui.password_first = Some(self.ui.password_input.clone());
                self.ui.password_input.clear();
                self.ui.unlock_mode = UnlockMode::ConfirmPassword;
                self.ui.clear_status();
            }
            UnlockMode::ConfirmPassword => {
                let first = self.ui.password_first.as_deref().unwrap_or("");
                if self.ui.password_input != first {
                    self.ui.set_status("两次密码不一致");
                    self.ui.password_input.clear();
                    return;
                }
                // 创建新存储并保存
                let store = WalletStore::new();
                match encrypted::save(&store, self.ui.password_input.as_bytes()) {
                    Ok(()) => {
                        self.store = Some(store);
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
            UnlockMode::EnterPassword => {
                match encrypted::load(self.ui.password_input.as_bytes()) {
                    Ok(store) => {
                        self.store = Some(store);
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

    fn handle_main_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                if self.ui.selected_index > 0 {
                    self.ui.selected_index -= 1;
                }
            }
            KeyCode::Down => {
                self.ui.selected_index += 1;
            }
            _ => {}
        }
    }
}

/// 空存储，用于渲染时的默认值
static EMPTY_STORE: std::sync::LazyLock<WalletStore> =
    std::sync::LazyLock::new(WalletStore::new);
