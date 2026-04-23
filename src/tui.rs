//! Terminal UI — 6 tabs: Chat, Queue, Models, Code, HW, Log.
//! Full snake easter egg, live streaming, hardware monitoring, code viewer.
//! Status bar with live cost, VRAM, task counter. Typewriter code animation.

use crate::llm::{LlmClient, StreamEvent};
use crate::mission::TuiEvent;
use crate::model_config::ModelConfig;
use crate::model_picker::{self, AvailableModel, ModelPickerState, PickerAction};
use crate::snake::SnakeGame;
use crate::space::SpaceGame;
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs, Wrap},
    Terminal,
};
use std::io;
use tokio::sync::mpsc;

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Chat,
    Queue,
    Models,
    Code,
    Hw,
    Log,
}

impl Tab {
    fn titles() -> Vec<&'static str> {
        vec![
            "Chat [1]",
            "Queue [2]",
            "Models [3]",
            "Code [4]",
            "HW [5]",
            "Log [6]",
        ]
    }
    fn index(&self) -> usize {
        match self {
            Tab::Chat => 0,
            Tab::Queue => 1,
            Tab::Models => 2,
            Tab::Code => 3,
            Tab::Hw => 4,
            Tab::Log => 5,
        }
    }
    fn next(&self) -> Self {
        match self {
            Tab::Chat => Tab::Queue,
            Tab::Queue => Tab::Models,
            Tab::Models => Tab::Code,
            Tab::Code => Tab::Hw,
            Tab::Hw => Tab::Log,
            Tab::Log => Tab::Chat,
        }
    }
}

/// Structured log entry for the Log tab.
struct LogEntry {
    level: String,
    message: String,
    timestamp: String,
}

/// Queue item for the Queue tab.
struct QueueItem {
    stage: String,
    step: String,
    model: String,
    status: String, // "running", "completed", "failed"
}

/// Thinking entry for reasoning visualization in Log tab.
struct ThinkingEntry {
    model: String,
    content: String,
    is_active: bool,
}

struct App {
    current_tab: Tab,
    input: String,
    input_cursor: usize,
    chat_messages: Vec<(String, String)>,
    // Streaming
    stream_buffer: String,
    stream_rx: Option<mpsc::Receiver<StreamEvent>>,
    is_generating: bool,
    // Code tab
    code_content: String,
    code_model: String,
    code_streaming: bool,
    code_history: Vec<String>,
    code_display_len: usize, // Typewriter: chars revealed so far
    // Code scroll
    code_scroll: u16,
    code_auto_scroll: bool,
    code_total_lines: u16,
    // Queue tab
    queue_items: Vec<QueueItem>,
    // Log tab (structured)
    log_entries: Vec<LogEntry>,
    // Log scroll
    log_scroll: u16,
    log_auto_scroll: bool,
    log_total_lines: u16,
    // Thinking buffer
    thinking_buffer: Vec<ThinkingEntry>,
    // HW
    hw_lines: Vec<String>,
    hw_cpu_pct: f32,
    hw_ram_used_gb: f64,
    hw_ram_total_gb: f64,
    hw_vram_gb: f64,
    // Easter eggs
    snake_game: Option<SnakeGame>,
    space_game: Option<SpaceGame>,
    // Model picker
    picker_state: Option<ModelPickerState>,
    model_config: ModelConfig,
    // Chat scrolling
    chat_scroll: u16,
    chat_auto_scroll: bool,
    chat_total_lines: u16,
    // CTO agent (persists across messages)
    cto_agent: Option<crate::cto::CtoAgent>,
    // Mission event channel
    mission_event_rx: mpsc::UnboundedReceiver<TuiEvent>,
    mission_event_tx: mpsc::UnboundedSender<TuiEvent>,
    // Status bar
    status_line: String,
    total_cost: f64,
    mission_running: bool,
    should_quit: bool,
}

impl App {
    fn new() -> Self {
        let (mission_event_tx, mission_event_rx) = mpsc::unbounded_channel();
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        Self {
            current_tab: Tab::Chat,
            input: String::new(),
            input_cursor: 0,
            chat_messages: vec![(
                "system".into(),
                format!(
                    "BattleCommand Forge v{} — Type a message or /help",
                    env!("CARGO_PKG_VERSION")
                ),
            )],
            stream_buffer: String::new(),
            stream_rx: None,
            is_generating: false,
            code_content: String::new(),
            code_model: String::new(),
            code_streaming: false,
            code_history: Vec::new(),
            code_display_len: 0,
            code_scroll: 0,
            code_auto_scroll: true,
            code_total_lines: 0,
            queue_items: Vec::new(),
            log_entries: vec![LogEntry {
                level: "info".into(),
                message: "TUI started".into(),
                timestamp: now,
            }],
            log_scroll: 0,
            log_auto_scroll: true,
            log_total_lines: 0,
            thinking_buffer: Vec::new(),
            hw_lines: vec!["Loading hardware metrics...".into()],
            hw_cpu_pct: 0.0,
            hw_ram_used_gb: 0.0,
            hw_ram_total_gb: 0.0,
            hw_vram_gb: 0.0,
            snake_game: None,
            space_game: None,
            picker_state: None,
            model_config: ModelConfig::resolve(
                crate::model_config::Preset::Premium,
                ".",
                None,
                None,
                None,
                None,
            ),
            chat_scroll: 0,
            chat_auto_scroll: true,
            chat_total_lines: 0,
            cto_agent: None,
            mission_event_rx,
            mission_event_tx,
            status_line: "READY".into(),
            total_cost: 0.0,
            mission_running: false,
            should_quit: false,
        }
    }

    fn log(&mut self, level: &str, message: impl Into<String>) {
        self.log_entries.push(LogEntry {
            level: level.into(),
            message: message.into(),
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
        });
    }
}

