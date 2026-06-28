use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Paragraph, Wrap},
};

use super::{ToothPasteTUI, auth_label, capture_label, firmware_label};
use super::screens::{Screen, ScreenVariant};

pub fn ui(frame: &mut Frame, app: &mut ToothPasteTUI) {
    let area = frame.area();

    ScreenVariant::render_outer_block(frame, area, app.nav_hints());
    let inner = Block::bordered().inner(area);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(inner);

    let left_area = columns[0];
    let right_area = columns[1];

    app.current_screen().render(frame, left_area);
    render_side_panel(frame, app, right_area);

    let status = app.status();
    if !status.is_empty() {
        render_status_bar(frame, status, area);
    }
}

fn render_status_bar(frame: &mut Frame, status: &str, outer: Rect) {
    let y = outer.bottom().saturating_sub(2);
    let area = Rect {
        x: outer.x + 2,
        y,
        width: outer.width.saturating_sub(4),
        height: 1,
    };
    let para = Paragraph::new(Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(status, Style::default().fg(Color::Yellow)),
    ]));
    frame.render_widget(para, area);
}

fn render_side_panel(frame: &mut Frame, app: &ToothPasteTUI, area: Rect) {
    let block = Block::bordered()
        .title(Line::from(" App State ").centered())
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let state = &app.app_state;

    let (svc_label, svc_color) = if app.service_available {
        ("● Running", Color::Green)
    } else {
        ("○ Not running", Color::Red)
    };

    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled("Service:  ", Style::default().fg(Color::DarkGray)),
            Span::styled(svc_label, Style::default().fg(svc_color)),
        ]),
        Line::from(vec![
            Span::styled("Version   ", Style::default().fg(Color::DarkGray)),
            Span::raw(&state.app_version),
        ]),
        Line::from(vec![
            Span::styled("Key Capture:       ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                capture_label(state.enable_key_capture),
                Style::default().fg(if state.enable_key_capture { Color::Green } else { Color::Red }),
            ),
        ]),
        Line::from(vec![
            Span::styled("Clipboard Capture: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                capture_label(state.enable_clipboard_capture),
                Style::default().fg(if state.enable_clipboard_capture { Color::Green } else { Color::Red }),
            ),
        ]),
        Line::from(vec![
            Span::styled("Mouse Capture:     ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                capture_label(state.enable_mouse_capture),
                Style::default().fg(if state.enable_mouse_capture { Color::Green } else { Color::Red }),
            ),
        ]),
        Line::from(""),
    ];

    match &state.connected_device {
        Some(dev) => {
            let auth = auth_label(state);
            let fw = firmware_label(state);
            let auth_color = match auth {
                "Authenticated" => Color::Green,
                "Pairing required" => Color::Red,
                _ => Color::Yellow,
            };

            lines.push(Line::from(Span::styled(
                "● Connected",
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(vec![
                Span::styled("  Name     ", Style::default().fg(Color::DarkGray)),
                Span::raw(&dev.name),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  Firmware ", Style::default().fg(Color::DarkGray)),
                Span::raw(fw),
            ]));
            lines.push(Line::from(vec![
                Span::styled("  Auth     ", Style::default().fg(Color::DarkGray)),
                Span::styled(auth, Style::default().fg(auth_color)),
            ]));
        }
        None => {
            lines.push(Line::from(Span::styled(
                "○ No device connected",
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    if !state.devices.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("Nearby ({}):", state.devices.len()),
            Style::default().add_modifier(Modifier::BOLD),
        )));
        for dev in &state.devices {
            lines.push(Line::from(vec![
                Span::raw(format!("  {:<16}", truncate(&dev.name, 16))),
                Span::styled(
                    format!("{:>5} dBm", dev.signal_strength),
                    Style::default().fg(rssi_color(dev.signal_strength)),
                ),
            ]));
        }
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        inner,
    );
}

fn rssi_color(rssi: i32) -> Color {
    match rssi {
        i32::MIN..=-80 => Color::Red,
        -79..=-65 => Color::Yellow,
        _ => Color::Green,
    }
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max { s } else { &s[..max] }
}
