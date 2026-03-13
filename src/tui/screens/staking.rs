use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::tui::state::{StakingStep, UiState};

pub fn render(frame: &mut Frame, state: &UiState) {
    match state.stk_step {
        StakingStep::SelectChain => render_select_chain(frame, state),
        StakingStep::SelectFeePayer => render_select_fee_payer(frame, state),
        StakingStep::CreateVoteInputIdentity => render_create_vote_identity(frame, state),
        StakingStep::CreateVoteInputWithdrawer => render_create_vote_withdrawer(frame, state),
        StakingStep::CreateVoteConfirm => render_confirm(frame, state, "创建 Vote 账户"),
        StakingStep::CreateStakeInputAmount => render_create_stake_amount(frame, state),
        StakingStep::CreateStakeConfirm => render_confirm(frame, state, "创建 Stake 账户"),
        StakingStep::VoteDetail => render_vote_detail(frame, state),
        StakingStep::StakeDetail => render_stake_detail(frame, state),
        StakingStep::VoteAuthorize | StakingStep::StakeAuthorize => {
            render_authorize_input(frame, state)
        }
        StakingStep::StakeDelegateInput => render_delegate_input(frame, state),
        StakingStep::StakeDeactivateConfirm => render_confirm(frame, state, "取消质押"),
        StakingStep::StakeWithdrawInput => render_withdraw_input(frame, state),
        StakingStep::Confirm => render_confirm(frame, state, "确认操作"),
        StakingStep::Submitting => render_submitting(frame),
        StakingStep::Result => render_result(frame, state),
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let [_, v_center, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, h_center, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(width),
        Constraint::Fill(1),
    ])
    .areas(v_center);
    h_center
}

// ========== 选择网络 ==========

fn render_select_chain(frame: &mut Frame, state: &UiState) {
    use crate::tui::state::StakingCreateType;

    let title = match state.stk_create_type {
        StakingCreateType::Vote => "创建 Vote 账户 - 选择网络",
        StakingCreateType::Stake => "创建 Stake 账户 - 选择网络",
    };

    let height = (state.stk_solana_chains.len() as u16 + 4).min(15);
    let area = centered_rect(50, height, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(format!(" {title} "))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let items: Vec<ListItem> = state
        .stk_solana_chains
        .iter()
        .map(|(_id, name, _rpc)| ListItem::new(Span::styled(format!("  {name}"), Style::default().fg(Color::White))))
        .collect();

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Indexed(236))
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    let mut list_state = ListState::default();
    list_state.select(Some(state.stk_chain_selected));

    let [list_area, hint_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_stateful_widget(list, list_area, &mut list_state);

    frame.render_widget(
        Paragraph::new(Span::styled(
            "↑↓ 选择  Enter 确认  Esc 返回",
            Style::default().fg(Color::DarkGray),
        )),
        hint_area,
    );
}

// ========== 选择 Fee Payer ==========

fn render_select_fee_payer(frame: &mut Frame, state: &UiState) {
    use crate::tui::state::StakingCreateType;

    let type_name = match state.stk_create_type {
        StakingCreateType::Vote => "Vote",
        StakingCreateType::Stake => "Stake",
    };
    let title = format!("创建 {type_name} 账户 - 选择 Fee Payer");

    let height = (state.stk_fee_payer_list.len() as u16 + 5).min(18);
    let area = centered_rect(60, height, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(format!(" {title} "))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let items: Vec<ListItem> = state
        .stk_fee_payer_list
        .iter()
        .map(|(addr, label, _wi, _ai)| {
            let short_addr = if addr.len() > 16 {
                format!("{}...{}", &addr[..8], &addr[addr.len() - 8..])
            } else {
                addr.clone()
            };
            ListItem::new(Span::styled(
                format!("  {label}  ({short_addr})"),
                Style::default().fg(Color::White),
            ))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .bg(Color::Indexed(236))
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    let mut list_state = ListState::default();
    list_state.select(Some(state.stk_fee_payer_selected));

    let [desc_area, list_area, hint_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(Span::styled(
            "选择一个有余额的地址支付交易手续费：",
            Style::default().fg(Color::Yellow),
        )),
        desc_area,
    );

    frame.render_stateful_widget(list, list_area, &mut list_state);

    frame.render_widget(
        Paragraph::new(Span::styled(
            "↑↓ 选择  Enter 确认  Esc 返回",
            Style::default().fg(Color::DarkGray),
        )),
        hint_area,
    );
}

// ========== 创建 Vote 账户 ==========

fn render_create_vote_identity(frame: &mut Frame, state: &UiState) {
    let area = centered_rect(70, 10, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" 创建 Vote 账户 - 输入 Identity 私钥 ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [addr_area, _, input_area, _, hint_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("地址: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&state.stk_from_address, Style::default().fg(Color::White)),
        ])),
        addr_area,
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::raw(&state.stk_identity_input),
            Span::styled("_", Style::default().fg(Color::Cyan)),
        ])),
        input_area,
    );

    frame.render_widget(
        Paragraph::new(Span::styled(
            "输入 Validator Identity 的 bs58 私钥  Esc 返回",
            Style::default().fg(Color::DarkGray),
        )),
        hint_area,
    );
}

