use ratatui::{
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::tui::state::{AddWalletOption, AddWalletStep, UiState};

pub fn render(frame: &mut Frame, state: &UiState) {
    match state.add_wallet_step {
        AddWalletStep::SelectType => render_select_type(frame, state),
        AddWalletStep::InputName
        | AddWalletStep::InputMnemonic
        | AddWalletStep::InputPrivateKey
        | AddWalletStep::InputAddress => render_text_input(frame, state),
        AddWalletStep::ShowMnemonic => render_show_mnemonic(frame, state),
        AddWalletStep::SelectChainType => render_select_chain(frame, state),
        AddWalletStep::SelectHiddenItem => render_select_hidden(frame, state),
    }
}

fn render_select_type(frame: &mut Frame, state: &UiState) {
    let area = frame.area();
    let [_, center_v, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(12),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, center, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(40),
        Constraint::Fill(1),
    ])
    .areas(center_v);

    let options = AddWalletOption::all();
    let items: Vec<ListItem> = options
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            let style = if i == state.add_wallet_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if i == state.add_wallet_selected {
                "▸ "
            } else {
                "  "
            };
            ListItem::new(Line::from(Span::styled(
                format!("{prefix}{}", opt.label()),
                style,
            )))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" 添加钱包 ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );

    let mut list_state = ListState::default();
    list_state.select(Some(state.add_wallet_selected));
    frame.render_stateful_widget(list, center, &mut list_state);
}

fn render_text_input(frame: &mut Frame, state: &UiState) {
    let area = frame.area();
    let [_, center_v, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(8),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, center, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(60),
        Constraint::Fill(1),
    ])
    .areas(center_v);

    let (title, hint) = match state.add_wallet_step {
        AddWalletStep::InputName => (" 钱包名称 ", "请输入钱包名称："),
        AddWalletStep::InputMnemonic => (" 导入助记词 ", "请输入助记词（空格分隔）："),
        AddWalletStep::InputPrivateKey => {
            let chain_hint = if state.chain_type_selected == 0 {
                " 导入私钥 (ETH) "
            } else {
                " 导入私钥 (SOL) "
            };
            (chain_hint, "请输入私钥：")
        }
        AddWalletStep::InputAddress => {
            let chain_hint = if state.chain_type_selected == 0 {
                " 导入地址 (ETH) "
            } else {
                " 导入地址 (SOL) "
            };
            (chain_hint, "请输入地址：")
        }
        _ => (" 输入 ", "请输入："),
    };

    let is_secret = matches!(
        state.add_wallet_step,
        AddWalletStep::InputPrivateKey
    );

    let display_text = if is_secret {
        "*".repeat(state.input_buffer.len())
    } else {
        state.input_buffer.clone()
    };

    let mut lines = vec![
        Line::from(""),
        Line::from(hint),
        Line::from(""),
        Line::from(Span::styled(
            format!("  > {display_text}"),
            Style::default().fg(Color::Yellow),
        )),
    ];

    if let Some(ref msg) = state.status_message {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("  {msg}"),
            Style::default().fg(Color::Red),
        )));
    }

    let block = Block::default()
        .title(title)
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, center);
}

fn render_show_mnemonic(frame: &mut Frame, state: &UiState) {
    let area = frame.area();
    let [_, center_v, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(14),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, center, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(60),
        Constraint::Fill(1),
    ])
    .areas(center_v);

    let words: Vec<&str> = state.mnemonic_buffer.split_whitespace().collect();
    let mut word_lines = Vec::new();

    // 每行显示 4 个词
    for chunk in words.chunks(4) {
        let line_str: Vec<String> = chunk
            .iter()
            .enumerate()
            .map(|(j, w)| {
                let idx = word_lines.len() * 4 + j + 1;
                format!("{idx:2}. {w}")
            })
            .collect();
        word_lines.push(line_str.join("  "));
    }

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  请妥善保管以下助记词！",
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    for wl in &word_lines {
        lines.push(Line::from(Span::styled(
            format!("  {wl}"),
            Style::default().fg(Color::Yellow),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "  按 Enter 确认已保存",
        Style::default().fg(Color::DarkGray),
    )));

    let block = Block::default()
        .title(" 助记词 ")
        .title_alignment(Alignment::Center)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, center);
}

fn render_select_chain(frame: &mut Frame, state: &UiState) {
    let area = frame.area();
    let [_, center_v, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(6),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, center, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(30),
        Constraint::Fill(1),
    ])
    .areas(center_v);

    let chain_options = ["Ethereum (ETH)", "Solana (SOL)"];
    let items: Vec<ListItem> = chain_options
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let style = if i == state.chain_type_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if i == state.chain_type_selected {
                "▸ "
            } else {
                "  "
            };
            ListItem::new(Line::from(Span::styled(format!("{prefix}{label}"), style)))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" 选择链类型 ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );

    let mut list_state = ListState::default();
    list_state.select(Some(state.chain_type_selected));
    frame.render_stateful_widget(list, center, &mut list_state);
}

fn render_select_hidden(frame: &mut Frame, _state: &UiState) {
    let area = frame.area();
    let [_, center_v, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(8),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, center, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(50),
        Constraint::Fill(1),
    ])
    .areas(center_v);

    let paragraph = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            "  暂无隐藏项目",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  按 Esc 返回",
            Style::default().fg(Color::DarkGray),
        )),
    ])
    .block(
        Block::default()
            .title(" 恢复隐藏项目 ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );

    frame.render_widget(paragraph, center);
}
