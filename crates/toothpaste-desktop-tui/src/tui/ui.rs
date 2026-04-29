use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Stylize,
    style::{Color, Modifier, Style},
    symbols::border,
    text::{Line, Span, Text},
    widgets::{Block, List, ListItem, Paragraph, Wrap},
};

use super::{CurrentScreen, ToothPasteTUI, auth_label, firmware_label};

pub fn ui(frame: &mut Frame, app: &mut ToothPasteTUI) {
    let area = frame.area();

    render_outer_block(frame, area);

    // Inner area inset from the border
    let inner = Block::bordered().inner(area);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(inner);

    // Strip one more row at top for a subtle section gap
    let left_area = columns[0];
    let right_area = columns[1];

    match app.current_screen {
        CurrentScreen::Home => render_home(frame, app, left_area),
        CurrentScreen::Scanning => render_scanning(frame, app, left_area),
        CurrentScreen::DeviceList => render_device_list(frame, app, left_area),
        CurrentScreen::Connected => render_connected(frame, app, left_area),
        CurrentScreen::PairingInput => render_pairing(frame, app, left_area),
    }

    render_side_panel(frame, app, right_area);

    // Status bar above the bottom instruction line
    if !app.status.is_empty() {
        render_status_bar(frame, app, area);
    }
}

// ── Outer chrome ─────────────────────────────────────────────────────────────

fn render_outer_block(frame: &mut Frame, area: Rect) {
    let title = Line::from(vec![" ".into(), "ToothPaste".bold(), " ".into()]);
    let instructions = Line::from(vec![
        " ".into(),
        "<↑↓>".blue().bold(),
        " navigate  ".into(),
        "<Enter>".blue().bold(),
        " select  ".into(),
        "<S>".blue().bold(),
        " scan  ".into(),
        "<X>".blue().bold(),
        " KillService  ".into(),
        "<Q>".blue().bold(),
        " quit ".into(),
    ]);
    let block = Block::bordered()
        .title(title.centered())
        .title_bottom(instructions.centered())
        .border_set(border::THICK);
    frame.render_widget(block, area);
}

fn render_status_bar(frame: &mut Frame, app: &ToothPasteTUI, outer: Rect) {
    // Render a one-line status just above the bottom border
    let y = outer.bottom().saturating_sub(2);
    let area = Rect {
        x: outer.x + 2,
        y,
        width: outer.width.saturating_sub(4),
        height: 1,
    };
    let para = Paragraph::new(Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(&app.status, Style::default().fg(Color::Yellow)),
    ]));
    frame.render_widget(para, area);
}

// ── Left-panel screens ────────────────────────────────────────────────────────

fn render_home(frame: &mut Frame, app: &mut ToothPasteTUI, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let header = Paragraph::new(Text::from(vec![
        Line::from("Welcome to ToothPaste!").bold(),
        Line::from("Wireless clipboard for your devices.").dim(),
    ]));
    frame.render_widget(header, chunks[0]);

    let items = vec![
        ListItem::new("  Scan for Devices"),
        ListItem::new("  Shut down Service"),
        ListItem::new("  Quit"),
    ];
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, chunks[1], &mut app.list_state);
}

fn render_scanning(frame: &mut Frame, app: &ToothPasteTUI, area: Rect) {
    let para = Paragraph::new(Text::from(vec![
        Line::from("Scanning for ToothPaste devices…").bold(),
        Line::from("").into(),
        Line::from("This may take up to 5 seconds.").dim(),
    ]))
    .wrap(Wrap { trim: false });
    frame.render_widget(para, area);

    let _ = app; // suppress unused warning; app available if needed
}

fn render_device_list(frame: &mut Frame, app: &mut ToothPasteTUI, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "Discovered devices",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("  ({})", app.app_state.devices.len())),
    ]));
    frame.render_widget(header, chunks[0]);

    let items: Vec<ListItem> = app
        .app_state
        .devices
        .iter()
        .map(|d| {
            ListItem::new(Line::from(vec![
                Span::raw(format!("  {:<22}", d.name)),
                Span::styled(
                    format!("{:>5} dBm", d.signal_strength),
                    Style::default().fg(rssi_color(d.signal_strength)),
                ),
            ]))
        })
        .collect();

    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, chunks[1], &mut app.list_state);
}

fn render_connected(frame: &mut Frame, app: &mut ToothPasteTUI, area: Rect) {
    let dev = match &app.app_state.connected_device {
        Some(d) => d,
        None => return,
    };

    let auth = auth_label(&app.app_state);
    let fw = firmware_label(&app.app_state);

    let auth_color = if auth == "Authenticated" {
        Color::Green
    } else if auth == "Pairing required" {
        Color::Red
    } else {
        Color::Yellow
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(0)])
        .split(area);

    let info = Text::from(vec![
        Line::from(vec![
            Span::styled("Connected  ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                &dev.name,
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  Address:   "),
            Span::styled(&dev.address, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::raw("  Firmware:  "),
            Span::styled(fw, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::raw("  Auth:      "),
            Span::styled(auth, Style::default().fg(auth_color)),
        ]),
    ]);
    frame.render_widget(Paragraph::new(info).wrap(Wrap { trim: false }), chunks[0]);

    let items = vec![
        ListItem::new("  Pair Device"),
        ListItem::new("  Disconnect"),
    ];
    let list = List::new(items)
        .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, chunks[1], &mut app.list_state);
}

fn render_pairing(frame: &mut Frame, app: &ToothPasteTUI, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

    let prompt = Paragraph::new(Text::from(vec![
        Line::from("Pairing required").bold(),
        Line::from(""),
        Line::from("Enter the device's compressed public key (base64):").dim(),
    ]));
    frame.render_widget(prompt, chunks[0]);

    let input_block = Block::bordered()
        .title(" Public Key ")
        .border_style(Style::default().fg(Color::Cyan));
    let input_inner = input_block.inner(chunks[1]);
    frame.render_widget(input_block, chunks[1]);

    let input_text =
        Paragraph::new(app.pair_input.as_str()).style(Style::default().fg(Color::White));
    frame.render_widget(input_text, input_inner);

    let help = Paragraph::new(Line::from(vec![
        Span::styled(
            "<Enter>",
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" confirm  "),
        Span::styled(
            "<Esc>",
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" cancel"),
    ]));
    frame.render_widget(help, chunks[2]);
}

// ── Right side panel ──────────────────────────────────────────────────────────

fn render_side_panel(frame: &mut Frame, app: &ToothPasteTUI, area: Rect) {
    let block = Block::bordered()
        .title(Line::from(" App State ").centered())
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let state = &app.app_state;

    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled("Version  ", Style::default().fg(Color::DarkGray)),
            Span::raw(&state.app_version),
        ]),
        Line::from(""),
    ];

    // Connected device section
    match &state.connected_device {
        Some(dev) => {
            let auth = auth_label(state);
            let fw = firmware_label(state);
            let auth_color = if auth == "Authenticated" {
                Color::Green
            } else if auth == "Pairing required" {
                Color::Red
            } else {
                Color::Yellow
            };

            lines.push(Line::from(Span::styled(
                "● Connected",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
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

    // Device scan results
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

// ── Helpers ───────────────────────────────────────────────────────────────────

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