pub async fn run_tui() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new();

    // Initial HW poll
    let metrics = crate::hardware::collect_metrics().await;
    app.hw_lines = crate::hardware::render_for_tui(&metrics);
    app.hw_cpu_pct = metrics.cpu_usage_total;
    app.hw_ram_used_gb = metrics.mem_used_gb;
    app.hw_ram_total_gb = metrics.mem_total_gb;
    app.hw_vram_gb = metrics.ollama_vram_total_gb;

    // HW refresh counter
    let mut hw_tick = 0u32;

    loop {
        // Easter egg overlays take priority
        if let Some(ref snake) = app.snake_game {
            terminal.draw(|f| {
                snake.draw(f, f.area());
            })?;
        } else if let Some(ref space) = app.space_game {
            terminal.draw(|f| {
                space.draw(f, f.area());
            })?;
        } else {
            let picker_ref = &app.picker_state;
            terminal.draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(3), // Tab bar
                        Constraint::Min(0),    // Content
                        Constraint::Length(1), // Status bar
                        Constraint::Length(3), // Input
                    ])
                    .split(f.area());

                // Tab bar with HW summary in title
                let title = if app.hw_ram_total_gb > 0.0 {
                    format!(
                        " BattleCommand Forge | CPU:{:.0}% RAM:{:.0}/{:.0}G VRAM:{:.0}G ",
                        app.hw_cpu_pct,
                        app.hw_ram_used_gb,
                        app.hw_ram_total_gb,
                        app.hw_vram_gb.abs()
                    )
                } else {
                    " BattleCommand Forge ".to_string()
                };
                let titles: Vec<Line> = Tab::titles()
                    .iter()
                    .map(|t| Line::from(Span::raw(*t)))
                    .collect();
                let tabs = Tabs::new(titles)
                    .block(Block::default().borders(Borders::ALL).title(title))
                    .select(app.current_tab.index())
                    .style(Style::default().fg(Color::White))
                    .highlight_style(
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    );
                f.render_widget(tabs, chunks[0]);

                // Content area
                let visible_height = chunks[1].height.saturating_sub(2); // minus borders
                let content_width = chunks[1].width.saturating_sub(2) as usize;
                match app.current_tab {
                    Tab::Chat => {
                        let cto_model = app.model_config.cto.model.clone();
                        let (para, total) = render_chat(
                            &app.chat_messages,
                            &app.stream_buffer,
                            app.is_generating,
                            app.chat_scroll,
                            app.chat_auto_scroll,
                            visible_height,
                            content_width,
                            &cto_model,
                        );
                        app.chat_total_lines = total;
                        f.render_widget(para, chunks[1]);
                    }
                    Tab::Queue => f.render_widget(render_queue(&app.queue_items), chunks[1]),
                    Tab::Models => f.render_widget(render_models(), chunks[1]),
                    Tab::Code => {
                        let (para, total) = render_code(
                            &app.code_content,
                            &app.code_model,
                            app.code_streaming,
                            &app.code_history,
                            app.code_display_len,
                            app.code_scroll,
                            app.code_auto_scroll,
                            visible_height,
                            content_width,
                        );
                        app.code_total_lines = total;
                        f.render_widget(para, chunks[1]);
                    }
                    Tab::Hw => f.render_widget(render_hw(&app.hw_lines), chunks[1]),
                    Tab::Log => {
                        let (para, total) = render_log(
                            &app.log_entries,
                            &app.thinking_buffer,
                            app.log_scroll,
                            app.log_auto_scroll,
                            visible_height,
                            content_width,
                        );
                        app.log_total_lines = total;
                        f.render_widget(para, chunks[1]);
                    }
                }

                // Status bar
                let completed = app
                    .queue_items
                    .iter()
                    .filter(|i| i.status.starts_with("done"))
                    .count();
                let total_tasks = app.queue_items.len();
                f.render_widget(
                    render_status_bar(
                        &app.status_line,
                        completed,
                        total_tasks,
                        app.total_cost,
                        app.hw_vram_gb,
                    ),
                    chunks[2],
                );

                // Input bar
                let input_text = if app.current_tab == Tab::Chat {
                    if app.is_generating {
                        " Generating...".to_string()
                    } else {
                        let (before, after) =
                            app.input.split_at(app.input_cursor.min(app.input.len()));
                        format!(" > {}|{}", before, after)
                    }
                } else {
                    " 1-6=tabs | Tab=cycle | PgUp/PgDn=scroll".into()
                };
                let input_bar = Paragraph::new(input_text)
                    .style(Style::default().fg(Color::Cyan))
                    .block(Block::default().borders(Borders::ALL).title(" Input "));
                f.render_widget(input_bar, chunks[3]);

                // Picker overlay (drawn on top)
                if let Some(ref picker) = picker_ref {
                    model_picker::draw_model_picker(f, picker);
                }
            })?;
        }

        // Drain streaming tokens
        {
            let mut deferred_logs: Vec<(&str, String)> = Vec::new();
            if let Some(ref mut rx) = app.stream_rx {
                while let Ok(evt) = rx.try_recv() {
                    match evt {
                        StreamEvent::Token(t) => {
                            app.stream_buffer.push_str(&t);
                        }
                        StreamEvent::Done(full) => {
                            app.chat_messages.push(("cto".into(), full.clone()));
                            let code_blocks = extract_code_blocks(&full);
                            if !code_blocks.is_empty() {
                                app.code_content = code_blocks;
                                app.code_model = "CTO".into();
                                app.code_display_len = 0; // Reset typewriter for new code
                                app.code_history.push(app.code_content.clone());
                            }
                            app.stream_buffer.clear();
                            app.code_streaming = false;
                            app.is_generating = false;
                            app.status_line = "READY".into();
                            deferred_logs.push(("info", "Response complete".into()));
                        }
                        StreamEvent::Error(e) => {
                            app.chat_messages.push(("error".into(), e));
                            app.stream_buffer.clear();
                            app.is_generating = false;
                            app.code_streaming = false;
                            app.status_line = "READY".into();
                        }
                        StreamEvent::ToolCallStart { name, args } => {
                            let display = if args.len() > 80 {
                                format!("{:.80}...", args)
                            } else {
                                args
                            };
                            app.chat_messages
                                .push(("tool".into(), format!("[{}] {}", name, display)));
                        }
                        StreamEvent::ToolCallResult { name, result } => {
                            let display = if result.len() > 200 {
                                format!("{:.200}...", result)
                            } else {
                                result
                            };
                            app.chat_messages
                                .push(("tool_result".into(), format!("[{}] {}", name, display)));
                        }
                        StreamEvent::AgentReturn(agent) => {
                            app.cto_agent = Some(*agent);
                            deferred_logs.push(("info", "CTO agent returned".into()));
                        }
                    }
                }
            }
            for (level, msg) in deferred_logs {
                app.log(level, msg);
            }
        }

        // Drain mission events
        while let Ok(evt) = app.mission_event_rx.try_recv() {
            match evt {
                TuiEvent::Log { level, message } => {
                    app.log(&level, &message);
                }
                TuiEvent::StageStarted { stage, step, model } => {
                    app.status_line = format!("Stage: {} [{}]", stage, model);
                    if let Some(item) = app.queue_items.iter_mut().find(|i| i.stage == stage) {
                        item.status = "running".into();
                        item.model = model;
                    } else {
                        app.queue_items.push(QueueItem {
                            stage,
                            step,
                            model,
                            status: "running".into(),
                        });
                    }
                }
                TuiEvent::StageCompleted { stage, status } => {
                    if let Some(item) = app.queue_items.iter_mut().find(|i| i.stage == stage) {
                        item.status = format!("done: {}", status);
                    }
                }
                TuiEvent::CodeChunk {
                    content,
                    model,
                    done: _,
                } => {
                    // Reset typewriter if content is brand new (not appended)
                    if app.code_content.is_empty() || !content.starts_with(&app.code_content) {
                        app.code_display_len = 0;
                    }
                    app.code_content = content;
                    app.code_model = model;
                }
                TuiEvent::MissionCompleted { score, output_dir } => {
                    app.mission_running = false;
                    app.status_line = format!("MISSION COMPLETE — Score: {:.1}/10", score);
                    app.chat_messages.push((
                        "system".into(),
                        format!("Mission complete! Score: {:.1}/10 — {}", score, output_dir),
                    ));
                    app.log("info", format!("Mission complete: {:.1}/10", score));
                    if !app.code_content.is_empty() {
                        app.code_history.push(app.code_content.clone());
                    }
                }
                TuiEvent::MissionFailed { error } => {
                    app.mission_running = false;
                    app.status_line = "MISSION FAILED".into();
                    app.chat_messages
                        .push(("error".into(), format!("Mission failed: {}", error)));
                    app.log("error", format!("Mission failed: {}", error));
                }
                TuiEvent::CostUpdate { total_usd } => {
                    app.total_cost = total_usd;
                }
                TuiEvent::ThinkingChunk {
                    model,
                    content,
                    done,
                } => {
                    if done {
                        if let Some(last) = app.thinking_buffer.last_mut() {
                            last.is_active = false;
                        }
                    } else if let Some(last) = app.thinking_buffer.last_mut() {
                        if last.is_active && last.model == model {
                            last.content.push_str(&content);
                        } else {
                            app.thinking_buffer.push(ThinkingEntry {
                                model,
                                content,
                                is_active: true,
                            });
                        }
                    } else {
                        app.thinking_buffer.push(ThinkingEntry {
                            model,
                            content,
                            is_active: true,
                        });
                    }
                }
            }
        }

        // HW refresh every ~4 seconds (80 ticks * 50ms) — always poll for status bar
        hw_tick += 1;
        if hw_tick.is_multiple_of(80) {
            let metrics = crate::hardware::collect_metrics().await;
            app.hw_cpu_pct = metrics.cpu_usage_total;
            app.hw_ram_used_gb = metrics.mem_used_gb;
            app.hw_ram_total_gb = metrics.mem_total_gb;
            app.hw_vram_gb = metrics.ollama_vram_total_gb;
            if app.current_tab == Tab::Hw {
                app.hw_lines = crate::hardware::render_for_tui(&metrics);
            }
        }

        // Typewriter tick — advance 12 chars per frame (~240 chars/sec)
        if !app.code_content.is_empty() && app.code_display_len < app.code_content.len() {
            let remaining = app.code_content.len().saturating_sub(app.code_display_len);
            let advance = 12usize.min(remaining);
            let target = app.code_display_len + advance;
            // Safe char boundary
            let safe = if target >= app.code_content.len() {
                app.code_content.len()
            } else {
                let mut pos = target;
                while pos < app.code_content.len() && !app.code_content.is_char_boundary(pos) {
                    pos += 1;
                }
                pos
            };
            app.code_display_len = safe;
        }

        // Easter egg ticks
        if let Some(ref mut snake) = app.snake_game {
            snake.tick();
        }
        if let Some(ref mut space) = app.space_game {
            space.tick();
        }

        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                // Easter egg intercepts
                if app.snake_game.is_some() {
                    let exit = app.snake_game.as_mut().unwrap().handle_input(key.code);
                    if exit {
                        app.snake_game = None;
                    }
                    continue;
                }
                if app.space_game.is_some() {
                    let exit = app.space_game.as_mut().unwrap().handle_input(key.code);
                    if exit {
                        app.space_game = None;
                    }
                    continue;
                }

                // Picker intercept
                if app.picker_state.is_some() {
                    match key.code {
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.picker_state = None;
                        }
                        _ => {
                            if let Some(ref mut picker) = app.picker_state {
                                match model_picker::handle_picker_input(picker, key.code) {
                                    PickerAction::Confirm(config) => {
                                        let toml = picker.to_toml();
                                        app.picker_state = None;
                                        app.model_config = config;
                                        // Save to .battlecommand/models.toml
                                        let _ = std::fs::create_dir_all(".battlecommand");
                                        match std::fs::write(".battlecommand/models.toml", &toml) {
                                            Ok(_) => {
                                                app.chat_messages.push(("system".into(), "Model config saved to .battlecommand/models.toml".into()));
                                                app.log("info", "Model config saved");
                                            }
                                            Err(e) => {
                                                app.chat_messages.push((
                                                    "error".into(),
                                                    format!("Failed to save: {}", e),
                                                ));
                                            }
                                        }
                                        app.model_config.print_summary();
                                    }
                                    PickerAction::Cancel => {
                                        app.picker_state = None;
                                        app.chat_messages.push((
                                            "system".into(),
                                            "Model setup cancelled — keeping current config."
                                                .into(),
                                        ));
                                    }
                                    PickerAction::Continue => {}
                                }
                            }
                        }
                    }
                    continue;
                }

                if app.current_tab == Tab::Chat {
                    match key.code {
                        KeyCode::Enter if !app.input.is_empty() && !app.is_generating => {
                            let msg = app.input.clone();
                            app.input.clear();
                            app.input_cursor = 0;
                            app.chat_auto_scroll = true;
                            app.chat_scroll = 0;

                            // ── Slash commands ──
                            if msg == "/snake" {
                                app.snake_game = Some(SnakeGame::new());
                                continue;
                            } else if msg == "/space" {
                                app.space_game = Some(SpaceGame::new());
                                continue;
                            } else if msg == "/status" {
                                let ws = crate::workspace::list_workspaces().unwrap_or_default();
                                app.chat_messages.push((
                                    "system".into(),
                                    format!("Workspaces: {} | Modules: 30", ws.len()),
                                ));
                                continue;
                            } else if msg == "/models" {
                                app.current_tab = Tab::Models;
                                continue;
                            } else if msg == "/hw" {
                                app.current_tab = Tab::Hw;
                                continue;
                            } else if msg == "/settings" {
                                match crate::models::list_ollama_models().await {
                                    Ok(models) => {
                                        let available = to_available_models(&models);
                                        if available.is_empty() {
                                            app.chat_messages.push((
                                                "system".into(),
                                                "No models available. Is Ollama running?".into(),
                                            ));
                                        } else {
                                            app.picker_state = Some(ModelPickerState::new(
                                                available,
                                                &app.model_config,
                                            ));
                                        }
                                    }
                                    Err(e) => app
                                        .chat_messages
                                        .push(("error".into(), format!("Failed: {}", e))),
                                }
                                continue;
                            } else if msg == "/clear" {
                                app.chat_messages.clear();
                                app.chat_messages
                                    .push(("system".into(), "Chat cleared.".into()));
                                app.thinking_buffer.clear();
                                if let Some(ref mut agent) = app.cto_agent {
                                    agent.clear_history();
                                    agent.save_history().ok();
                                }
                                continue;
                            } else if msg == "/compress" {
                                if let Some(ref mut agent) = app.cto_agent {
                                    agent.compact_history();
                                    agent.save_history().ok();
                                    app.chat_messages.push((
                                        "system".into(),
                                        format!(
                                            "History compacted to {} messages",
                                            agent.history_len()
                                        ),
                                    ));
                                } else {
                                    app.chat_messages
                                        .push(("system".into(), "No active CTO session.".into()));
                                }
                                continue;
                            } else if msg.starts_with("/mission ") {
                                // Duplicate mission guard
                                if app.mission_running {
                                    app.chat_messages.push((
                                        "system".into(),
                                        "A mission is already running. Wait for it to complete."
                                            .into(),
                                    ));
                                    continue;
                                }
                                let prompt = msg.strip_prefix("/mission ").unwrap_or("").trim();
                                if !prompt.is_empty() {
                                    app.mission_running = true;
                                    app.status_line = "MISSION RUNNING...".into();
                                    app.chat_messages.push((
                                        "system".into(),
                                        format!("Mission launched: {}", prompt),
                                    ));
                                    app.queue_items.clear();
                                    app.code_content.clear();
                                    app.code_display_len = 0;
                                    let config = app.model_config.clone();
                                    let p = prompt.to_string();
                                    let etx = app.mission_event_tx.clone();
                                    tokio::spawn(async move {
                                        let mut runner = crate::mission::MissionRunner::new(config);
                                        runner.auto_mode = true;
                                        runner.event_tx = Some(etx.clone());
                                        if let Err(e) = runner.run(&p).await {
                                            let _ = etx.send(TuiEvent::MissionFailed {
                                                error: e.to_string(),
                                            });
                                        }
                                    });
                                } else {
                                    app.chat_messages
                                        .push(("system".into(), "Usage: /mission <prompt>".into()));
                                }
                                continue;
                            } else if msg.starts_with("/verify") {
                                let arg = msg.strip_prefix("/verify").unwrap_or("").trim();
                                let path = if arg.is_empty() {
                                    // Find most recent output directory
                                    let mut best: Option<std::path::PathBuf> = None;
                                    if let Ok(entries) = std::fs::read_dir("output") {
                                        for entry in entries.flatten() {
                                            let p = entry.path();
                                            if p.is_dir()
                                                && best.as_ref().is_none_or(|b| {
                                                    p.metadata()
                                                        .and_then(|m| m.modified())
                                                        .unwrap_or(
                                                            std::time::SystemTime::UNIX_EPOCH,
                                                        )
                                                        > b.metadata()
                                                            .and_then(|m| m.modified())
                                                            .unwrap_or(
                                                                std::time::SystemTime::UNIX_EPOCH,
                                                            )
                                                })
                                            {
                                                best = Some(p);
                                            }
                                        }
                                    }
                                    best
                                } else {
                                    Some(std::path::PathBuf::from(arg))
                                };
                                match path {
                                    Some(dir) if dir.exists() => {
                                        app.chat_messages.push((
                                            "system".into(),
                                            format!("Verifying {}...", dir.display()),
                                        ));
                                        match crate::verifier::verify_project(&dir, "python") {
                                            Ok(report) => {
                                                app.chat_messages.push(("system".into(), format!(
                                                        "Score: {:.1}/10 | Tests: {} passed, {} failed | Files: {}",
                                                        report.avg_score, report.tests_passed, report.tests_failed,
                                                        report.file_reports.len()
                                                    )));
                                                if !report.test_errors.is_empty() {
                                                    let errors: String = report
                                                        .test_errors
                                                        .iter()
                                                        .take(5)
                                                        .map(|e| format!("  {}", e))
                                                        .collect::<Vec<_>>()
                                                        .join("\n");
                                                    app.chat_messages.push((
                                                        "error".into(),
                                                        format!("Errors:\n{}", errors),
                                                    ));
                                                }
                                            }
                                            Err(e) => app.chat_messages.push((
                                                "error".into(),
                                                format!("Verify failed: {}", e),
                                            )),
                                        }
                                    }
                                    Some(dir) => app.chat_messages.push((
                                        "error".into(),
                                        format!("Not found: {}", dir.display()),
                                    )),
                                    None => app.chat_messages.push((
                                        "system".into(),
                                        "No output directory found. Usage: /verify [path]".into(),
                                    )),
                                }
                                continue;
                            } else if msg.starts_with("/report") {
                                let arg = msg.strip_prefix("/report").unwrap_or("").trim();
                                if arg == "list" || arg.is_empty() {
                                    match crate::report::list_reports() {
                                        Ok(reports) if reports.is_empty() => {
                                            app.chat_messages.push((
                                                "system".into(),
                                                "No reports yet. Run a mission first.".into(),
                                            ));
                                        }
                                        Ok(reports) => {
                                            app.chat_messages.push((
                                                "system".into(),
                                                format!("{} reports:", reports.len()),
                                            ));
                                            for r in reports.iter().rev().take(10) {
                                                app.chat_messages.push((
                                                    "system".into(),
                                                    format!("  {}", r.display()),
                                                ));
                                            }
                                        }
                                        Err(e) => app
                                            .chat_messages
                                            .push(("error".into(), format!("Failed: {}", e))),
                                    }
                                } else {
                                    // /report show [path]
                                    let report_path = if arg == "show" {
                                        std::path::PathBuf::from(
                                            ".battlecommand/reports/latest.json",
                                        )
                                    } else {
                                        let p = arg.strip_prefix("show ").unwrap_or(arg);
                                        std::path::PathBuf::from(p)
                                    };
                                    if !report_path.exists() {
                                        app.chat_messages.push((
                                            "error".into(),
                                            format!("Report not found: {}", report_path.display()),
                                        ));
                                    } else {
                                        match crate::report::load_report(&report_path) {
                                            Ok(report) => {
                                                app.chat_messages.push(("system".into(), format!(
                                                        "Mission: {} | Score: {:.1} | Rounds: {} | {}",
                                                        report.mission.prompt.chars().take(50).collect::<String>(),
                                                        report.result.best_score,
                                                        report.result.total_rounds,
                                                        if report.result.quality_gate_passed { "SHIPPED" } else { "NOT SHIPPED" }
                                                    )));
                                                if let Some(best) =
                                                    report.rounds.iter().max_by(|a, b| {
                                                        a.final_score
                                                            .partial_cmp(&b.final_score)
                                                            .unwrap_or(std::cmp::Ordering::Equal)
                                                    })
                                                {
                                                    let s = &best.critique.scores;
                                                    app.chat_messages.push(("system".into(), format!(
                                                            "Critique: DEV={:.1} ARCH={:.1} TEST={:.1} SEC={:.1} DOCS={:.1}",
                                                            s.dev, s.arch, s.test, s.sec, s.docs
                                                        )));
                                                }
                                            }
                                            Err(e) => app
                                                .chat_messages
                                                .push(("error".into(), format!("Failed: {}", e))),
                                        }
                                    }
                                }
                                continue;
                            } else if msg.starts_with("/audit") {
                                let arg = msg.strip_prefix("/audit").unwrap_or("").trim();
                                let limit: usize = arg.parse().unwrap_or(10);
                                match crate::enterprise::read_audit_log(limit) {
                                    Ok(entries) if entries.is_empty() => {
                                        app.chat_messages
                                            .push(("system".into(), "No audit entries.".into()));
                                    }
                                    Ok(entries) => {
                                        app.chat_messages.push((
                                            "system".into(),
                                            format!("Last {} audit entries:", entries.len()),
                                        ));
                                        for e in &entries {
                                            app.chat_messages.push((
                                                "system".into(),
                                                format!(
                                                    "[{}] {} {} — {}",
                                                    e.timestamp, e.actor, e.action, e.resource
                                                ),
                                            ));
                                        }
                                    }
                                    Err(e) => app
                                        .chat_messages
                                        .push(("error".into(), format!("Failed: {}", e))),
                                }
                                continue;
                            } else if msg.starts_with("/preset") {
                                let arg = msg.strip_prefix("/preset").unwrap_or("").trim();
                                match arg {
                                    "fast" | "balanced" | "premium" => {
                                        let preset_enum = arg
                                            .parse::<crate::model_config::Preset>()
                                            .unwrap_or(crate::model_config::Preset::Premium);
                                        app.model_config =
                                            crate::model_config::ModelConfig::resolve(
                                                preset_enum,
                                                ".",
                                                None,
                                                None,
                                                None,
                                                None,
                                            );
                                        app.chat_messages.push((
                                            "system".into(),
                                            format!("Switched to {} preset", arg),
                                        ));
                                        app.chat_messages.push((
                                            "system".into(),
                                            format!(
                                                "  Architect: {} | Coder: {} | CTO: {}",
                                                app.model_config.architect.model,
                                                app.model_config.coder.model,
                                                app.model_config.cto.model,
                                            ),
                                        ));
                                        app.log("info", format!("Preset: {}", arg));
                                    }
                                    _ => {
                                        app.chat_messages.push((
                                            "system".into(),
                                            "Usage: /preset <fast|balanced|premium>".into(),
                                        ));
                                    }
                                }
                                continue;
                            } else if msg == "/cost" {
                                match crate::enterprise::total_cost() {
                                    Ok(cost) => {
                                        app.chat_messages.push((
                                            "system".into(),
                                            format!("Total API cost: ${:.4}", cost),
                                        ));
                                    }
                                    Err(e) => app
                                        .chat_messages
                                        .push(("error".into(), format!("Failed: {}", e))),
                                }
                                continue;
                            } else if msg == "/help" {
                                app.chat_messages
                                    .push(("system".into(), "── Commands ──".into()));
                                app.chat_messages.push((
                                    "system".into(),
                                    "/mission <prompt> — Launch a mission".into(),
                                ));
                                app.chat_messages.push((
                                    "system".into(),
                                    "/verify [path]   — Run verifier (default: latest output)"
                                        .into(),
                                ));
                                app.chat_messages.push((
                                    "system".into(),
                                    "/report [list|show] — View pipeline reports".into(),
                                ));
                                app.chat_messages.push((
                                    "system".into(),
                                    "/audit [n]       — Show audit log (default: 10)".into(),
                                ));
                                app.chat_messages.push((
                                    "system".into(),
                                    "/preset <name>   — Switch preset (fast/balanced/premium)"
                                        .into(),
                                ));
                                app.chat_messages.push((
                                    "system".into(),
                                    "/cost            — Show total API cost".into(),
                                ));
                                app.chat_messages.push((
                                    "system".into(),
                                    "/settings        — Model picker".into(),
                                ));
                                app.chat_messages.push((
                                    "system".into(),
                                    "/clear           — Clear chat + CTO history".into(),
                                ));
                                app.chat_messages.push((
                                    "system".into(),
                                    "/compress        — Compact CTO history".into(),
                                ));
                                app.chat_messages.push((
                                    "system".into(),
                                    "/models /hw /status — Switch tabs / info".into(),
                                ));
                                app.chat_messages.push((
                                    "system".into(),
                                    "Or type any message to chat with CTO".into(),
                                ));
                                continue;
                            }

                            // ── Regular chat → CTO agent ──
                            app.chat_messages.push(("you".into(), msg.clone()));
                            app.log(
                                "info",
                                format!("Prompt: {}", &msg.chars().take(50).collect::<String>()),
                            );

                            // Initialize CTO agent on first use
                            if app.cto_agent.is_none() {
                                let cto_model = &app.model_config.cto.model;
                                let llm = LlmClient::with_limits(
                                    cto_model,
                                    app.model_config.cto.context_size(),
                                    app.model_config.cto.max_predict(),
                                );
                                let mut agent = crate::cto::CtoAgent::new(llm);
                                agent.set_model_config(app.model_config.clone());
                                agent.set_tui_event_tx(app.mission_event_tx.clone());
                                agent.load_history().ok();
                                app.cto_agent = Some(agent);
                                app.log(
                                    "info",
                                    format!(
                                        "CTO agent initialized ({})",
                                        app.model_config.cto.model
                                    ),
                                );
                            }

                            let (tx, rx) = mpsc::channel(512);
                            app.stream_rx = Some(rx);
                            app.is_generating = true;
                            app.stream_buffer.clear();
                            app.status_line =
                                format!("CTO STREAMING [{}]...", app.model_config.cto.model);

                            // Take agent, spawn async task, return via channel
                            let mut agent = app.cto_agent.take().unwrap();
                            let tx_clone = tx.clone();
                            tokio::spawn(async move {
                                agent.set_event_tx(tx_clone.clone());
                                match agent.chat(&msg).await {
                                    Ok(response) => {
                                        let _ = tx_clone.send(StreamEvent::Done(response)).await;
                                    }
                                    Err(e) => {
                                        let _ =
                                            tx_clone.send(StreamEvent::Error(e.to_string())).await;
                                    }
                                }
                                let _ = tx_clone
                                    .send(StreamEvent::AgentReturn(Box::new(agent)))
                                    .await;
                            });
                        }
                        // ── Input cursor movement ──
                        KeyCode::Backspace if app.input_cursor > 0 => {
                            app.input.remove(app.input_cursor - 1);
                            app.input_cursor -= 1;
                        }
                        KeyCode::Delete if app.input_cursor < app.input.len() => {
                            app.input.remove(app.input_cursor);
                        }
                        KeyCode::Left => {
                            app.input_cursor = app.input_cursor.saturating_sub(1);
                        }
                        KeyCode::Right if app.input_cursor < app.input.len() => {
                            app.input_cursor += 1;
                        }
                        KeyCode::Home => {
                            if app.input.is_empty() {
                                app.chat_auto_scroll = false;
                                app.chat_scroll = app.chat_total_lines;
                            } else {
                                app.input_cursor = 0;
                            }
                        }
                        KeyCode::End => {
                            if app.input.is_empty() {
                                app.chat_scroll = 0;
                                app.chat_auto_scroll = true;
                            } else {
                                app.input_cursor = app.input.len();
                            }
                        }
                        // ── Scrolling ──
                        KeyCode::PageUp => {
                            app.chat_auto_scroll = false;
                            app.chat_scroll = app.chat_scroll.saturating_add(20);
                        }
                        KeyCode::PageDown => {
                            if app.chat_scroll >= 20 {
                                app.chat_scroll -= 20;
                            } else {
                                app.chat_scroll = 0;
                                app.chat_auto_scroll = true;
                            }
                        }
                        KeyCode::Up if app.input.is_empty() => {
                            app.chat_auto_scroll = false;
                            app.chat_scroll = app.chat_scroll.saturating_add(3);
                        }
                        KeyCode::Down if app.input.is_empty() => {
                            if app.chat_scroll >= 3 {
                                app.chat_scroll -= 3;
                            } else {
                                app.chat_scroll = 0;
                                app.chat_auto_scroll = true;
                            }
                        }
                        KeyCode::Esc => {
                            if app.input.is_empty() {
                                app.should_quit = true;
                            } else {
                                app.input.clear();
                                app.input_cursor = 0;
                            }
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.should_quit = true;
                        }
                        KeyCode::Char(c) => {
                            if app.input.is_empty() && matches!(c, '1'..='6') {
                                match c {
                                    '1' => app.current_tab = Tab::Chat,
                                    '2' => app.current_tab = Tab::Queue,
                                    '3' => app.current_tab = Tab::Models,
                                    '4' => app.current_tab = Tab::Code,
                                    '5' => app.current_tab = Tab::Hw,
                                    '6' => app.current_tab = Tab::Log,
                                    _ => {}
                                }
                            } else {
                                app.input.insert(app.input_cursor, c);
                                app.input_cursor += 1;
                            }
                        }
                        KeyCode::Tab => {
                            app.current_tab = app.current_tab.next();
                        }
                        _ => {}
                    }
                } else {
                    // Non-Chat tabs: scroll handling for Code + Log
                    match key.code {
                        KeyCode::PageUp => match app.current_tab {
                            Tab::Code => {
                                app.code_auto_scroll = false;
                                app.code_scroll = app.code_scroll.saturating_add(20);
                            }
                            Tab::Log => {
                                app.log_auto_scroll = false;
                                app.log_scroll = app.log_scroll.saturating_add(20);
                            }
                            _ => {}
                        },
                        KeyCode::PageDown => match app.current_tab {
                            Tab::Code => {
                                if app.code_scroll >= 20 {
                                    app.code_scroll -= 20;
                                } else {
                                    app.code_scroll = 0;
                                    app.code_auto_scroll = true;
                                }
                            }
                            Tab::Log => {
                                if app.log_scroll >= 20 {
                                    app.log_scroll -= 20;
                                } else {
                                    app.log_scroll = 0;
                                    app.log_auto_scroll = true;
                                }
                            }
                            _ => {}
                        },
                        KeyCode::Up => match app.current_tab {
                            Tab::Code => {
                                app.code_auto_scroll = false;
                                app.code_scroll = app.code_scroll.saturating_add(3);
                            }
                            Tab::Log => {
                                app.log_auto_scroll = false;
                                app.log_scroll = app.log_scroll.saturating_add(3);
                            }
                            _ => {}
                        },
                        KeyCode::Down => match app.current_tab {
                            Tab::Code => {
                                if app.code_scroll >= 3 {
                                    app.code_scroll -= 3;
                                } else {
                                    app.code_scroll = 0;
                                    app.code_auto_scroll = true;
                                }
                            }
                            Tab::Log => {
                                if app.log_scroll >= 3 {
                                    app.log_scroll -= 3;
                                } else {
                                    app.log_scroll = 0;
                                    app.log_auto_scroll = true;
                                }
                            }
                            _ => {}
                        },
                        KeyCode::Home => match app.current_tab {
                            Tab::Code => {
                                app.code_auto_scroll = false;
                                app.code_scroll = app.code_total_lines;
                            }
                            Tab::Log => {
                                app.log_auto_scroll = false;
                                app.log_scroll = app.log_total_lines;
                            }
                            _ => {}
                        },
                        KeyCode::End => match app.current_tab {
                            Tab::Code => {
                                app.code_scroll = 0;
                                app.code_auto_scroll = true;
                            }
                            Tab::Log => {
                                app.log_scroll = 0;
                                app.log_auto_scroll = true;
                            }
                            _ => {}
                        },
                        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.should_quit = true
                        }
                        KeyCode::Char('1') => app.current_tab = Tab::Chat,
                        KeyCode::Char('2') => app.current_tab = Tab::Queue,
                        KeyCode::Char('3') => app.current_tab = Tab::Models,
                        KeyCode::Char('4') => app.current_tab = Tab::Code,
                        KeyCode::Char('5') => app.current_tab = Tab::Hw,
                        KeyCode::Char('6') => app.current_tab = Tab::Log,
                        KeyCode::Tab => app.current_tab = app.current_tab.next(),
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    Ok(())
}

