use crossterm::event::KeyCode;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
/// Easter egg: Retro snake game — type /snake in chat!
/// Ported from battleclaw-v2. macOS audio (say + afplay).
use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

const GRID_W: u16 = 40;
const GRID_H: u16 = 20;
const INITIAL_SPEED: u64 = 3;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

pub struct SnakeGame {
    snake: VecDeque<(u16, u16)>,
    direction: Direction,
    next_direction: Direction,
    food: (u16, u16),
    pub score: u32,
    high_score: u32,
    alive: bool,
    pub game_over: bool,
    width: u16,
    height: u16,
    tick_count: u64,
    speed: u64,
    music_child: Option<std::process::Child>,
    rng_state: u64,
}

impl Default for SnakeGame {
    fn default() -> Self {
        Self::new()
    }
}

impl SnakeGame {
    pub fn new() -> Self {
        let mid_x = GRID_W / 2;
        let mid_y = GRID_H / 2;
        let mut snake = VecDeque::new();
        snake.push_back((mid_x, mid_y));
        snake.push_back((mid_x - 1, mid_y));
        snake.push_back((mid_x - 2, mid_y));

        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let mut game = Self {
            snake,
            direction: Direction::Right,
            next_direction: Direction::Right,
            food: (0, 0),
            score: 0,
            high_score: 0,
            alive: true,
            game_over: false,
            width: GRID_W,
            height: GRID_H,
            tick_count: 0,
            speed: INITIAL_SPEED,
            music_child: None,
            rng_state: seed,
        };
        game.spawn_food();
        game.start_music();
        game
    }

    fn next_rand(&mut self) -> u64 {
        self.rng_state ^= self.rng_state << 13;
        self.rng_state ^= self.rng_state >> 7;
        self.rng_state ^= self.rng_state << 17;
        self.rng_state
    }

    fn spawn_food(&mut self) {
        for _ in 0..200 {
            let r = self.next_rand();
            let x = (r % self.width as u64) as u16;
            let y = ((r >> 16) % self.height as u64) as u16;
            if !self.snake.contains(&(x, y)) {
                self.food = (x, y);
                return;
            }
        }
    }

    pub fn tick(&mut self) {
        if !self.alive || self.game_over {
            return;
        }
        self.tick_count += 1;
        if !self.tick_count.is_multiple_of(self.speed) {
            return;
        }

        self.direction = self.next_direction;
        let (hx, hy) = *self.snake.front().unwrap();
        let new_head = match self.direction {
            Direction::Up => {
                if hy == 0 {
                    self.die();
                    return;
                }
                (hx, hy - 1)
            }
            Direction::Down => (hx, hy + 1),
            Direction::Left => {
                if hx == 0 {
                    self.die();
                    return;
                }
                (hx - 1, hy)
            }
            Direction::Right => (hx + 1, hy),
        };

        if new_head.0 >= self.width || new_head.1 >= self.height {
            self.die();
            return;
        }
        if self.snake.contains(&new_head) {
            self.die();
            return;
        }

        self.snake.push_front(new_head);
        if new_head == self.food {
            self.score += 10;
            if self.score.is_multiple_of(50) && self.speed > 1 {
                self.speed -= 1;
            }
            self.spawn_food();
        } else {
            self.snake.pop_back();
        }
    }

    fn die(&mut self) {
        self.alive = false;
        self.game_over = true;
        self.high_score = self.high_score.max(self.score);
        self.stop_music();
        let score = self.score;
        std::thread::spawn(move || {
            if cfg!(target_os = "macos") {
                let _ = std::process::Command::new("say")
                    .args(["-v", "Trinoids", &format!("Game over. Score {}.", score)])
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
            }
        });
    }

    pub fn handle_input(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.stop_music();
                return true;
            }
            KeyCode::Up | KeyCode::Char('w')
                if self.direction != Direction::Down => {
                    self.next_direction = Direction::Up;
                }
            KeyCode::Down | KeyCode::Char('s')
                if self.direction != Direction::Up => {
                    self.next_direction = Direction::Down;
                }
            KeyCode::Left | KeyCode::Char('a')
                if self.direction != Direction::Right => {
                    self.next_direction = Direction::Left;
                }
            KeyCode::Right | KeyCode::Char('d')
                if self.direction != Direction::Left => {
                    self.next_direction = Direction::Right;
                }
            KeyCode::Enter if self.game_over => {
                let hs = self.high_score.max(self.score);
                *self = SnakeGame::new();
                self.high_score = hs;
            }
            _ => {}
        }
        false
    }

    fn start_music(&mut self) {
        if !cfg!(target_os = "macos") {
            return;
        }
        std::thread::spawn(|| {
            let _ = std::process::Command::new("say")
                .args(["-v", "Trinoids", "Snake mode activated"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        });
        if let Ok(child) = std::process::Command::new("bash")
            .args(["-c", "while true; do afplay -r 2.0 /System/Library/Sounds/Tink.aiff 2>/dev/null; sleep 0.12; done"])
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() { self.music_child = Some(child) }
    }

    fn stop_music(&mut self) {
        if let Some(ref mut child) = self.music_child {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.music_child = None;
    }

    pub fn draw(&self, f: &mut Frame, area: Rect) {
        f.render_widget(Clear, area);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(
                " SNAKE | Score: {} | High: {} | Speed: {} ",
                self.score,
                self.high_score,
                INITIAL_SPEED + 1 - self.speed
            ))
            .title_style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Green));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let cell_w = 2u16;
        let visible_w = (inner.width / cell_w).min(self.width);
        let visible_h = inner.height.min(self.height);
        let mut lines: Vec<Line> = Vec::new();

        for y in 0..visible_h {
            let mut spans: Vec<Span> = Vec::new();
            for x in 0..visible_w {
                let pos = (x, y);
                if Some(&pos) == self.snake.front() {
                    spans.push(Span::styled("██", Style::default().fg(Color::LightGreen)));
                } else if self.snake.contains(&pos) {
                    spans.push(Span::styled("██", Style::default().fg(Color::Green)));
                } else if pos == self.food {
                    spans.push(Span::styled("██", Style::default().fg(Color::Red)));
                } else {
                    spans.push(Span::raw("  "));
                }
            }
            lines.push(Line::from(spans));
        }
        f.render_widget(Paragraph::new(lines), inner);

        if self.game_over {
            let ow = 34u16;
            let oh = 7u16;
            let ox = area.x + area.width.saturating_sub(ow) / 2;
            let oy = area.y + area.height.saturating_sub(oh) / 2;
            let oa = Rect::new(ox, oy, ow, oh);
            f.render_widget(Clear, oa);
            let go = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    "  ╔══ GAME OVER ══╗",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    format!("  ║  Score: {:<7}║", self.score),
                    Style::default().fg(Color::Yellow),
                )),
                Line::from(Span::styled(
                    format!("  ║  High:  {:<7}║", self.high_score),
                    Style::default().fg(Color::Green),
                )),
                Line::from(Span::styled(
                    "  ╚═══════════════╝",
                    Style::default().fg(Color::Red),
                )),
                Line::from(Span::styled(
                    "  Enter=Restart Esc=Exit",
                    Style::default().fg(Color::DarkGray),
                )),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Red)),
            );
            f.render_widget(go, oa);
        }
    }
}

impl Drop for SnakeGame {
    fn drop(&mut self) {
        self.stop_music();
    }
}
