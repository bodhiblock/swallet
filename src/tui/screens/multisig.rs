use ratatui::{
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::multisig::ProposalType;
use crate::storage::data::MultisigAccount;
use crate::tui::state::{MultisigStep, UiState, VoteAction};

pub fn render(frame: &mut Frame, state: &UiState, multisigs: &[MultisigAccount]) {
    let area = frame.area();

    let [_, center_v, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(20),
        Constraint::Fill(1),
    ])
    .areas(area);

    let [_, center, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(64),
        Constraint::Fill(1),
    ])
    .areas(center_v);

    match state.ms_step {
        MultisigStep::List => render_list(frame, center, state, multisigs),
        MultisigStep::InputAddress => render_input_address(frame, center, state),
        MultisigStep::ViewDetail => render_view_detail(frame, center, state),
        MultisigStep::ViewProposals => render_view_proposals(frame, center, state),
        MultisigStep::ViewProposal => render_view_proposal(frame, center, state),
        MultisigStep::SelectProposalType => render_select_proposal_type(frame, center, state),
        MultisigStep::InputTransferTo => render_input_transfer_field(frame, center, state, "地址", &state.ms_transfer_to),
        MultisigStep::InputTransferAmount => render_input_transfer_field(frame, center, state, "数量", &state.ms_transfer_amount),
        MultisigStep::ConfirmCreate | MultisigStep::ConfirmVote => render_confirm(frame, center, state),
        MultisigStep::Submitting => render_submitting(frame, center),
        MultisigStep::Result => render_result(frame, center, state),
        MultisigStep::CreateSelectCreator => render_create_select_creator(frame, center, state),
        MultisigStep::CreateInputMembers => render_create_input_members(frame, center, state),
        MultisigStep::CreateInputThreshold => render_create_input_threshold(frame, center, state),
        MultisigStep::CreateConfirm => render_create_confirm(frame, center, state),
    }
}