// ── Renderers ──

fn wrapped_line_count(text: &str, width: usize) -> u16 {
    if width == 0 {
        return 1;
    }
    let len = text.len();
    if len <= width {
        1
    } else {
        len.div_ceil(width) as u16
    }
}

fn render_chat<'a>(
    messages: &[(String, String)],
    stream: &str,
    generating: bool,
    scroll_offset: u16,
    auto_scroll: bool,
    visible_height: u16,
    content_width: usize,
    cto_model: &str,
) -> (Paragraph<'a>, u16) {
    let mut lines: Vec<Line> = Vec::new();
    let mut visual_total: u16 = 0;
    for (role, content) in messages {
        let (prefix, style, display_content) = match role.as_str() {
            "you" => {
                // Truncate long user messages (v2 parity)
                let display = if content.len() > 80 {
                    format!(
                        "{}... [{} chars]",
                        &content[..77.min(content.len())],
                        content.len()
                    )
                } else {
                    content.clone()
                };
                (
                    "[YOU] ".to_string(),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                    display,
                )
            }
            "cto" => (
                format!("[{}] ", cto_model),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
                content.clone(),
            ),
            "error" => (
                "[ERR] ".to_string(),
                Style::default().fg(Color::Red),
                content.clone(),
            ),
            "tool" => (
                "[TOOL] ".to_string(),
                Style::default().fg(Color::Magenta),
                content.clone(),
            ),
            "tool_result" => (
                "  ".to_string(),
                Style::default().fg(Color::DarkGray),
                content.clone(),
            ),
            _ => (
                "[SYS] ".to_string(),
                Style::default().fg(Color::DarkGray),
                content.clone(),
            ),
        };
        for line in display_content.lines() {
            let text = format!("  {}{}", prefix, line);
            visual_total += wrapped_line_count(&text, content_width);
            lines.push(Line::from(Span::styled(text, style)));
        }
    }
    if !stream.is_empty() {
        let stream_lines: Vec<&str> = stream.lines().rev().take(5).collect();
        for line in stream_lines.into_iter().rev() {
            let text = format!("  [{} ...] {}", cto_model, line);
            visual_total += wrapped_line_count(&text, content_width);
            lines.push(Line::from(Span::styled(
                text,
                Style::default().fg(Color::Yellow),
            )));
        }
    }
    if generating && stream.is_empty() {
        visual_total += 1;
        lines.push(Line::from(Span::styled(
            "  Thinking...",
            Style::default().fg(Color::Yellow),
        )));
    }

    let total = visual_total;
    let scroll = if auto_scroll {
        total.saturating_sub(visible_height)
    } else {
        let max_scroll = total.saturating_sub(visible_height);
        max_scroll.saturating_sub(scroll_offset.min(max_scroll))
    };

    let title = if auto_scroll {
        " Chat [LIVE] ".to_string()
    } else {
        format!(" Chat [{}/{}] ", total.saturating_sub(scroll), total)
    };

    let para = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    (para, total)
}

