use crossterm::event::KeyCode;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
/// Easter egg: Space Invaders — type /space in chat!
use std::time::{SystemTime, UNIX_EPOCH};

const GRID_W: u16 = 40;
const GRID_H: u16 = 22;
const INVADER_ROWS: u16 = 4;
const INVADER_COLS: u16 = 8;
const INVADER_SPACING_X: u16 = 4;
const INVADER_SPACING_Y: u16 = 2;
const PLAYER_Y: u16 = GRID_H - 2;
const INVADER_MOVE_RATE: u64 = 12;
const BULLET_RATE: u64 = 2;

#[derive(Clone, Copy)]
struct Bullet {
    x: u16,
    y: i16,
    direction: i16, // -1 = up (player), +1 = down (enemy)
}

pub struct SpaceGame {
    player_x: u16,
    invaders: Vec<(u16, u16, bool)>, // x, y, alive
    bullets: Vec<Bullet>,
    pub score: u32,
    high_score: u32,
    alive: bool,
    pub game_over: bool,
    invader_dir: i16, // 1 = right, -1 = left
    invader_drop: bool,
    tick_count: u64,
    shoot_cooldown: u64,
    enemy_shoot_timer: u64,
    rng_state: u64,
    music_child: Option<std::process::Child>,
    victory: bool,
}

impl Default for SpaceGame {
    fn default() -> Self {
        Self::new()
    }
}

