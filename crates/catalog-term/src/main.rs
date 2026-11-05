//! `tui-catalog`: the same catalog as https://2lab.ai/tui, in your terminal.
//!
//! ↑/↓ pick a widget · ←/→ pick a demo · Tab gives keys to the demo (Esc
//! takes them back) · t cycles the theme · m toggles reduced motion · q quits.

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use twolab_tui::catalog::{self, ComponentDoc, Demo, DemoCx};
use twolab_tui::text;
use twolab_tui::{KeyInput, Motion, Theme};

const THEMES: [(&str, Theme); 4] = [
    ("16 colours", Theme::ansi()),
    ("true colour, dark", Theme::dark()),
    ("true colour, light", Theme::light()),
    ("no colour", Theme::mono()),
];

struct App {
    docs: Vec<&'static ComponentDoc>,
    comp: usize,
    demo_i: usize,
    demo: Box<dyn Demo>,
    started: Instant,
    theme_i: usize,
    reduced: bool,
    focused: bool,
}

impl App {
    fn new() -> App {
        let docs = catalog::docs();
        let demo = (docs[0].demos[0].make)();
        App {
            docs,
            comp: 0,
            demo_i: 0,
            demo,
            started: Instant::now(),
            theme_i: usize::from(std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())) * 3,
            reduced: false,
            focused: false,
        }
    }

    fn open(&mut self) {
        let d = self.docs[self.comp];
        self.demo_i = self.demo_i.min(d.demos.len() - 1);
        self.demo = (d.demos[self.demo_i].make)();
        self.started = Instant::now();
    }

    fn theme(&self) -> Theme {
        THEMES[self.theme_i].1.with_motion(if self.reduced {
            Motion::Reduced
        } else {
            Motion::Full
        })
    }
}

fn main() -> io::Result<()> {
    let mut terminal = ratatui::init();
    let res = run(&mut terminal);
    ratatui::restore();
    res
}

fn run(terminal: &mut DefaultTerminal) -> io::Result<()> {
    let mut app = App::new();
    loop {
        terminal.draw(|f| draw(f, &mut app))?;
        if !event::poll(Duration::from_millis(40))? {
            continue;
        }
        let Event::Key(k) = event::read()? else {
            continue;
        };
        if k.kind != KeyEventKind::Press {
            continue;
        }
        if app.focused {
            if k.code == KeyCode::Esc {
                app.focused = false;
            } else if let Some(input) = KeyInput::from_crossterm(&k) {
                app.demo.key(input);
            }
            continue;
        }
        let n = app.docs.len();
        match k.code {
            KeyCode::Char('q') => return Ok(()),
            KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => return Ok(()),
            KeyCode::Up | KeyCode::Char('k') => {
                app.comp = (app.comp + n - 1) % n;
                app.demo_i = 0;
                app.open();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.comp = (app.comp + 1) % n;
                app.demo_i = 0;
                app.open();
            }
            KeyCode::Left | KeyCode::Char('h') => {
                let m = app.docs[app.comp].demos.len();
                app.demo_i = (app.demo_i + m - 1) % m;
                app.open();
            }
            KeyCode::Right | KeyCode::Char('l') => {
                let m = app.docs[app.comp].demos.len();
                app.demo_i = (app.demo_i + 1) % m;
                app.open();
            }
            KeyCode::Tab | KeyCode::Enter => {
                app.focused = app.docs[app.comp].demos[app.demo_i].interactive;
            }
            KeyCode::Char('t') => app.theme_i = (app.theme_i + 1) % THEMES.len(),
            KeyCode::Char('m') => app.reduced = !app.reduced,
            KeyCode::Char('r') => app.open(),
            _ => {}
        }
    }
}

fn draw(f: &mut ratatui::Frame, app: &mut App) {
    let theme = app.theme();
    let [body, foot] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(f.area());
    let [side, main] =
        Layout::horizontal([Constraint::Length(26), Constraint::Fill(1)]).areas(body);

    // Sidebar: widgets by group.
    let mut lines: Vec<Line> = Vec::new();
    let mut group = None;
    for (i, d) in app.docs.iter().enumerate() {
        if group != Some(d.group) {
            if group.is_some() {
                lines.push(Line::raw(""));
            }
            group = Some(d.group);
            lines.push(Line::from(Span::styled(
                d.group.label().to_uppercase(),
                theme.dim(),
            )));
        }
        let name = text::pad(d.name, 22, text::Align::Left);
        lines.push(if i == app.comp {
            Line::from(Span::styled(format!(" {name}"), theme.selected()))
        } else {
            Line::from(Span::styled(format!(" {name}"), theme.text()))
        });
    }
    let sel_line = lines
        .iter()
        .position(|l| l.spans.first().is_some_and(|s| s.style == theme.selected()))
        .unwrap_or(0) as u16;
    let scroll = sel_line.saturating_sub(side.height.saturating_sub(4));
    f.render_widget(
        Paragraph::new(lines).scroll((scroll, 0)).block(
            Block::new()
                .borders(Borders::RIGHT)
                .border_style(theme.border()),
        ),
        side,
    );

    // Main: title, summary, demo tabs, the demo, its source.
    let d = app.docs[app.comp];
    let demo = &d.demos[app.demo_i];
    let [head, tabs, stage, src] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(main.inner(ratatui::layout::Margin::new(2, 0)));
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(d.name, theme.accent()),
                Span::styled(format!("  {}", d.ty), theme.dim()),
            ]),
            Line::from(Span::styled(d.summary, theme.secondary())),
        ])
        .wrap(Wrap { trim: true }),
        head,
    );
    let mut tab_spans = Vec::new();
    for (i, m) in d.demos.iter().enumerate() {
        let st = if i == app.demo_i {
            theme.selected()
        } else {
            theme.secondary()
        };
        tab_spans.push(Span::styled(format!(" {} ", m.title), st));
        tab_spans.push(Span::raw(" "));
    }
    f.render_widget(Line::from(tab_spans), tabs);

    let w = demo.cols.min(stage.width.saturating_sub(2));
    let h = demo.rows.min(stage.height.saturating_sub(2));
    let frame_area = Rect::new(stage.x, stage.y + 1, w + 2, h + 2);
    let title = if app.focused {
        " keys go to the demo · esc to leave "
    } else if demo.interactive {
        " tab to try it "
    } else {
        ""
    };
    let block = Block::bordered()
        .border_style(if app.focused {
            theme.accent()
        } else {
            theme.border()
        })
        .title(Span::styled(title, theme.dim()));
    let inner = block.inner(frame_area);
    f.render_widget(block, frame_area);
    let cx = DemoCx {
        theme,
        t: app.started.elapsed(),
    };
    app.demo.render(inner, f.buffer_mut(), &cx);
    f.render_widget(
        Line::from(Span::styled(
            format!(
                "{} · {} · {}",
                d.source.project, d.source.license, d.source.url
            ),
            theme.dim(),
        )),
        src,
    );

    let hint = format!(
        " ↑↓ widget · ←→ demo · tab try · t theme ({}) · m motion ({}) · r replay · q quit",
        THEMES[app.theme_i].0,
        if app.reduced { "reduced" } else { "full" }
    );
    f.render_widget(
        Line::from(Span::styled(
            text::truncate(&hint, foot.width as usize).into_owned(),
            theme.dim(),
        )),
        foot,
    );
    if theme.is_mono() {
        twolab_tui::theme::decolor(f.buffer_mut());
    }
}