fn render_queue<'a>(items: &'a [QueueItem]) -> Paragraph<'a> {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::from(""));

    if items.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No active missions.",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            "  Use /mission <prompt> or chat with CTO to launch.",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        // Header
        lines.push(Line::from(vec![
            Span::styled(
                "  Stage    ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Step           ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Model                    ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Status",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::from(Span::styled(
            "  ─────────────────────────────────────────────────────────────────",
            Style::default().fg(Color::DarkGray),
        )));

        for item in items {
            let status_color = if item.status == "running" {
                Color::Yellow
            } else if item.status.starts_with("done") {
                Color::Green
            } else {
                Color::Red
            };
            let status_marker = if item.status == "running" {
                ">>>"
            } else {
                "   "
            };
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  [{}] ", item.stage),
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(
                    format!("{:<15}", item.step),
                    Style::default().fg(Color::White),
                ),
                Span::styled(
                    format!("{:<25}", truncate_display(&item.model, 24)),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(status_marker, Style::default().fg(status_color)),
                Span::styled(&item.status, Style::default().fg(status_color)),
            ]));
        }
    }

    Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Queue "))
}

fn truncate_display(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

fn render_models<'a>() -> Paragraph<'a> {
    Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Model Configuration",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  [premium]  qwen3-coder:30b-a3b-q8_0",
            Style::default().fg(Color::Green),
        )),
        Line::from(Span::styled(
            "  [balanced] qwen2.5-coder:32b",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            "  [fast]     qwen2.5-coder:7b",
            Style::default().fg(Color::Red),
        )),
        Line::from(""),
        Line::from("  Claude API: set ANTHROPIC_API_KEY for Sonnet"),
        Line::from(""),
        Line::from("  CLI: battlecommand-forge models list|benchmark|presets"),
    ])
    .block(Block::default().borders(Borders::ALL).title(" Models "))
}