fn render_create_vote_withdrawer(frame: &mut Frame, state: &UiState) {
    let area = centered_rect(70, 10, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" 创建 Vote 账户 - 输入 Withdrawer 地址 ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [addr_area, _, input_area, _, hint_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("地址: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&state.stk_from_address, Style::default().fg(Color::White)),
        ])),
        addr_area,
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::raw(&state.stk_withdrawer_input),
            Span::styled("_", Style::default().fg(Color::Cyan)),
        ])),
        input_area,
    );

    frame.render_widget(
        Paragraph::new(Span::styled(
            "输入 Withdrawer 地址（默认当前地址）  Enter 确认  Esc 返回",
            Style::default().fg(Color::DarkGray),
        )),
        hint_area,
    );
}

// ========== 创建 Stake 账户 ==========

fn render_create_stake_amount(frame: &mut Frame, state: &UiState) {
    let area = centered_rect(60, 10, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" 创建 Stake 账户 - 输入质押数量 ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Green));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [addr_area, _, input_area, _, hint_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("地址: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&state.stk_from_address, Style::default().fg(Color::White)),
        ])),
        addr_area,
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Green)),
            Span::raw(&state.stk_amount_input),
            Span::styled(" SOL", Style::default().fg(Color::DarkGray)),
            Span::styled("_", Style::default().fg(Color::Green)),
        ])),
        input_area,
    );

    frame.render_widget(
        Paragraph::new(Span::styled(
            "输入质押数量 (SOL)  Enter 确认  Esc 返回",
            Style::default().fg(Color::DarkGray),
        )),
        hint_area,
    );
}

// ========== 确认（密码） ==========

fn render_confirm(frame: &mut Frame, state: &UiState, title: &str) {
    let area = centered_rect(50, 8, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(format!(" {title} - 确认 "))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [info_area, _, pw_area, _, hint_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("地址: ", Style::default().fg(Color::DarkGray)),
            Span::styled(&state.stk_from_address, Style::default().fg(Color::White)),
        ])),
        info_area,
    );

    let masked: String = "*".repeat(state.stk_confirm_password.len());
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("密码: ", Style::default().fg(Color::DarkGray)),
            Span::raw(masked),
            Span::styled("_", Style::default().fg(Color::Yellow)),
        ])),
        pw_area,
    );

    let status = state.status_message.as_deref().unwrap_or("输入密码确认  Enter 提交  Esc 返回");
    frame.render_widget(
        Paragraph::new(Span::styled(status, Style::default().fg(Color::DarkGray))),
        hint_area,
    );
}

// ========== Vote 详情 ==========