impl SpaceGame {
    pub fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let mut game = Self {
            player_x: GRID_W / 2,
            invaders: Vec::new(),
            bullets: Vec::new(),
            score: 0,
            high_score: 0,
            alive: true,
            game_over: false,
            invader_dir: 1,
            invader_drop: false,
            tick_count: 0,
            shoot_cooldown: 0,
            enemy_shoot_timer: 0,
            rng_state: seed,
            music_child: None,
            victory: false,
        };
        game.spawn_invaders();
        game.start_music();
        game
    }

    fn spawn_invaders(&mut self) {
        self.invaders.clear();
        let start_x = (GRID_W - INVADER_COLS * INVADER_SPACING_X) / 2;
        for row in 0..INVADER_ROWS {
            for col in 0..INVADER_COLS {
                self.invaders.push((
                    start_x + col * INVADER_SPACING_X,
                    2 + row * INVADER_SPACING_Y,
                    true,
                ));
            }
        }
    }

    fn next_rand(&mut self) -> u64 {
        self.rng_state ^= self.rng_state << 13;
        self.rng_state ^= self.rng_state >> 7;
        self.rng_state ^= self.rng_state << 17;
        self.rng_state
    }

    pub fn tick(&mut self) {
        if !self.alive || self.game_over {
            return;
        }
        self.tick_count += 1;

        // Move bullets
        if self.tick_count.is_multiple_of(BULLET_RATE) {
            let mut hits: Vec<usize> = Vec::new();
            for (bi, bullet) in self.bullets.iter_mut().enumerate() {
                bullet.y += bullet.direction;
                if bullet.y < 0 || bullet.y >= GRID_H as i16 {
                    hits.push(bi);
                    continue;
                }
                // Player bullet hits invader
                if bullet.direction == -1 {
                    for inv in self.invaders.iter_mut() {
                        if inv.2 && bullet.x.abs_diff(inv.0) <= 1 && bullet.y == inv.1 as i16 {
                            inv.2 = false;
                            hits.push(bi);
                            self.score += 10;
                            break;
                        }
                    }
                }
                // Enemy bullet hits player
                if bullet.direction == 1
                    && bullet.y == PLAYER_Y as i16
                    && bullet.x.abs_diff(self.player_x) <= 1
                {
                    self.die();
                    return;
                }
            }
            hits.sort_unstable();
            hits.dedup();
            for i in hits.into_iter().rev() {
                if i < self.bullets.len() {
                    self.bullets.remove(i);
                }
            }
        }

        // Move invaders
        if self.tick_count.is_multiple_of(INVADER_MOVE_RATE) {
            if self.invader_drop {
                for inv in self.invaders.iter_mut() {
                    if inv.2 {
                        inv.1 += 1;
                    }
                }
                self.invader_drop = false;
                // Check if invaders reached player
                for inv in &self.invaders {
                    if inv.2 && inv.1 >= PLAYER_Y {
                        self.die();
                        return;
                    }
                }
            } else {
                let mut should_drop = false;
                for inv in self.invaders.iter_mut() {
                    if !inv.2 {
                        continue;
                    }
                    let new_x = inv.0 as i16 + self.invader_dir;
                    if new_x < 0 || new_x >= GRID_W as i16 {
                        should_drop = true;
                        break;
                    }
                }
                if should_drop {
                    self.invader_dir = -self.invader_dir;
                    self.invader_drop = true;
                } else {
                    for inv in self.invaders.iter_mut() {
                        if inv.2 {
                            inv.0 = (inv.0 as i16 + self.invader_dir) as u16;
                        }
                    }
                }
            }
        }

        // Enemy shooting
        self.enemy_shoot_timer += 1;
        if self.enemy_shoot_timer >= 20 {
            self.enemy_shoot_timer = 0;
            let alive_count = self.invaders.iter().filter(|i| i.2).count();
            if alive_count > 0 {
                let idx = self.next_rand() as usize % alive_count;
                let inv = self.invaders.iter().filter(|i| i.2).nth(idx).copied();
                if let Some((x, y, _)) = inv {
                    self.bullets.push(Bullet {
                        x,
                        y: y as i16 + 1,
                        direction: 1,
                    });
                }
            }
        }

        if self.shoot_cooldown > 0 {
            self.shoot_cooldown -= 1;
        }

        // Victory check
        if self.invaders.iter().all(|i| !i.2) {
            self.victory = true;
            self.game_over = true;
            self.high_score = self.high_score.max(self.score);
            self.stop_music();
            let score = self.score;
            std::thread::spawn(move || {
                if cfg!(target_os = "macos") {
                    let _ = std::process::Command::new("say")
                        .args(["-v", "Trinoids", &format!("Victory. Score {}.", score)])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .spawn();
                }
            });
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
            KeyCode::Left | KeyCode::Char('a')
                if self.player_x > 1 => {
                    self.player_x -= 1;
                }
            KeyCode::Right | KeyCode::Char('d')
                if self.player_x < GRID_W - 2 => {
                    self.player_x += 1;
                }
            KeyCode::Char(' ') | KeyCode::Up
                if self.alive && self.shoot_cooldown == 0 => {
                    self.bullets.push(Bullet {
                        x: self.player_x,
                        y: PLAYER_Y as i16 - 1,
                        direction: -1,
                    });
                    self.shoot_cooldown = 4;
                }
            KeyCode::Enter if self.game_over => {
                let hs = self.high_score.max(self.score);
                *self = SpaceGame::new();
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
                .args(["-v", "Trinoids", "Space invaders activated"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        });
        if let Ok(child) = std::process::Command::new("bash")
            .args(["-c", "while true; do afplay -r 1.5 /System/Library/Sounds/Pop.aiff 2>/dev/null; sleep 0.3; done"])
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
                " SPACE INVADERS | Score: {} | High: {} ",
                self.score, self.high_score
            ))
            .title_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(Color::Cyan));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let cell_w = 2u16;
        let visible_w = (inner.width / cell_w).min(GRID_W);
        let visible_h = inner.height.min(GRID_H);
        let mut lines: Vec<Line> = Vec::new();

        for y in 0..visible_h {
            let mut spans: Vec<Span> = Vec::new();
            for x in 0..visible_w {
                // Player
                if y == PLAYER_Y && x.abs_diff(self.player_x) <= 1 {
                    if x == self.player_x {
                        spans.push(Span::styled(
                            "/\\",
                            Style::default()
                                .fg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        ));
                    } else if x == self.player_x.wrapping_sub(1) {
                        spans.push(Span::styled("[=", Style::default().fg(Color::Green)));
                    } else {
                        spans.push(Span::styled("=]", Style::default().fg(Color::Green)));
                    }
                    continue;
                }
                // Invaders
                let mut drawn = false;
                for inv in &self.invaders {
                    if inv.2 && inv.0 == x && inv.1 == y {
                        let row = self.invaders.iter().position(|i| i == inv).unwrap_or(0)
                            / INVADER_COLS as usize;
                        let color = match row {
                            0 => Color::Red,
                            1 => Color::Magenta,
                            2 => Color::Yellow,
                            _ => Color::LightRed,
                        };
                        spans.push(Span::styled("<>", Style::default().fg(color)));
                        drawn = true;
                        break;
                    }
                }
                if drawn {
                    continue;
                }
                // Bullets
                let mut bullet_drawn = false;
                for bullet in &self.bullets {
                    if bullet.x == x && bullet.y == y as i16 {
                        let (ch, color) = if bullet.direction == -1 {
                            ("||", Color::White)
                        } else {
                            ("::", Color::Red)
                        };
                        spans.push(Span::styled(ch, Style::default().fg(color)));
                        bullet_drawn = true;
                        break;
                    }
                }
                if bullet_drawn {
                    continue;
                }
                // Empty
                spans.push(Span::raw("  "));
            }
            lines.push(Line::from(spans));
        }
        f.render_widget(Paragraph::new(lines), inner);

        // Game over / victory overlay
        if self.game_over {
            let title = if self.victory {
                "VICTORY!"
            } else {
                "GAME OVER"
            };
            let title_color = if self.victory {
                Color::Green
            } else {
                Color::Red
            };
            let ow = 34u16;
            let oh = 7u16;
            let ox = area.x + area.width.saturating_sub(ow) / 2;
            let oy = area.y + area.height.saturating_sub(oh) / 2;
            let oa = Rect::new(ox, oy, ow, oh);
            f.render_widget(Clear, oa);
            let go = Paragraph::new(vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!("  == {} ==", title),
                    Style::default()
                        .fg(title_color)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    format!("  Score: {:<7}", self.score),
                    Style::default().fg(Color::Yellow),
                )),
                Line::from(Span::styled(
                    format!("  High:  {:<7}", self.high_score),
                    Style::default().fg(Color::Green),
                )),
                Line::from(Span::styled(
                    "  ═══════════════════",
                    Style::default().fg(title_color),
                )),
                Line::from(Span::styled(
                    "  Enter=Restart Esc=Exit",
                    Style::default().fg(Color::DarkGray),
                )),
            ])
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(title_color)),
            );
            f.render_widget(go, oa);
        }
    }
}

impl Drop for SpaceGame {
    fn drop(&mut self) {
        self.stop_music();
    }
}