fn render_code<'a>(
    content: &str,
    model: &str,
    streaming: bool,
    history: &[String],
    display_len: usize,
    scroll_offset: u16,
    auto_scroll: bool,
    visible_height: u16,
    content_width: usize,
) -> (Paragraph<'a>, u16) {
    let mut lines: Vec<Line> = Vec::new();
    let mut visual_total: u16 = 0;

    if content.is_empty() && history.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  No code being generated.",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            "  Start a mission or chat with the CTO.",
            Style::default().fg(Color::DarkGray),
        )));
        visual_total = 3;
    } else {
        let full_content = if content.is_empty() && !history.is_empty() {
            history.last().unwrap().as_str()
        } else {
            content
        };

        // Typewriter: only show first display_len chars
        let typewriter_active = display_len < full_content.len() && !full_content.is_empty();
        let display = if typewriter_active {
            // Safe char boundary
            let mut safe = display_len;
            while safe < full_content.len() && !full_content.is_char_boundary(safe) {
                safe += 1;
            }
            &full_content[..safe]
        } else {
            full_content
        };

        for line in display.lines() {
            let text = format!("  {}", line);
            visual_total += wrapped_line_count(&text, content_width);
            lines.push(Line::from(Span::styled(
                text,
                Style::default().fg(Color::Green),
            )));
        }

        // Blinking cursor during typewriter or streaming
        if typewriter_active || streaming {
            visual_total += 1;
            lines.push(Line::from(Span::styled(
                "  \u{2588}",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::SLOW_BLINK),
            )));
        }
    }

    let total = visual_total;
    let scroll = if auto_scroll {
        total.saturating_sub(visible_height)
    } else {
        let max_scroll = total.saturating_sub(visible_height);
        max_scroll.saturating_sub(scroll_offset.min(max_scroll))
    };

    let title = if streaming {
        format!(" Code [{}|streaming] ", model)
    } else if display_len < content.len() && !content.is_empty() {
        format!(" Code [{}|typewriter] ", model)
    } else if auto_scroll {
        if model.is_empty() {
            " Code ".to_string()
        } else {
            format!(" Code [{}] ", model)
        }
    } else {
        format!(" Code [{}/{}] ", total.saturating_sub(scroll), total)
    };

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .style(Style::default().bg(Color::Black)),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    (para, total)
}

