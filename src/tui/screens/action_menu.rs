use ratatui::{
    layout::{Alignment, Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::tui::state::UiState;

pub fn render(frame: &mut Frame, state: &UiState) {
    let area = frame.area();

    let height = (state.action_items.len() as u16) + 2; // items + border
    let [_, center_v, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(height),
        Constraint::Fill(1),
    ])
    .areas(area);
    let [_, center, _] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(30),
        Constraint::Fill(1),
    ])
    .areas(center_v);

    let items: Vec<ListItem> = state
        .action_items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let style = if i == state.action_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let prefix = if i == state.action_selected {
                "▸ "
            } else {
                "  "
            };
            ListItem::new(Line::from(Span::styled(
                format!("{prefix}{}", item.label()),
                style,
            )))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" 操作 ")
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow)),
    );

    let mut list_state = ListState::default();
    list_state.select(Some(state.action_selected));
    frame.render_stateful_widget(list, center, &mut list_state);
}
