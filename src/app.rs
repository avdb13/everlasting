use std::collections::VecDeque;
use std::{
    io::Write,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use color_eyre::Report;
use crossterm::event::Event;
use tokio::sync::mpsc::{self, Receiver, Sender, UnboundedReceiver, UnboundedSender};
use tokio::sync::{oneshot, RwLock};
use tui::{
    backend::Backend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Span, Spans, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Row, Table, TableState},
    Frame, Terminal,
};

use crate::{helpers::prettier, writer::Writer};

pub struct App {
    actions: StatefulList<String>,
    table: StatefulTable<String>,
    stdout: StatefulList<String>,
    buffer: Arc<RwLock<VecDeque<String>>>,

    tick_rate: Duration,
    tx_actions: Sender<Action>,
    // writer: Writer,
}

impl App {
    pub fn new(tx_actions: Sender<Action>, tick_rate: Duration) -> Self {
        let titles = vec!["connect".to_owned(), "announce".to_owned()];

        Self {
            actions: StatefulList::new(titles.clone()),
            table: StatefulTable::new(titles),
            stdout: StatefulList::new(Vec::new()),
            buffer: Arc::new(RwLock::new(VecDeque::new())),
            tick_rate,
            tx_actions,
        }
    }

    pub async fn run<B: Backend>(
        &mut self,
        term: &mut Terminal<B>,
        (mut rx_stdout, mut rx_err): (UnboundedReceiver<String>, oneshot::Receiver<Report>),
    ) -> Result<(), Report> {
        let buffer = self.buffer.clone();
        tokio::spawn(async move {
            while let r = rx_stdout.try_recv() {
                if let Ok(s) = r {
                    let mut buffer = buffer.write().await;
                    if buffer.len() > 100 {
                        buffer.pop_back();
                    }
                    buffer.push_front(s);
                }
            }
        });

        tokio::spawn(async move {
            while let r = rx_err.try_recv() {
                if let Ok(e) = r {
                    panic!("{}", e)
                }
            }
        });

        let mut last_tick = Instant::now();
        loop {
            let buffer = self.buffer.read().await;
            let b = buffer.as_slices();

            self.stdout.items = [b.0, b.1].concat().to_vec();
            drop(buffer);

            term.draw(|f| self.ui(f))?;

            let timeout = self
                .tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or_else(|| Duration::from_secs(0));

            if crossterm::event::poll(timeout)? {
                if let Event::Key(key) = crossterm::event::read()? {
                    use crossterm::event::KeyCode::*;

                    match self.table.state.selected() {
                        Some(0) => match key.code {
                            Char('q') => return Ok(()),
                            Left => self.actions.unselect(),
                            Down => self.actions.next(),
                            Up => self.actions.prev(),
                            Tab => {
                                self.table.next();
                            }
                            Enter => {
                                let i = self.actions.state.selected().unwrap_or(0);

                                let action = match i {
                                    0 => Action::Connect,
                                    _ => Action::Announce,
                                };

                                self.tx_actions.send(action).await?;
                            }
                            _ => {}
                        },
                        Some(_) => match key.code {
                            Char('q') => return Ok(()),
                            Down => self.stdout.next(),
                            Up => self.stdout.prev(),
                            Tab => {
                                self.table.next();
                            }
                            _ => {}
                        },
                        None => match key.code {
                            Char('q') => return Ok(()),
                            Enter => self.table.state.select(Some(0)),
                            Down => self.table.next(),
                            Up => self.table.prev(),
                            Tab => {
                                self.table.next();
                            }
                            _ => {}
                        },
                    }
                }
            }

            if last_tick.elapsed() >= self.tick_rate {
                self.on_tick()?;
                last_tick = Instant::now();
            }
        }
    }

    fn ui<B: Backend>(&mut self, f: &mut Frame<B>) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(25)].as_ref())
            .split(f.size());

        let items: Vec<ListItem> = self
            .actions
            .items
            .iter()
            .map(|a| ListItem::new(Span::styled(a, Style::default())))
            .collect();

        let style = match self.table.state.selected() {
            None => (Style::default(), Style::default()),
            Some(0) => (Style::default().fg(Color::LightGreen), Style::default()),
            Some(_) => (Style::default(), Style::default().fg(Color::LightGreen)),
        };

        let items = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::NONE)
                    .title(Span::styled("actions", style.0))
                    .title_alignment(Alignment::Center),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        f.render_stateful_widget(items, layout[0], &mut self.actions.state);

        let stdout = self
            .stdout
            .items
            .iter()
            .map(|s| Row::new(Text::from(prettier(s.to_owned()))))
            .collect::<Vec<_>>();

        let stdout = Table::new(stdout.clone())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled("stdout", style.1))
                    .title_alignment(Alignment::Center),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            );

        f.render_stateful_widget(stdout, layout[1], &mut self.table.state);
    }

    fn on_tick(&mut self) -> Result<(), Report> {
        // let x = self.stdout.items.remove(0);
        // self.stdout.items.push(x);

        Ok(())
    }
}
#[derive(Debug)]
pub enum Action {
    Connect,
    Announce,
}

pub struct StatefulList<T> {
    state: ListState,
    items: Vec<T>,
}

pub struct StatefulTable<T> {
    state: TableState,
    items: Vec<T>,
}

impl<T> StatefulList<T> {
    pub fn new(items: Vec<T>) -> Self {
        Self {
            state: ListState::default(),
            items,
        }
    }

    fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };

        self.state.select(Some(i));
    }

    fn prev(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };

        self.state.select(Some(i));
    }

    fn unselect(&mut self) {
        self.state.select(None);
    }
}
impl<T> StatefulTable<T> {
    pub fn new(items: Vec<T>) -> Self {
        Self {
            state: TableState::default(),
            items,
        }
    }

    fn next(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.items.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };

        self.state.select(Some(i));
    }

    fn prev(&mut self) {
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.items.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };

        self.state.select(Some(i));
    }

    // fn unselect(&mut self) {
    //     self.state.select(None);
    // }
}
