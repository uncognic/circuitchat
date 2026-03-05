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
use std::io::{self, Write};
use std::time::Instant;
pub struct ChatMessage {
    pub direction: MessageDirection,
    pub content: String,
    pub timestamp: String,
}

pub struct TransferProgress {
    pub name: String,
    pub size: u64,
    pub transferred: u64,
    pub start: Instant,
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
    pub pending_incoming_offer: Option<(String, u64, Option<Vec<u8>>)>,
    pub peer_typing: bool,
    pub pending_delivery: usize,
    pub session_fingerprint: Option<String>,
    pub message_notification_sound: bool,
    pub mention_notification_sound: bool,
}

impl App {
    pub fn new(
        status: &str,
        message_notification_sound: bool,
        mention_notification_sound: bool,
    ) -> Self {
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
            pending_incoming_offer: None,
            peer_typing: false,
            pending_delivery: 0,
            session_fingerprint: None,
            message_notification_sound,
            mention_notification_sound,
        }
    }

    pub fn add_message(&mut self, direction: MessageDirection, content: String, timestamp: String) {
        let should_bell = matches!(direction, MessageDirection::Received)
            && (self.message_notification_sound
                || (content.contains("@peer") && self.mention_notification_sound));

        self.messages.push(ChatMessage {
            direction,
            content,
            timestamp,
        });

        if should_bell {
            let _ = io::stdout().write_all(b"\x07");
            let _ = io::stdout().flush();
        }

        self.scroll_to_bottom();
    }

    pub fn set_send_progress(&mut self, name: String, size: u64) {
        self.send_progress = Some(TransferProgress {
            name,
            size,
            transferred: 0,
            start: Instant::now(),
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
            start: Instant::now(),
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

    pub fn scroll_to_bottom(&mut self) {
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
                KeyCode::Char('h') | KeyCode::Char('H') => {
                    self.show_menu = false;
                    self.input = "/help ".to_string();
                    self.cursor_position = self.input.len();
                    return None;
                }
                KeyCode::Char('m') | KeyCode::Char('M') => {
                    self.show_menu = false;
                    self.input = "@peer ".to_string();
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
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.input.clear();
                self.cursor_position = 0;
                return Some("/panic".to_string());
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
        let mut inner_height = area.height.saturating_sub(2) as usize;
        if let Some(ref fp) = self.session_fingerprint {
            if !fp.is_empty() {
                inner_height = inner_height.saturating_sub(1);
            }
        }
        self.visible_height = inner_height;

        let mut block = Block::default()
            .borders(Borders::ALL)
            .title(format!("circuitchat v{}", env!("CARGO_PKG_VERSION")))
            .border_style(Style::default().fg(Color::DarkGray));

        if let Some(ref fp) = self.session_fingerprint {
            if !fp.is_empty() {
                block = block.title_bottom(format!("fp: {}", fp));
            }
        }

        let end = self.messages.len().min(self.scroll_offset + inner_height);
        let start = self.scroll_offset.min(end);

        let lines: Vec<Line> = self.messages[start..end]
            .iter()
            .map(|msg| {
                let (label, color) = match msg.direction {
                    MessageDirection::Sent => ("you", Color::Green),
                    MessageDirection::Received => ("peer", Color::Cyan),
                    MessageDirection::System => ("system", Color::Yellow),
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
            let p = Paragraph::new(Line::from(Span::styled(label, Style::default())));
            frame.render_widget(p, rect);
        }

        let status_label = &self.status;
        let sw = (status_label.len() as u16).saturating_add(2);
        if area.width > sw {
            let sx = area.x + area.width.saturating_sub(sw);
            let sy = area.y + area.height.saturating_sub(1);
            let srect = Rect::new(sx, sy, sw, 1);
            let sp = Paragraph::new(Line::from(Span::styled(
                status_label.clone(),
                Style::default().fg(Color::DarkGray),
            )));
            frame.render_widget(sp, srect);
        }
    }

    fn draw_menu(&self, frame: &mut Frame) {
        let area = frame.area();
        let mw = 48u16.min(area.width.saturating_sub(4));
        let mh = 14u16.min(area.height.saturating_sub(4));
        let mx = area.x + (area.width.saturating_sub(mw)) / 2;
        let my = area.y + (area.height.saturating_sub(mh)) / 2;
        let rect = Rect::new(mx, my, mw, mh);

        let mut fill_lines: Vec<Line> = Vec::new();
        for _ in 0..mh {
            fill_lines.push(Line::from(" ".repeat(mw as usize)));
        }
        let filler = Paragraph::new(fill_lines).style(Style::default().bg(Color::Black));
        frame.render_widget(filler, rect);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" menu ")
            .border_style(Style::default().fg(Color::White));

        let lines: Vec<Line> = vec![
            Line::from(Span::styled(
                "shortcuts:",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  alt+m : toggle menu"),
            Line::from("  ctrl+c / ctrl+d : quit"),
            Line::from("  ctrl+w : panic (wipe & exit)"),
            Line::from(""),
            Line::from(Span::styled(
                "actions:",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("  /help (h) : show available commands"),
            Line::from("  @peer (m) : sound the bell in peer's terminal"),
            Line::from(""),
            Line::from(Span::styled(
                "q / enter to quit - esc to close",
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
        let mh = 10u16.min(area.height.saturating_sub(4));
        let mx = area.x + (area.width.saturating_sub(mw)) / 2;
        let my = area.y + (area.height.saturating_sub(mh)) / 2;
        let rect = Rect::new(mx, my, mw, mh);
        let mut fill_lines: Vec<Line> = Vec::new();
        for _ in 0..mh {
            fill_lines.push(Line::from(" ".repeat(mw as usize)));
        }
        let filler = Paragraph::new(fill_lines).style(Style::default().bg(Color::Black));
        frame.render_widget(filler, rect);
        let pct = t.pct();
        let pct_text = format!("{}%", pct);
        let reserved = 12 + pct_text.len();
        let bar_inner = (mw as usize).saturating_sub(reserved);
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

        let elapsed = t.start.elapsed();
        let speed_bps = if elapsed.as_secs_f64() > 0.0 {
            (t.transferred as f64) / elapsed.as_secs_f64()
        } else {
            0.0
        };
        let speed_line = format!(
            " speed: {}/s",
            crate::file_transfer::format_size(speed_bps as u64)
        );

        let eta_line = {
            let remaining = t.size.saturating_sub(t.transferred) as f64;
            if speed_bps > 0.0 && remaining > 0.0 {
                let secs = (remaining / speed_bps).round() as u64;
                let h = secs / 3600;
                let m = (secs % 3600) / 60;
                let s = secs % 60;
                if h > 0 {
                    format!(" ETA: {:02}:{:02}:{:02}", h, m, s)
                } else {
                    format!(" ETA: {:02}:{:02}", m, s)
                }
            } else if t.transferred == 0 && t.size > 0 {
                " ETA: calculating...".to_string()
            } else {
                " ETA: --:--".to_string()
            }
        };

        let (title, color, hint) = if is_send {
            (" sending File ", Color::Yellow, " Esc to cancel")
        } else {
            (" receiving File ", Color::Cyan, " /cancel to abort")
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
            Line::from(speed_line),
            Line::from(eta_line),
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

        let count = self.input.chars().count();
        let max: usize = 50000;
        let count_label = format!(" {}/{}", count, max);
        let w = count_label.len() as u16 + 2;
        if area.width > w {
            let x = area.x + area.width.saturating_sub(w);
            let y = area.y + 1;
            let rect = Rect::new(x, y, w, 1);
            let style = if count > max {
                Style::default().fg(Color::Red)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            let p = Paragraph::new(Line::from(Span::styled(count_label, style)));
            frame.render_widget(p, rect);
        }
    }
    pub fn mark_last_sent_delivered(&mut self) {
        for msg in self.messages.iter_mut().rev() {
            if msg.direction == MessageDirection::Sent && !msg.content.ends_with(" ✓") {
                msg.content.push_str(" ✓");
                break;
            }
        }
    }
}

pub fn format_timestamp(
    unix_secs: i64,
    use_local: bool,
    hour24: bool,
    show_tz: bool,
    show_seconds: bool,
) -> String {
    if use_local {
        let dt = Local
            .timestamp_opt(unix_secs, 0)
            .single()
            .unwrap_or_else(|| Local::now());
        let fmt = if show_seconds {
            if hour24 { "%H:%M:%S" } else { "%I:%M:%S %p" }
        } else {
            if hour24 { "%H:%M" } else { "%I:%M %p" }
        };
        let time_part = dt.format(fmt).to_string();
        if show_tz {
            let tz = dt.format("%Z").to_string().to_lowercase();
            format!("{} {}", tz, time_part)
        } else {
            time_part
        }
    } else {
        let dt = Utc
            .timestamp_opt(unix_secs, 0)
            .single()
            .unwrap_or_else(|| Utc::now());
        let fmt = if show_seconds {
            if hour24 { "%H:%M:%S" } else { "%I:%M:%S %p" }
        } else {
            if hour24 { "%H:%M" } else { "%I:%M %p" }
        };
        let time_part = dt.format(fmt).to_string();
        if show_tz {
            format!("utc {}", time_part)
        } else {
            time_part
        }
    }
}

pub fn now_timestamp(use_local: bool, hour24: bool, show_tz: bool, show_seconds: bool) -> String {
    if use_local {
        let dt = Local::now();
        let fmt = if show_seconds {
            if hour24 { "%H:%M:%S" } else { "%I:%M:%S %p" }
        } else {
            if hour24 { "%H:%M" } else { "%I:%M %p" }
        };
        let time_part = dt.format(fmt).to_string();
        if show_tz {
            let tz = dt.format("%Z").to_string().to_lowercase();
            format!("{} {}", tz, time_part)
        } else {
            time_part
        }
    } else {
        let dt = Utc::now();
        let fmt = if show_seconds {
            if hour24 { "%H:%M:%S" } else { "%I:%M:%S %p" }
        } else {
            if hour24 { "%H:%M" } else { "%I:%M %p" }
        };
        let time_part = dt.format(fmt).to_string();
        if show_tz {
            format!("utc {}", time_part)
        } else {
            time_part
        }
    }
}