fn render_vote_detail(frame: &mut Frame, state: &UiState) {
    let area = frame.area();

    let [header_area, main_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(3),
    ])
    .areas(area);

    // Header
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " Vote 账户详情 ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {}", state.stk_from_address),
            Style::default().fg(Color::Gray),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(header, header_area);

    // Main content
    let mut items: Vec<ListItem> = Vec::new();

    if let Some(info) = &state.stk_vote_info {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("  Identity:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(&info.validator_identity, Style::default().fg(Color::White)),
        ])));
        items.push(ListItem::new(Line::from(vec![
            Span::styled("  Voter:       ", Style::default().fg(Color::DarkGray)),
            Span::styled(&info.authorized_voter, Style::default().fg(Color::White)),
        ])));
        items.push(ListItem::new(Line::from(vec![
            Span::styled("  Withdrawer:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                &info.authorized_withdrawer,
                Style::default().fg(Color::White),
            ),
        ])));
        items.push(ListItem::new(Line::from(vec![
            Span::styled("  Commission:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}%", info.commission),
                Style::default().fg(Color::Green),
            ),
        ])));
        items.push(ListItem::new(Line::from("")));
        // 操作菜单
        items.push(ListItem::new(Line::from(Span::styled(
            "  [1] 修改 Voter 权限",
            Style::default().fg(Color::Yellow),
        ))));
        items.push(ListItem::new(Line::from(Span::styled(
            "  [2] 修改 Withdrawer 权限",
            Style::default().fg(Color::Yellow),
        ))));
    } else {
        items.push(ListItem::new(Span::styled(
            "  加载中...",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let list = List::new(items)
        .block(Block::default().borders(Borders::NONE))
        .highlight_style(
            Style::default()
                .bg(Color::Indexed(236))
                .add_modifier(Modifier::BOLD),
        );
    let mut list_state = ListState::default();
    list_state.select(Some(state.stk_detail_selected));
    frame.render_stateful_widget(list, main_area, &mut list_state);

    // Footer
    let footer = Paragraph::new(Span::styled(
        " ↑↓ 选择  Enter 操作  r 刷新  Esc 返回",
        Style::default().fg(Color::DarkGray),
    ))
    .block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(footer, footer_area);
}

// ========== Stake 详情 ==========

fn render_stake_detail(frame: &mut Frame, state: &UiState) {
    let area = frame.area();

    let [header_area, main_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Length(3),
    ])
    .areas(area);

    // Header
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            " Stake 账户详情 ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" {}", state.stk_from_address),
            Style::default().fg(Color::Gray),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(header, header_area);

    // Main content
    let mut items: Vec<ListItem> = Vec::new();

    if let Some(info) = &state.stk_stake_info {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("  状态:        ", Style::default().fg(Color::DarkGray)),
            Span::styled(&info.state, Style::default().fg(Color::White)),
        ])));
        items.push(ListItem::new(Line::from(vec![
            Span::styled("  质押数量:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!(
                    "{} SOL",
                    crate::chain::format_balance(info.stake_lamports as u128, 9)
                ),
                Style::default().fg(Color::Green),
            ),
        ])));
        if let Some(vote) = &info.delegated_vote_account {
            items.push(ListItem::new(Line::from(vec![
                Span::styled("  委托 Vote:   ", Style::default().fg(Color::DarkGray)),
                Span::styled(vote, Style::default().fg(Color::White)),
            ])));
        }
        items.push(ListItem::new(Line::from(vec![
            Span::styled("  Staker:      ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                &info.authorized_staker,
                Style::default().fg(Color::White),
            ),
        ])));
        items.push(ListItem::new(Line::from(vec![
            Span::styled("  Withdrawer:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                &info.authorized_withdrawer,
                Style::default().fg(Color::White),
            ),
        ])));
        items.push(ListItem::new(Line::from("")));
        // 操作菜单
        items.push(ListItem::new(Line::from(Span::styled(
            "  [1] 修改 Staker 权限",
            Style::default().fg(Color::Yellow),
        ))));
        items.push(ListItem::new(Line::from(Span::styled(
            "  [2] 修改 Withdrawer 权限",
            Style::default().fg(Color::Yellow),
        ))));
        items.push(ListItem::new(Line::from(Span::styled(
            "  [3] 委托 (Delegate)",
            Style::default().fg(Color::Yellow),
        ))));
        items.push(ListItem::new(Line::from(Span::styled(
            "  [4] 取消质押 (Deactivate)",
            Style::default().fg(Color::Yellow),
        ))));
        items.push(ListItem::new(Line::from(Span::styled(
            "  [5] 提取 (Withdraw)",
            Style::default().fg(Color::Yellow),
        ))));
    } else {
        items.push(ListItem::new(Span::styled(
            "  加载中...",
            Style::default().fg(Color::DarkGray),
        )));
    }

    let list = List::new(items)
        .block(Block::default().borders(Borders::NONE))
        .highlight_style(
            Style::default()
                .bg(Color::Indexed(236))
                .add_modifier(Modifier::BOLD),
        );
    let mut list_state = ListState::default();
    list_state.select(Some(state.stk_detail_selected));
    frame.render_stateful_widget(list, main_area, &mut list_state);

    // Footer
    let footer = Paragraph::new(Span::styled(
        " ↑↓ 选择  Enter 操作  r 刷新  Esc 返回",
        Style::default().fg(Color::DarkGray),
    ))
    .block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(Color::DarkGray)),
    );
    frame.render_widget(footer, footer_area);
}

