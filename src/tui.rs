use crate::storage::MessageDirection;
use chrono::{Local, TimeZone, Utc};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

pub struct ChatMessage {
    pub direction: MessageDirection,
    pub content: String,
    pub timestamp: String,
}

pub struct TransferProgress {
    pub name: String,
    pub size: u64,
    pub transferred: u64,
}

impl TransferProgress {
    fn pct(&self) -> u16 {
        if self.size == 0 {
            100
        } else {
            (self.transferred * 100 / self.size) as u16
        }
    }
}

pub struct App {
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub cursor_position: usize,
    pub status: String,
    pub should_quit: bool,
    scroll_offset: usize,
    visible_height: usize,
    pub show_menu: bool,
    pub send_progress: Option<TransferProgress>,
    pub recv_progress: Option<TransferProgress>,
}

impl App {
    pub fn new(status: &str) -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
            cursor_position: 0,
            status: status.to_string(),
            should_quit: false,
            show_menu: false,
            scroll_offset: 0,
            visible_height: 0,
            send_progress: None,
            recv_progress: None,
        }
    }

    pub fn add_message(&mut self, direction: MessageDirection, content: String, timestamp: String) {
        self.messages.push(ChatMessage {
            direction,
            content,
            timestamp,
        });
        self.scroll_to_bottom();
    }

    pub fn set_send_progress(&mut self, name: String, size: u64) {
        self.send_progress = Some(TransferProgress {
            name,
            size,
            transferred: 0,
        });
    }

    pub fn update_send_progress(&mut self, sent: u64) {
        if let Some(ref mut p) = self.send_progress {
            p.transferred = sent;
        }
    }

    pub fn clear_send_progress(&mut self) {
        self.send_progress = None;
    }

    pub fn set_recv_progress(&mut self, name: String, size: u64) {
        self.recv_progress = Some(TransferProgress {
            name,
            size,
            transferred: 0,
        });
    }

    pub fn update_recv_progress(&mut self, received: u64) {
        if let Some(ref mut p) = self.recv_progress {
            p.transferred = received;
        }
    }

    pub fn clear_recv_progress(&mut self) {
        self.recv_progress = None;
    }

    fn scroll_to_bottom(&mut self) {
        let total = self.messages.len();
        if total > self.visible_height && self.visible_height > 0 {
            self.scroll_offset = total - self.visible_height;
        } else {
            self.scroll_offset = 0;
        }
    }

    fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
    }

    fn scroll_down(&mut self, n: usize) {
        let max = self.messages.len().saturating_sub(self.visible_height);
        self.scroll_offset = (self.scroll_offset + n).min(max);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        if key.kind != KeyEventKind::Press {
            return None;
        }
        if let KeyCode::Char('m') = key.code {
            if key.modifiers.contains(KeyModifiers::ALT) {
                self.show_menu = !self.show_menu;
                return None;
            }
        }

        if self.show_menu && self.recv_progress.is_none() {
            match key.code {
                KeyCode::Esc => {
                    self.show_menu = false;
                    return None;
                }
                KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Enter => {
                    self.should_quit = true;
                    return None;
                }
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    self.show_menu = false;
                    self.input = "/send ".to_string();
                    self.cursor_position = self.input.len();
                    return None;
                }
                _ => return None,
            }
        }

        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                None
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                None
            }
            KeyCode::Enter => {
                if self.input.is_empty() {
                    return None;
                }
                let text: String = self.input.drain(..).collect();
                self.cursor_position = 0;
                Some(text)
            }
            KeyCode::Char(c) => {
                self.input.insert(self.cursor_position, c);
                self.cursor_position += c.len_utf8();
                None
            }
            KeyCode::Backspace => {
                if self.cursor_position > 0 {
                    let prev = self.input[..self.cursor_position]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.input.drain(prev..self.cursor_position);
                    self.cursor_position = prev;
                }
                None
            }
            KeyCode::Delete => {
                if self.cursor_position < self.input.len() {
                    let end = self.input[self.cursor_position..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.cursor_position + i)
                        .unwrap_or(self.input.len());
                    self.input.drain(self.cursor_position..end);
                }
                None
            }
            KeyCode::Left => {
                if self.cursor_position > 0 {
                    self.cursor_position = self.input[..self.cursor_position]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
                None
            }
            KeyCode::Right => {
                if self.cursor_position < self.input.len() {
                    self.cursor_position = self.input[self.cursor_position..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.cursor_position + i)
                        .unwrap_or(self.input.len());
                }
                None
            }
            KeyCode::Home => {
                self.cursor_position = 0;
                None
            }
            KeyCode::End => {
                self.cursor_position = self.input.len();
                None
            }
            KeyCode::PageUp => {
                let h = self.visible_height.max(1);
                self.scroll_up(h);
                None
            }
            KeyCode::PageDown => {
                let h = self.visible_height.max(1);
                self.scroll_down(h);
                None
            }
            KeyCode::Up => {
                self.scroll_up(1);
                None
            }
            KeyCode::Down => {
                self.scroll_down(1);
                None
            }
            _ => None,
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(3)])
            .split(frame.area());

        self.draw_messages(frame, chunks[0]);
        self.draw_input(frame, chunks[1]);
        if self.send_progress.is_some() {
            self.draw_transfer_modal(frame, true);
        } else if self.recv_progress.is_some() {
            self.draw_transfer_modal(frame, false);
        } else if self.show_menu {
            self.draw_menu(frame);
        }
    }

    fn draw_messages(&mut self, frame: &mut Frame, area: Rect) {
        let inner_height = area.height.saturating_sub(2) as usize;
        self.visible_height = inner_height;

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" circuitchat ")
            .title_bottom(Line::from(format!(" {} ", self.status)).right_aligned())
            .border_style(Style::default().fg(Color::DarkGray));

        let end = self.messages.len().min(self.scroll_offset + inner_height);
        let start = self.scroll_offset.min(end);

        let lines: Vec<Line> = self.messages[start..end]
            .iter()
            .map(|msg| {
                let (label, color) = match msg.direction {
                    MessageDirection::Sent => ("you", Color::Green),
                    MessageDirection::Received => ("peer", Color::Cyan),
                };
                Line::from(vec![
                    Span::styled(
                        format!("[{}] ", msg.timestamp),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("{}: ", label),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(msg.content.clone(), Style::default().fg(Color::White)),
                ])
            })
            .collect();

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);

        let label = "menu: alt+m";
        let w = (label.len() as u16).saturating_add(2);
        if area.width > w {
            let x = area.x + area.width.saturating_sub(w);
            let rect = Rect::new(x, area.y, w, 1);
            let p = Paragraph::new(Line::from(Span::styled(
                label,
                Style::default().add_modifier(Modifier::BOLD),
            )));
            frame.render_widget(p, rect);
        }
    }

    fn draw_menu(&self, frame: &mut Frame) {
        let area = frame.area();
        let mw = 48u16.min(area.width.saturating_sub(4));
        let mh = 14u16.min(area.height.saturating_sub(4));
        let mx = area.x + (area.width.saturating_sub(mw)) / 2;
        let my = area.y + (area.height.saturating_sub(mh)) / 2;
        let rect = Rect::new(mx, my, mw, mh);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Menu ")
            .border_style(Style::default().fg(Color::White));

        let lines: Vec<Line> = vec![
            Line::from(Span::styled(
                "Shortcuts:",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  Alt+M : Toggle menu"),
            Line::from("  Ctrl+C / Ctrl+D : Quit"),
            Line::from(""),
            Line::from(Span::styled(
                "Actions:",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  S : Send a file"),
            Line::from("  /cancel : Cancel incoming transfer"),
            Line::from(""),
            Line::from(Span::styled(
                "Q / Enter to quit · Esc to close",
                Style::default().fg(Color::DarkGray),
            )),
        ];

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, rect);
    }

    fn draw_transfer_modal(&self, frame: &mut Frame, is_send: bool) {
        let t = if is_send {
            self.send_progress.as_ref().unwrap()
        } else {
            self.recv_progress.as_ref().unwrap()
        };

        let area = frame.area();
        let mw = 52u16.min(area.width.saturating_sub(4));
        let mh = 8u16.min(area.height.saturating_sub(4));
        let mx = area.x + (area.width.saturating_sub(mw)) / 2;
        let my = area.y + (area.height.saturating_sub(mh)) / 2;
        let rect = Rect::new(mx, my, mw, mh);

        let pct = t.pct();
        let bar_inner = (mw as usize).saturating_sub(8);
        let filled = if t.size == 0 {
            bar_inner
        } else {
            (bar_inner as u64 * t.transferred / t.size) as usize
        };
        let empty = bar_inner.saturating_sub(filled);
        let bar = format!(
            " [{}{}] {}%",
            "\u{2588}".repeat(filled),
            "\u{2591}".repeat(empty),
            pct
        );

        let size_line = format!(
            " {} / {}",
            crate::file_transfer::format_size(t.transferred),
            crate::file_transfer::format_size(t.size)
        );

        let (title, color, hint) = if is_send {
            (" Sending File ", Color::Yellow, " Esc to cancel")
        } else {
            (" Receiving File ", Color::Cyan, " /cancel to abort")
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(color));

        let lines: Vec<Line> = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!(" {}", t.name),
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(size_line),
            Line::from(bar),
            Line::from(""),
            Line::from(Span::styled(hint, Style::default().fg(Color::DarkGray))),
        ];

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });
        frame.render_widget(paragraph, rect);
    }

    fn draw_input(&self, frame: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" message ")
            .border_style(Style::default().fg(Color::DarkGray));

        let paragraph = Paragraph::new(Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::DarkGray)),
            Span::raw(self.input.clone()),
        ]))
        .block(block);

        frame.render_widget(paragraph, area);

        let chars_before = self.input[..self.cursor_position].chars().count();
        frame.set_cursor_position((area.x + 1 + 2 + chars_before as u16, area.y + 1));
    }
}

