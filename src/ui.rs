use crate::{
    config::{self, Config},
    runner, scan,
};
use anyhow::{bail, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use std::{
    io::{self, IsTerminal},
    path::Path,
    sync::mpsc,
    time::Duration,
};
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}
fn terminal() -> Result<(Terminal<CrosstermBackend<io::Stdout>>, Guard)> {
    if !io::stdin().is_terminal() {
        bail!("TUI requires a terminal; use init flags or headless commands");
    }
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;
    Ok((Terminal::new(CrosstermBackend::new(io::stdout()))?, Guard))
}
pub fn onboarding(root: &Path) -> Result<()> {
    let (mut c, framework) = config::discover(root);
    let (mut term, _guard) = terminal()?;
    let mut step = 0usize;
    let mut input = String::new();
    let mut error = String::new();
    loop {
        let questions = [
            "Site URL (optional)",
            "Content mode: repository / api / mixed",
            "Content roots, comma separated",
            "Agent: codex / claude",
            "Validation command (required for runs)",
            "Delivery: review / commit / pull_request",
            "Interval in hours",
        ];
        let defaults = [
            c.site_url.clone(),
            c.content_mode.clone(),
            c.content_roots.join(","),
            c.agent.clone(),
            c.validation.join(" && "),
            c.delivery.clone(),
            c.interval_hours.to_string(),
        ];
        term.draw(|f| {
            let areas = Layout::vertical([Constraint::Length(6), Constraint::Length(6), Constraint::Min(4)]).split(f.area());
            f.render_widget(Paragraph::new(format!("RefreshAgent · first-run setup\nProject: {}\nDetected: {framework} · agent {}\nEnter accepts the default. Esc cancels.", root.display(), c.agent_executable.display())).block(Block::default().borders(Borders::ALL)), areas[0]);
            f.render_widget(Paragraph::new(format!("Step {}/{}: {}\nDefault: {}\n> {}", step+1, questions.len(), questions[step], defaults[step], input)).block(Block::default().borders(Borders::ALL)).style(Style::default().fg(Color::Cyan)), areas[1]);
            f.render_widget(Paragraph::new(format!("{error}\nRepository execution is supported in v0.1. API/mixed setups are recorded but cannot run yet.\nReview keeps changes on an isolated branch. Commit creates a local branch commit. Pull request pushes that branch and invokes gh.\nScheduling is opt-in with service install after setup.")).wrap(Wrap { trim: false }), areas[2]);
        })?;
        if let Event::Key(k) = event::read()? {
            if k.kind != KeyEventKind::Press {
                continue;
            }
            match k.code {
                KeyCode::Esc => return Ok(()),
                KeyCode::Char(ch) => input.push(ch),
                KeyCode::Backspace => {
                    input.pop();
                }
                KeyCode::Enter => {
                    let value = if input.is_empty() {
                        defaults[step].clone()
                    } else {
                        input.trim().into()
                    };
                    let valid = match step {
                        1 => ["repository", "api", "mixed"].contains(&value.as_str()),
                        3 => ["codex", "claude"].contains(&value.as_str()),
                        5 => ["review", "commit", "pull_request"].contains(&value.as_str()),
                        6 => value.parse::<u64>().is_ok_and(|n| n > 0),
                        _ => true,
                    };
                    if !valid {
                        error = "Invalid choice; use one of the listed values.".into();
                        continue;
                    }
                    match step {
                        0 => c.site_url = value,
                        1 => c.content_mode = value,
                        2 => {
                            c.content_roots =
                                value.split(',').map(|s| s.trim().to_string()).collect()
                        }
                        3 => {
                            c.agent = value;
                            c.agent_executable = config::executable(&c.agent)
                                .unwrap_or_else(|| c.agent.clone().into());
                        }
                        4 => {
                            c.validation = if value.is_empty() {
                                vec![]
                            } else {
                                vec![value]
                            }
                        }
                        5 => c.delivery = value,
                        6 => c.interval_hours = value.parse()?,
                        _ => {}
                    }
                    input.clear();
                    error.clear();
                    if step == 6 {
                        match c.save(root) {
                            Ok(()) => return Ok(()),
                            Err(e) => {
                                error = e.to_string();
                                step = 2;
                            }
                        }
                    } else {
                        step += 1;
                    }
                }
                _ => {}
            }
        }
    }
}
pub fn dashboard(root: &Path) -> Result<()> {
    if !Config::path(root).exists() {
        onboarding(root)?;
    }
    if !Config::path(root).exists() {
        return Ok(());
    }
    let (mut term, _guard) = terminal()?;
    let mut selected = 0usize;
    let mut detail = "Enter: execute selected task using configured delivery. h: history · p: pause/resume schedule · q: exit/cancel active run".to_string();
    let mut receiver: Option<mpsc::Receiver<String>> = None;
    let mut worker: Option<std::thread::JoinHandle<()>> = None;
    loop {
        let c = Config::load(root)?;
        let ops = scan::scan(root, &c)?;
        selected = selected.min(ops.len().saturating_sub(1));
        if let Some(rx) = &receiver {
            for msg in rx.try_iter().filter(|s| !s.is_empty()) {
                detail = msg;
            }
        }
        if worker.as_ref().is_some_and(|h| h.is_finished()) {
            if let Some(h) = worker.take() {
                let _ = h.join();
            }
            receiver = None;
        }
        term.draw(|f| {
            let areas = Layout::vertical([
                Constraint::Length(5),
                Constraint::Percentage(55),
                Constraint::Min(5),
            ])
            .split(f.area());
            f.render_widget(
                Paragraph::new(format!(
                    "RefreshAgent · {}\n{} · delivery {} · {}h schedule {}\n{} opportunities · {}",
                    c.site_url,
                    root.display(),
                    c.delivery,
                    c.interval_hours,
                    if c.paused {
                        "paused"
                    } else {
                        "enabled when service installed"
                    },
                    ops.len(),
                    if worker.is_some() { "RUNNING" } else { "ready" }
                ))
                .block(Block::default().borders(Borders::ALL)),
                areas[0],
            );
            let items: Vec<ListItem> = ops
                .iter()
                .map(|o| ListItem::new(format!("{:>3.0}  {}  {}", o.priority, o.path, o.issue)))
                .collect();
            let mut state = ListState::default().with_selected(Some(selected));
            f.render_stateful_widget(
                List::new(items)
                    .block(
                        Block::default()
                            .title("Opportunities · local evidence / imported performance")
                            .borders(Borders::ALL),
                    )
                    .highlight_style(Style::default().fg(Color::Cyan)),
                areas[1],
                &mut state,
            );
            f.render_widget(
                Paragraph::new(detail.clone())
                    .wrap(Wrap { trim: false })
                    .block(Block::default().title("Activity").borders(Borders::ALL)),
                areas[2],
            );
        })?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(k) = event::read()? {
                if k.kind != KeyEventKind::Press {
                    continue;
                }
                match k.code {
                    KeyCode::Char('q') | KeyCode::Esc => {
                        drop(receiver.take());
                        if let Some(h) = worker.take() {
                            let _ = h.join();
                        }
                        return Ok(());
                    }
                    KeyCode::Down => selected = (selected + 1).min(ops.len().saturating_sub(1)),
                    KeyCode::Up => selected = selected.saturating_sub(1),
                    KeyCode::Char('p') => {
                        let mut c = c;
                        c.paused = !c.paused;
                        c.save(root)?;
                    }
                    KeyCode::Char('h') => {
                        detail = runner::history(root)?
                            .iter()
                            .take(8)
                            .map(|r| format!("{} {} {}", r.id, r.status, r.opportunity.path))
                            .collect::<Vec<_>>()
                            .join("\n")
                    }
                    KeyCode::Enter if worker.is_none() => {
                        if let Some(o) = ops.get(selected) {
                            let root = root.to_path_buf();
                            let id = o.id.clone();
                            let (tx, rx) = mpsc::channel();
                            receiver = Some(rx);
                            worker = Some(std::thread::spawn(move || {
                                let result = runner::run(&root, &c, Some(&id), false, Some(&tx));
                                let _ = tx.send(match result {
                                    Ok(s) => s,
                                    Err(e) => format!("{e:#}"),
                                });
                            }));
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