// ========== 权限修改输入 ==========

fn render_authorize_input(frame: &mut Frame, state: &UiState) {
    let label = if state.stk_step == StakingStep::VoteAuthorize {
        if state.stk_authorize_type == 0 {
            "修改 Voter 权限"
        } else {
            "修改 Withdrawer 权限"
        }
    } else if state.stk_authorize_type == 0 {
        "修改 Staker 权限"
    } else {
        "修改 Withdrawer 权限"
    };

    let area = centered_rect(70, 8, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(format!(" {label} "))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [_, input_area, _, hint_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("新地址: ", Style::default().fg(Color::DarkGray)),
            Span::raw(&state.stk_new_authority_input),
            Span::styled("_", Style::default().fg(Color::Yellow)),
        ])),
        input_area,
    );

    frame.render_widget(
        Paragraph::new(Span::styled(
            "输入新的权限地址  Enter 确认  Esc 返回",
            Style::default().fg(Color::DarkGray),
        )),
        hint_area,
    );
}

// ========== Delegate 输入 ==========

fn render_delegate_input(frame: &mut Frame, state: &UiState) {
    let area = centered_rect(70, 8, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" 委托质押 - 输入 Vote Account 地址 ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [_, input_area, _, hint_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Yellow)),
            Span::raw(&state.stk_vote_account_input),
            Span::styled("_", Style::default().fg(Color::Yellow)),
        ])),
        input_area,
    );

    frame.render_widget(
        Paragraph::new(Span::styled(
            "输入要委托的 Vote Account 地址  Enter 确认  Esc 返回",
            Style::default().fg(Color::DarkGray),
        )),
        hint_area,
    );
}

// ========== Withdraw 输入 ==========

fn render_withdraw_input(frame: &mut Frame, state: &UiState) {
    let area = centered_rect(60, 10, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .title(" 提取质押 ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [addr_area, _, input_area, _, hint_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("目标地址: ", Style::default().fg(Color::DarkGray)),
            Span::raw(&state.stk_target_address),
        ])),
        addr_area,
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("数量: ", Style::default().fg(Color::DarkGray)),
            Span::raw(&state.stk_amount_input),
            Span::styled(" SOL_", Style::default().fg(Color::Yellow)),
        ])),
        input_area,
    );

    frame.render_widget(
        Paragraph::new(Span::styled(
            "输入提取数量 (SOL)  Enter 确认  Esc 返回",
            Style::default().fg(Color::DarkGray),
        )),
        hint_area,
    );
}

// ========== 提交中 ==========

fn render_submitting(frame: &mut Frame) {
    let area = centered_rect(30, 5, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    frame.render_widget(
        Paragraph::new(Span::styled(
            "  提交中...",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        inner,
    );
}

// ========== 结果 ==========

fn render_result(frame: &mut Frame, state: &UiState) {
    let area = centered_rect(60, 8, frame.area());
    frame.render_widget(Clear, area);

    let (success, msg) = state.stk_result.as_ref().map(|(s, m)| (*s, m.as_str())).unwrap_or((false, "未知"));

    let color = if success { Color::Green } else { Color::Red };
    let title = if success { " 成功 " } else { " 失败 " };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [msg_area, _, hint_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    frame.render_widget(
        Paragraph::new(Span::styled(msg, Style::default().fg(color))),
        msg_area,
    );

    frame.render_widget(
        Paragraph::new(Span::styled(
            "按任意键返回",
            Style::default().fg(Color::DarkGray),
        )),
        hint_area,
    );
}
