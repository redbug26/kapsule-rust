#[cfg(not(target_os = "emscripten"))]
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
};
pub struct ShortcutBarItem {
    pub key: String,
    pub label: String,
}
#[cfg(not(target_os = "emscripten"))]
pub fn render_shortcut_bar(frame: &mut Frame, area: Rect, items: &[ShortcutBarItem]) {
    let mut spans = Vec::new();
    for item in items {
        spans.push(Span::styled(
            format!(" {} ", item.key),
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(210, 180, 140)),
        ));
        spans.push(Span::raw(format!(" {}  ", item.label)));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(
            Style::default()
                .fg(Color::Rgb(240, 220, 180))
                .bg(Color::Rgb(70, 50, 35)),
        ),
        area,
    );
}