fn render_hw<'a>(hw_lines: &[String]) -> Paragraph<'a> {
    let mut lines = vec![Line::from("")];
    lines.push(Line::from(Span::styled(
        "  Hardware Monitor",
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(""));
    for line in hw_lines {
        // Color code based on content
        let style = if line
            .contains("\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}")
        {
            Style::default().fg(Color::Red)
        } else if line.contains("\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}\u{2588}") {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::Green)
        };
        lines.push(Line::from(Span::styled(format!("  {}", line), style)));
    }
    Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(" Hardware "))
}

fn render_log<'a>(
    entries: &[LogEntry],
    thinking: &[ThinkingEntry],
    scroll_offset: u16,
    auto_scroll: bool,
    visible_height: u16,
    content_width: usize,
) -> (Paragraph<'a>, u16) {
    let mut lines: Vec<Line> = Vec::new();
    let mut visual_total: u16 = 0;

    // Thinking section (top) — show last thinking entry
    if let Some(last) = thinking.last() {
        let status = if last.is_active {
            Line::from(Span::styled(
                format!("  [{}] thinking...", last.model),
                Style::default().fg(Color::Yellow),
            ))
        } else {
            Line::from(Span::styled(
                format!("  [{}] done", last.model),
                Style::default().fg(Color::DarkGray),
            ))
        };
        visual_total += 1;
        lines.push(status);

        // Show last few lines of thinking content (dimmed)
        for line in last
            .content
            .lines()
            .rev()
            .take(5)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            let text = format!("  {}", line);
            visual_total += wrapped_line_count(&text, content_width);
            lines.push(Line::from(Span::styled(
                text,
                Style::default().fg(Color::DarkGray),
            )));
        }

        let sep = "  ─────────────────────────────────────────";
        visual_total += 1;
        lines.push(Line::from(Span::styled(
            sep,
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Log entries (bottom)
    for e in entries
        .iter()
        .rev()
        .take(100)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let level_color = match e.level.as_str() {
            "error" => Color::Red,
            "warn" => Color::Yellow,
            "info" => Color::Green,
            "debug" => Color::DarkGray,
            _ => Color::White,
        };
        let text = format!(
            "  [{}] {:5} {}",
            e.timestamp,
            e.level.to_uppercase(),
            e.message
        );
        visual_total += wrapped_line_count(&text, content_width);
        lines.push(Line::from(vec![
            Span::styled(
                format!("  [{}] ", e.timestamp),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{:5} ", e.level.to_uppercase()),
                Style::default().fg(level_color),
            ),
            Span::styled(e.message.clone(), Style::default().fg(Color::White)),
        ]));
    }

    let total = visual_total;
    let scroll = if auto_scroll {
        total.saturating_sub(visible_height)
    } else {
        let max_scroll = total.saturating_sub(visible_height);
        max_scroll.saturating_sub(scroll_offset.min(max_scroll))
    };

    let title = if auto_scroll {
        format!(" Log ({} entries) [LIVE] ", entries.len())
    } else {
        format!(
            " Log ({} entries) [{}/{}] ",
            entries.len(),
            total.saturating_sub(scroll),
            total
        )
    };

    let para = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(title))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    (para, total)
}

/// Status bar: FORGE badge | status | [completed/total] | Cost | VRAM | help
fn render_status_bar<'a>(
    status: &str,
    completed: usize,
    total_tasks: usize,
    cost: f64,
    vram: f64,
) -> Paragraph<'a> {
    let mut spans: Vec<Span> = vec![
        Span::styled(
            " FORGE ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(status.to_string(), Style::default().fg(Color::Yellow)),
    ];

    if total_tasks > 0 {
        spans.push(Span::styled(
            format!(" [{}/{}]", completed, total_tasks),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
    }

    spans.push(Span::raw("  |  "));
    spans.push(Span::styled(
        format!("Cost: ${:.4}", cost),
        Style::default().fg(Color::Green),
    ));

    if vram.abs() > 0.01 {
        spans.push(Span::raw("  |  "));
        let vram_color = if vram > 40.0 {
            Color::Red
        } else if vram > 20.0 {
            Color::Yellow
        } else {
            Color::Green
        };
        spans.push(Span::styled(
            format!("VRAM {:.0}G", vram.abs()),
            Style::default().fg(vram_color).add_modifier(Modifier::BOLD),
        ));
    }

    spans.push(Span::raw("  |  "));
    spans.push(Span::styled(
        "Tab | Ctrl+C quit",
        Style::default().fg(Color::DarkGray),
    ));

    Paragraph::new(Line::from(spans))
}

/// Extract code blocks (``` fenced) from a CTO response.
fn extract_code_blocks(text: &str) -> String {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut current = String::new();

    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            if in_block {
                blocks.push(current.clone());
                current.clear();
                in_block = false;
            } else {
                in_block = true;
            }
        } else if in_block {
            current.push_str(line);
            current.push('\n');
        }
    }
    blocks.join("\n---\n\n")
}

/// Convert Ollama model list to AvailableModel list for picker (includes Claude cloud models).
fn to_available_models(models: &[crate::models::ModelInfo]) -> Vec<AvailableModel> {
    let mut available: Vec<AvailableModel> = models
        .iter()
        .map(|m| {
            let size_gb = m.size.trim_end_matches(" GB").parse::<f64>().unwrap_or(0.0);
            AvailableModel {
                name: m.name.clone(),
                size_gb,
                provider: crate::model_config::ModelProvider::Local,
            }
        })
        .collect();
    // Sort by size descending (bigger models first)
    available.sort_by(|a, b| {
        b.size_gb
            .partial_cmp(&a.size_gb)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Append Claude cloud models
    for &(model_id, _label) in model_picker::CLAUDE_MODELS {
        available.push(AvailableModel {
            name: model_id.to_string(),
            size_gb: 0.0,
            provider: crate::model_config::ModelProvider::Cloud,
        });
    }

    available
}