fn render_list(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &UiState,
    multisigs: &[MultisigAccount],
) {
    let block = Block::default()
        .title(" 多签管理 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [list_area, footer_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let visible: Vec<_> = multisigs.iter().filter(|m| !m.hidden).collect();

    let mut items: Vec<ListItem> = visible
        .iter()
        .map(|m| {
            ListItem::new(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(&m.name, Style::default().fg(Color::White)),
                Span::styled(
                    format!("  ({})", shorten_address(&m.address)),
                    Style::default().fg(Color::DarkGray),
                ),
            ]))
        })
        .collect();

    // 添加 "创建多签" 选项
    items.push(ListItem::new(Line::from(Span::styled(
        "  + 创建 Squads 多签 (Solana)",
        Style::default().fg(Color::Green),
    ))));

    // 添加 "导入多签" 选项
    items.push(ListItem::new(Line::from(Span::styled(
        "  + 导入 Squads 多签 (Solana)",
        Style::default().fg(Color::Green),
    ))));

    // ETH Safe placeholder
    items.push(ListItem::new(Line::from(Span::styled(
        "  + 导入 Safe 多签 (EVM) - 开发中",
        Style::default().fg(Color::DarkGray),
    ))));

    let list = List::new(items).highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );
    let mut list_state = ListState::default();
    list_state.select(Some(state.ms_list_selected));
    frame.render_stateful_widget(list, list_area, &mut list_state);

    let footer = Paragraph::new(Line::from(Span::styled(
        " ↑↓选择  Enter确认  Esc返回",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(footer, footer_area);
}

fn render_input_address(frame: &mut Frame, area: ratatui::layout::Rect, state: &UiState) {
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            " 请输入 Squads 多签地址:",
            Style::default().fg(Color::White),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!(" > {}", state.ms_input_address),
            Style::default().fg(Color::Yellow),
        )),
    ];

    append_status(&mut lines, state);
    append_hint(&mut lines, " Enter确认  Esc返回");

    let block = Block::default()
        .title(" 导入多签 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_view_detail(frame: &mut Frame, area: ratatui::layout::Rect, state: &UiState) {
    let mut lines = vec![Line::from("")];

    if let Some(ref info) = state.ms_current_info {
        lines.push(Line::from(vec![
            Span::styled(" 地址: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                shorten_address(&info.address.to_string()),
                Style::default().fg(Color::Yellow),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(" 阈值: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}/{}", info.threshold, info.members.len()),
                Style::default().fg(Color::Cyan),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(" 交易数: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", info.transaction_index),
                Style::default().fg(Color::White),
            ),
        ]));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            " 成员:",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )));
        for member in &info.members {
            lines.push(Line::from(vec![
                Span::styled("   ", Style::default()),
                Span::styled(
                    shorten_address(&member.address()),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    format!(" [{}]", member.permission_label()),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
    } else {
        lines.push(Line::from(Span::styled(
            " 正在加载...",
            Style::default().fg(Color::Yellow),
        )));
    }

    append_status(&mut lines, state);
    lines.push(Line::from(""));
    append_hint(&mut lines, " P查看提案  N创建提案  Esc返回");

    let block = Block::default()
        .title(" 多签详情 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_view_proposals(frame: &mut Frame, area: ratatui::layout::Rect, state: &UiState) {
    let block = Block::default()
        .title(" 提案列表 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [list_area, footer_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    if state.ms_proposals.is_empty() {
        let msg = Paragraph::new(Line::from(Span::styled(
            "  暂无提案",
            Style::default().fg(Color::DarkGray),
        )));
        frame.render_widget(msg, list_area);
    } else {
        let items: Vec<ListItem> = state
            .ms_proposals
            .iter()
            .map(|p| {
                let status_color = match p.status {
                    crate::multisig::ProposalStatus::Active => Color::Yellow,
                    crate::multisig::ProposalStatus::Approved => Color::Green,
                    crate::multisig::ProposalStatus::Executed => Color::Cyan,
                    crate::multisig::ProposalStatus::Rejected | crate::multisig::ProposalStatus::Cancelled => Color::Red,
                    _ => Color::DarkGray,
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("  #{} ", p.transaction_index),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!("[{}]", p.status.label()),
                        Style::default().fg(status_color),
                    ),
                    Span::styled(
                        format!("  通过:{} 拒绝:{}", p.approved.len(), p.rejected.len()),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();

        let list = List::new(items).highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
        let mut list_state = ListState::default();
        list_state.select(Some(state.ms_proposal_selected));
        frame.render_stateful_widget(list, list_area, &mut list_state);
    }

    let footer = Paragraph::new(Line::from(Span::styled(
        " ↑↓选择  Enter查看  Esc返回",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(footer, footer_area);
}

fn render_view_proposal(frame: &mut Frame, area: ratatui::layout::Rect, state: &UiState) {
    let mut lines = vec![Line::from("")];

    if let Some(ref proposal) = state.ms_current_proposal {
        let status_color = match proposal.status {
            crate::multisig::ProposalStatus::Active => Color::Yellow,
            crate::multisig::ProposalStatus::Approved => Color::Green,
            _ => Color::DarkGray,
        };

        lines.push(Line::from(vec![
            Span::styled(
                format!(" 提案 #{}", proposal.transaction_index),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(vec![
            Span::styled(" 状态: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                proposal.status.label(),
                Style::default().fg(status_color),
            ),
        ]));
        lines.push(Line::from(""));

        lines.push(Line::from(Span::styled(
            format!(" 已通过 ({}):", proposal.approved.len()),
            Style::default().fg(Color::Green),
        )));
        for addr in &proposal.approved {
            lines.push(Line::from(Span::styled(
                format!("   {}", shorten_address(&addr.to_string())),
                Style::default().fg(Color::DarkGray),
            )));
        }

        if !proposal.rejected.is_empty() {
            lines.push(Line::from(Span::styled(
                format!(" 已拒绝 ({}):", proposal.rejected.len()),
                Style::default().fg(Color::Red),
            )));
            for addr in &proposal.rejected {
                lines.push(Line::from(Span::styled(
                    format!("   {}", shorten_address(&addr.to_string())),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }

        lines.push(Line::from(""));

        // 操作提示
        match proposal.status {
            crate::multisig::ProposalStatus::Active => {
                append_hint(&mut lines, " A审批  R拒绝  Esc返回");
            }
            crate::multisig::ProposalStatus::Approved => {
                append_hint(&mut lines, " E执行  Esc返回");
            }
            _ => {
                append_hint(&mut lines, " Esc返回");
            }
        }
    } else {
        lines.push(Line::from(Span::styled(
            " 无提案数据",
            Style::default().fg(Color::DarkGray),
        )));
        append_hint(&mut lines, " Esc返回");
    }

    append_status(&mut lines, state);

    let block = Block::default()
        .title(" 提案详情 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_select_proposal_type(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &UiState,
) {
    let block = Block::default()
        .title(" 创建提案 - 选择类型 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [list_area, footer_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    let types = ProposalType::all();
    let items: Vec<ListItem> = types
        .iter()
        .map(|t| {
            ListItem::new(Line::from(Span::styled(
                format!("  {}", t.label()),
                Style::default().fg(Color::White),
            )))
        })
        .collect();

    let list = List::new(items).highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );
    let mut list_state = ListState::default();
    list_state.select(Some(state.ms_proposal_type_selected));
    frame.render_stateful_widget(list, list_area, &mut list_state);

    let footer = Paragraph::new(Line::from(Span::styled(
        " ↑↓选择  Enter确认  Esc返回",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(footer, footer_area);
}

fn render_input_transfer_field(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &UiState,
    field_name: &str,
    field_value: &str,
) {
    let vault_addr = state
        .ms_current_info
        .as_ref()
        .map(|i| {
            let (vault_pda, _) = crate::multisig::derive_vault_pda(&i.address, 0);
            vault_pda.to_string()
        })
        .unwrap_or_default();

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(" 从 Vault: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                shorten_address(&vault_addr),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            format!(" 目标{field_name}:"),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            format!(" > {field_value}"),
            Style::default().fg(Color::Yellow),
        )),
    ];

    append_status(&mut lines, state);
    append_hint(&mut lines, " Enter确认  Esc返回");

    let block = Block::default()
        .title(" 创建提案 - 转账参数 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_confirm(frame: &mut Frame, area: ratatui::layout::Rect, state: &UiState) {
    let masked: String = "*".repeat(state.ms_confirm_password.len());

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            " 请确认操作:",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    match state.ms_step {
        MultisigStep::ConfirmCreate => {
            let proposal_types = ProposalType::all();
            let ptype_label = proposal_types
                .get(state.ms_proposal_type_selected)
                .map(|t| t.label().to_string())
                .unwrap_or_else(|| "未知".to_string());
            lines.push(Line::from(vec![
                Span::styled("   类型: ", Style::default().fg(Color::DarkGray)),
                Span::styled(ptype_label, Style::default().fg(Color::Cyan)),
            ]));
            lines.push(Line::from(vec![
                Span::styled("   目标: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    shorten_address(&state.ms_transfer_to),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("   数量: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    state.ms_transfer_amount.clone(),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        MultisigStep::ConfirmVote => {
            if let Some(ref action) = state.ms_vote_action {
                lines.push(Line::from(vec![
                    Span::styled("   操作: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        action.label(),
                        Style::default().fg(match action {
                            VoteAction::Approve => Color::Green,
                            VoteAction::Reject => Color::Red,
                            VoteAction::Execute => Color::Cyan,
                        }),
                    ),
                ]));
            }
            if let Some(ref p) = state.ms_current_proposal {
                lines.push(Line::from(vec![
                    Span::styled("   提案: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("#{}", p.transaction_index),
                        Style::default().fg(Color::White),
                    ),
                ]));
            }
        }
        _ => {}
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " 请输入密码确认:",
        Style::default().fg(Color::White),
    )));
    lines.push(Line::from(Span::styled(
        format!(" > {masked}"),
        Style::default().fg(Color::Yellow),
    )));

    append_status(&mut lines, state);
    append_hint(&mut lines, " Enter确认执行  Esc取消");

    let block = Block::default()
        .title(" 确认操作 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_submitting(frame: &mut Frame, area: ratatui::layout::Rect) {
    let lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "   正在提交交易，请稍候...",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
    ];

    let block = Block::default()
        .title(" 提交中 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_result(frame: &mut Frame, area: ratatui::layout::Rect, state: &UiState) {
    let (success, message) = state
        .ms_result
        .as_ref()
        .cloned()
        .unwrap_or((false, "未知状态".into()));

    let (icon, color) = if success {
        ("OK", Color::Green)
    } else {
        ("FAIL", Color::Red)
    };

    let mut lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            format!("   [{icon}] {}", if success { "操作成功" } else { "操作失败" }),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    // Wrap long messages
    if message.len() > 50 {
        lines.push(Line::from(Span::styled(
            format!("   {}", &message[..50]),
            Style::default().fg(Color::White),
        )));
        lines.push(Line::from(Span::styled(
            format!("   {}", &message[50..]),
            Style::default().fg(Color::White),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            format!("   {message}"),
            Style::default().fg(Color::White),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "   按任意键返回",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .title(" 操作结果 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(color));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_create_select_creator(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &UiState,
) {
    let block = Block::default()
        .title(" 创建多签 - 选择创建者 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [list_area, footer_area] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(inner);

    if state.ms_create_sol_addresses.is_empty() {
        let msg = Paragraph::new(Line::from(Span::styled(
            "  没有可用的 SOL 地址",
            Style::default().fg(Color::Red),
        )));
        frame.render_widget(msg, list_area);
    } else {
        let items: Vec<ListItem> = state
            .ms_create_sol_addresses
            .iter()
            .map(|(addr, label)| {
                ListItem::new(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        shorten_address(addr),
                        Style::default().fg(Color::Yellow),
                    ),
                    Span::styled(
                        format!("  {label}"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();

        let list = List::new(items).highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );
        let mut list_state = ListState::default();
        list_state.select(Some(state.ms_create_creator_selected));
        frame.render_stateful_widget(list, list_area, &mut list_state);
    }

    let footer = Paragraph::new(Line::from(Span::styled(
        " ↑↓选择  Enter确认  Esc返回",
        Style::default().fg(Color::DarkGray),
    )));
    frame.render_widget(footer, footer_area);
}

fn render_create_input_members(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &UiState,
) {
    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            " 已添加的成员:",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
    ];

    if state.ms_create_members.is_empty() {
        lines.push(Line::from(Span::styled(
            "   (尚未添加)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        for (i, addr) in state.ms_create_members.iter().enumerate() {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("   {}. ", i + 1),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    shorten_address(addr),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " 输入成员地址:",
        Style::default().fg(Color::White),
    )));
    lines.push(Line::from(Span::styled(
        format!(" > {}", state.ms_create_member_input),
        Style::default().fg(Color::Yellow),
    )));

    append_status(&mut lines, state);
    append_hint(&mut lines, " Enter添加  D完成  Esc返回");

    let block = Block::default()
        .title(" 创建多签 - 添加成员 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_create_input_threshold(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &UiState,
) {
    let member_count = state.ms_create_members.len();

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(" 成员数: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{member_count}"),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            format!(" 输入阈值 (1-{member_count}):"),
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            format!(" > {}", state.ms_create_threshold_input),
            Style::default().fg(Color::Yellow),
        )),
    ];

    append_status(&mut lines, state);
    append_hint(&mut lines, " Enter确认  Esc返回");

    let block = Block::default()
        .title(" 创建多签 - 设置阈值 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_create_confirm(
    frame: &mut Frame,
    area: ratatui::layout::Rect,
    state: &UiState,
) {
    let masked: String = "*".repeat(state.ms_confirm_password.len());

    let creator_addr = state
        .ms_create_sol_addresses
        .get(state.ms_create_creator_selected)
        .map(|(addr, _)| shorten_address(addr))
        .unwrap_or_else(|| "未知".to_string());

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            " 创建 Squads 多签:",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("   创建者: ", Style::default().fg(Color::DarkGray)),
            Span::styled(creator_addr, Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::styled("   成员数: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}", state.ms_create_members.len()),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(vec![
            Span::styled("   阈值:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}/{}", state.ms_create_threshold_input, state.ms_create_members.len()),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " 请输入密码确认:",
            Style::default().fg(Color::White),
        )),
        Line::from(Span::styled(
            format!(" > {masked}"),
            Style::default().fg(Color::Yellow),
        )),
    ];

    append_status(&mut lines, state);
    append_hint(&mut lines, " Enter确认创建  Esc取消");

    let block = Block::default()
        .title(" 确认创建多签 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn append_status(lines: &mut Vec<Line<'_>>, state: &UiState) {
    if let Some(ref msg) = state.status_message {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("   {msg}"),
            Style::default().fg(Color::Red),
        )));
    }
}

fn append_hint<'a>(lines: &mut Vec<Line<'a>>, hint: &'a str) {
    lines.push(Line::from(Span::styled(
        hint,
        Style::default().fg(Color::DarkGray),
    )));
}

fn shorten_address(addr: &str) -> String {
    if addr.len() > 16 {
        format!("{}...{}", &addr[..8], &addr[addr.len() - 6..])
    } else {
        addr.to_string()
    }
}