pub fn format_timestamp(unix_secs: i64, use_local: bool, hour24: bool) -> String {
    if use_local {
        let dt = Local
            .timestamp_opt(unix_secs, 0)
            .single()
            .unwrap_or_else(|| Local::now());
        let tz = dt.format("%Z").to_string().to_lowercase();
        let fmt = if hour24 { "%H:%M:%S" } else { "%I:%M:%S %p" };
        format!("{} {}", tz, dt.format(fmt))
    } else {
        let dt = Utc
            .timestamp_opt(unix_secs, 0)
            .single()
            .unwrap_or_else(|| Utc::now());
        let fmt = if hour24 { "%H:%M:%S" } else { "%I:%M:%S %p" };
        format!("utc {}", dt.format(fmt))
    }
}

pub fn now_timestamp(use_local: bool, hour24: bool) -> String {
    if use_local {
        let dt = Local::now();
        let tz = dt.format("utc%Z").to_string().to_lowercase();
        let fmt = if hour24 { "%H:%M:%S" } else { "%I:%M:%S %p" };
        format!("{} {}", tz, dt.format(fmt))
    } else {
        let dt = Utc::now();
        let fmt = if hour24 { "%H:%M:%S" } else { "%I:%M:%S %p" };
        format!("utc {}", dt.format(fmt))
    }
}
