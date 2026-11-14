//! O heroi: personagem de plataforma controlado pelo jogador (logica pura, sem Bevy/SO).
//!
//! O mundo dele e a area de trabalho:
//!
//! * **apoios** ([`Platform`]): o chao da tela, o topo de cada janela e icone e, dentro dos
//!   aplicativos, as linhas de texto e bordas que a "visao" encontrou
//!   ([`crate::navigation::vision`]). Todos sao de mao unica: da para subir pulando por baixo e
//!   descer segurando "baixo";
//! * **escadas** (segmentos verticais): as laterais das janelas, as paredes da tela e os fios de
//!   seda das aranhas. "Cima" perto de uma escada agarra e sobe.
//!
//! Combate: golpe de espada (caixa a frente), pisao (cair em cima de um bicho) e dano por contato.
//! Coleta: moedas, estrelas (missao: chegar ate um lugar distante) e coracoes.

use bevy::math::Vec2;

use crate::config::HeroConfig;
use crate::navigation::geometry::{Rect, Segment, subtract_intervals};
use crate::navigation::vision::{Ledge, LedgeKind};
use crate::platform::DesktopWindow;
use crate::rng::Rng;

/// Duracao do golpe de espada (s).
pub const SWING_SECS: f32 = 0.22;
/// Passo maximo da fisica (s): quedas rapidas nunca atravessam um apoio.
const MAX_STEP: f32 = 1.0 / 120.0;
const COYOTE_SECS: f32 = 0.10;
const JUMP_BUFFER_SECS: f32 = 0.12;
const TERMINAL_SPEED: f32 = 1700.0;
/// No ar, segurando "cima" perto de uma escada, agarra quando a subida ja perdeu forca
/// (velocidade vertical acima disto): um toque curto pula sem grudar na parede.
const GRAB_RISE: f32 = -280.0;
/// No fim de uma escada, distancia (px) ate outra que continue subindo (troca sozinho).
const LADDER_HANDOVER: f32 = 40.0;
/// Distancia (px) a que o heroi alcanca uma escada.
const LADDER_REACH: f32 = 16.0;
/// Um apoio que se mexeu ate isso (px) na vertical continua sendo "o mesmo" sob os pes.
const GROUND_FOLLOW: f32 = 8.0;

/// O que e o apoio onde o heroi pisa (para ele "identificar" as coisas).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ground {
    /// Base da tela.
    Floor,
    /// Topo de uma janela de aplicativo.
    WindowTop,
    /// Topo de um elemento do desktop (icone, barra).
    Element,
    /// Linha de texto dentro de um aplicativo.
    Text,
    /// Borda (botao, caixa, divisoria, imagem) dentro de um aplicativo.
    Line,
}

impl Ground {
    /// Dentro de um aplicativo (achado pela visao)?
    pub fn is_inside_app(self) -> bool {
        matches!(self, Ground::Text | Ground::Line)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Platform {
    pub x0: f32,
    pub x1: f32,
    pub y: f32,
    /// Janela/elemento dono (id de [`DesktopWindow`]); 0 = a tela.
    pub owner: u64,
    pub what: Ground,
    /// Nao da para atravessar descendo (so o chao da tela).
    pub solid: bool,
}

impl Platform {
    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    fn holds(&self, x: f32, slack: f32) -> bool {
        x >= self.x0 - slack && x <= self.x1 + slack
    }
}

/// Apoios encontrados na imagem de uma janela (coordenadas da imagem).
#[derive(Debug, Clone, Default)]
pub struct WindowLedges {
    pub ledges: Vec<Ledge>,
    pub image_size: (u32, u32),
}

/// Monta o mundo do heroi a partir das janelas (da frente para o fundo, px locais) e dos apoios
/// vistos dentro de cada uma. Devolve (apoios, escadas). `threads` sao os fios de seda do grafo.
pub fn build_world(
    screen: &Rect,
    windows: &[DesktopWindow],
    inside: &dyn Fn(u64) -> Option<WindowLedges>,
    threads: &[Segment],
    min_len: f32,
) -> (Vec<Platform>, Vec<Segment>) {
    // barras do sistema (fora da area util): blocos onde ninguem entra
    let panels: Vec<Rect> = windows
        .iter()
        .filter(|w| w.is_panel())
        .map(|w| w.rect)
        .collect();
    // apoio colado embaixo de uma barra: nao cabe ninguem em pe ali
    let no_headroom = |x0: f32, x1: f32, y: f32| {
        let above = Vec2::new((x0 + x1) * 0.5, y - 24.0);
        panels.iter().any(|p| p.contains(above))
    };
    let mut platforms: Vec<Platform> = Vec::new();
    // chao da tela, menos onde ha barra (la o chao e o topo da barra, abaixo)
    let floor_cuts: Vec<(f32, f32)> = panels
        .iter()
        .filter(|p| p.max.y >= screen.max.y - 1.0)
        .map(|p| (p.min.x, p.max.x))
        .collect();
    for (x0, x1) in subtract_intervals((screen.min.x, screen.max.x), &floor_cuts) {
        platforms.push(Platform {
            x0,
            x1,
            y: screen.max.y,
            owner: 0,
            what: Ground::Floor,
            solid: true,
        });
    }
    // paredes da tela: escadas de cima a baixo (sem os trechos dentro das barras)
    let mut ladders = Vec::new();
    for x in [screen.min.x + 6.0, screen.max.x - 6.0] {
        let cuts: Vec<(f32, f32)> = panels
            .iter()
            .filter(|p| x >= p.min.x && x <= p.max.x)
            .map(|p| (p.min.y, p.max.y))
            .collect();
        for (y0, y1) in subtract_intervals((screen.min.y, screen.max.y), &cuts) {
            if y1 - y0 >= 24.0 {
                ladders.push(Segment::new(Vec2::new(x, y0), Vec2::new(x, y1)));
            }
        }
    }
    ladders.extend(threads.iter().copied());

    for (i, w) in windows.iter().enumerate() {
        let r = &w.rect;
        if r.width() < 16.0 || r.height() < 8.0 || !r.intersects(screen) {
            continue;
        }
        let front = &windows[..i];
        // trechos de uma linha horizontal em `y` que nenhuma janela da frente cobre
        let visible = |x0: f32, x1: f32, y: f32| {
            let cuts: Vec<(f32, f32)> = front
                .iter()
                .filter(|o| y >= o.rect.min.y && y <= o.rect.max.y)
                .map(|o| (o.rect.min.x, o.rect.max.x))
                .collect();
            subtract_intervals((x0.max(screen.min.x), x1.min(screen.max.x)), &cuts)
        };
        let what = if w.is_panel() {
            Ground::Floor // topo de uma barra de tarefas: chao firme
        } else if w.is_element() {
            Ground::Element
        } else {
            Ground::WindowTop
        };
        // topo (so se sobrar altura para o heroi aparecer em cima)
        if r.min.y >= screen.min.y + 24.0 && r.min.y <= screen.max.y {
            for (x0, x1) in visible(r.min.x, r.max.x, r.min.y) {
                if x1 - x0 >= min_len && (w.is_panel() || !no_headroom(x0, x1, r.min.y)) {
                    platforms.push(Platform {
                        x0,
                        x1,
                        y: r.min.y,
                        owner: if w.is_panel() { 0 } else { w.id },
                        what,
                        solid: w.is_panel(),
                    });
                }
            }
        }
        // laterais visiveis: escadas (janelas de app; icones sao baixos demais para precisar)
        if !w.is_element() {
            for x in [r.min.x, r.max.x] {
                let cuts: Vec<(f32, f32)> = front
                    .iter()
                    .filter(|o| x >= o.rect.min.x && x <= o.rect.max.x)
                    .map(|o| (o.rect.min.y, o.rect.max.y))
                    .collect();
                if x < screen.min.x + 2.0 || x > screen.max.x - 2.0 {
                    continue; // encostada na borda: a parede da tela ja e escada
                }
                for (y0, y1) in subtract_intervals(
                    (r.min.y.max(screen.min.y), r.max.y.min(screen.max.y)),
                    &cuts,
                ) {
                    if y1 - y0 >= 24.0 {
                        ladders.push(Segment::new(Vec2::new(x, y0), Vec2::new(x, y1)));
                    }
                }
            }
        }
        // por dentro: o que a visao achou na imagem da janela
        let Some(seen) = inside(w.id) else { continue };
        let (iw, ih) = (seen.image_size.0 as f32, seen.image_size.1 as f32);
        if iw < 1.0 || ih < 1.0 {
            continue;
        }
        let (sx, sy) = (r.width() / iw, r.height() / ih);
        for l in &seen.ledges {
            let y = r.min.y + l.y * sy;
            // colado no topo da janela e redundante; colado na base nao cabe ninguem
            if y < r.min.y + 6.0 || y > r.max.y - 3.0 || y < screen.min.y + 24.0 {
                continue;
            }
            for (x0, x1) in visible(r.min.x + l.x0 * sx, r.min.x + l.x1 * sx, y) {
                if x1 - x0 >= min_len && !no_headroom(x0, x1, y) {
                    platforms.push(Platform {
                        x0,
                        x1,
                        y,
                        owner: w.id,
                        what: match l.kind {
                            LedgeKind::Text => Ground::Text,
                            LedgeKind::Line => Ground::Line,
                        },
                        solid: false,
                    });
                }
            }
        }
    }
    (platforms, ladders)
}

/// Teclas do jogador num quadro.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HeroInput {
    pub left: bool,
    pub right: bool,
    /// Cima: sobe escada; longe de escada, pula.
    pub up: bool,
    /// Baixo: desce de escada / atravessa o apoio para baixo.
    pub down: bool,
    pub jump: bool,
    pub run: bool,
    /// Andar devagar sem cair das pontas dos apoios (tem prioridade sobre correr).
    pub sneak: bool,
    pub attack: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct HeroEvents {
    pub jumped: bool,
    pub double_jumped: bool,
    /// Pousou neste apoio (vindo do ar).
    pub landed: Option<Platform>,
    pub swung: bool,
    pub grabbed_ladder: bool,
    pub respawned: bool,
}

#[derive(Debug, Clone)]
pub struct Hero {
    /// Pes (centro), em px locais do overlay, Y para baixo.
    pub pos: Vec2,
    pub vel: Vec2,
    /// +1 = olhando para a direita, -1 = esquerda.
    pub facing: f32,
    pub ground: Option<Platform>,
    /// X da escada em que esta agarrado.
    pub climbing: Option<f32>,
    pub hp: i32,
    pub invuln: f32,
    pub hurt_flash: f32,
    /// Sem controle (tranco de um golpe levado).
    stagger: f32,
    /// Tempo restante do golpe de espada em andamento.
    pub attack: f32,
    attack_cd: f32,
    /// Numero do golpe (cada golpe acerta cada bicho no maximo uma vez).
    pub swing_id: u32,
    /// Morto ha quanto tempo (reaparece em `respawn_secs`).
    pub dead_for: Option<f32>,
    coyote: f32,
    jump_buffer: f32,
    air_jumps: u8,
    prev: HeroInput,
    /// Fase do ciclo de passos/escalada (voltas), para a animacao.
    pub stride: f32,
    /// "Amassada" do pouso (1 = recem-pousado, decai a 0).
    pub squash: f32,
    pub placed: bool,
}

impl Hero {
    pub fn new(c: &HeroConfig) -> Self {
        Self {
            pos: Vec2::ZERO,
            vel: Vec2::ZERO,
            facing: 1.0,
            ground: None,
            climbing: None,
            hp: c.max_hp as i32,
            invuln: 0.0,
            hurt_flash: 0.0,
            stagger: 0.0,
            attack: 0.0,
            attack_cd: 0.0,
            swing_id: 0,
            dead_for: None,
            coyote: 0.0,
            jump_buffer: 0.0,
            air_jumps: 0,
            prev: HeroInput::default(),
            stride: 0.0,
            squash: 0.0,
            placed: false,
        }
    }

    pub fn alive(&self) -> bool {
        self.placed && self.dead_for.is_none()
    }

    pub fn half_width(&self, c: &HeroConfig) -> f32 {
        c.size_px * 0.28
    }

    /// Centro do corpo (alvo dos bichos, coleta).
    pub fn center(&self, c: &HeroConfig) -> Vec2 {
        self.pos - Vec2::new(0.0, c.size_px * 0.5)
    }

    /// Raio do corpo para contato.
    pub fn radius(&self, c: &HeroConfig) -> f32 {
        c.size_px * 0.36
    }

    /// Caixa do golpe de espada em andamento (a frente do heroi).
    pub fn attack_box(&self, c: &HeroConfig) -> Option<Rect> {
        (self.attack > 0.0 && self.alive()).then(|| {
            let (a, b) = (self.pos.x, self.pos.x + self.facing * c.attack_reach_px);
            Rect::new(
                Vec2::new(a.min(b) - 4.0, self.pos.y - c.size_px * 1.15),
                Vec2::new(a.max(b) + 4.0, self.pos.y + 8.0),
            )
        })
    }

    /// Coloca o heroi no meio do chao da tela.
    pub fn place(&mut self, bounds: &Rect, c: &HeroConfig) {
        self.pos = Vec2::new(bounds.center().x, bounds.max.y);
        self.vel = Vec2::ZERO;
        self.ground = None;
        self.climbing = None;
        self.hp = c.max_hp as i32;
        self.dead_for = None;
        self.placed = true;
    }

    /// Poe os pes no chao firme mais baixo (o chao da tela, ou o topo da barra de tarefas).
    fn stand_on_floor(&mut self, platforms: &[Platform]) {
        let x = self.pos.x;
        let floor = platforms
            .iter()
            .filter(|p| p.solid && p.holds(x, 0.0))
            .max_by(|a, b| a.y.total_cmp(&b.y))
            .or_else(|| {
                platforms.iter().filter(|p| p.solid).min_by(|a, b| {
                    ((a.x0 + a.x1) * 0.5 - x)
                        .abs()
                        .total_cmp(&((b.x0 + b.x1) * 0.5 - x).abs())
                })
            });
        if let Some(p) = floor {
            self.pos = Vec2::new(x.clamp(p.x0, p.x1), p.y);
            self.ground = Some(*p);
        }
    }

    /// Leva dano vindo de `from`. `false` se estava invulneravel/morto.
    pub fn hurt(&mut self, damage: i32, from: Vec2, c: &HeroConfig) -> bool {
        if !self.alive() || self.invuln > 0.0 {
            return false;
        }
        self.hp -= damage.max(0);
        self.invuln = c.invuln_secs;
        self.hurt_flash = 0.25;
        self.stagger = 0.28;
        self.attack = 0.0;
        self.climbing = None;
        self.ground = None;
        let away = if self.pos.x >= from.x { 1.0 } else { -1.0 };
        self.vel = Vec2::new(away * 300.0, -360.0);
        if self.hp <= 0 {
            self.hp = 0;
            self.dead_for = Some(0.0);
        }
        true
    }

    pub fn heal(&mut self, amount: i32, c: &HeroConfig) {
        self.hp = (self.hp + amount.max(0)).min(c.max_hp as i32);
    }

    /// Tira o heroi de dentro de blocos (barras do sistema), sempre para longe da borda da tela
    /// em que a barra encosta: a barra superior empurra para baixo, a de tarefas para cima, um
    /// dock para dentro da tela. Zera a velocidade naquela direcao.
    pub fn keep_out_of(&mut self, blocks: &[Rect], bounds: &Rect, c: &HeroConfig) {
        let half = self.half_width(c);
        for b in blocks {
            let body = Rect::new(
                Vec2::new(self.pos.x - half, self.pos.y - c.size_px),
                Vec2::new(self.pos.x + half, self.pos.y),
            );
            if !body.intersects(b) {
                continue;
            }
            let (top, bottom) = (b.min.y <= bounds.min.y + 1.0, b.max.y >= bounds.max.y - 1.0);
            let (left, right) = (b.min.x <= bounds.min.x + 1.0, b.max.x >= bounds.max.x - 1.0);
            let down = Vec2::new(0.0, b.max.y - body.min.y);
            let up = Vec2::new(0.0, b.min.y - body.max.y);
            let to_right = Vec2::new(b.max.x - body.min.x, 0.0);
            let to_left = Vec2::new(b.min.x - body.max.x, 0.0);
            let push = match (top, bottom) {
                // faixa de cima a baixo: dock (ou vao entre telas)
                (true, true) if left && !right => to_right,
                (true, true) if right && !left => to_left,
                (true, true) => {
                    if to_right.x.abs() < to_left.x.abs() {
                        to_right
                    } else {
                        to_left
                    }
                }
                (true, false) => down,
                (false, true) => up,
                _ => [down, up, to_right, to_left]
                    .into_iter()
                    .min_by(|a, b| a.length().total_cmp(&b.length()))
                    .unwrap_or(down),
            };
            self.pos += push;
            if push.y > 0.0 {
                self.vel.y = self.vel.y.max(0.0);
            } else if push.y < 0.0 {
                self.vel.y = self.vel.y.min(0.0);
            }
            if push.x != 0.0 {
                self.vel.x = 0.0;
                self.climbing = None;
            }
        }
    }

    /// Pisao: caindo em cima de um bicho (centro `target`, raio `r`)? Se sim, quica.
    pub fn try_stomp(&mut self, target: Vec2, r: f32, c: &HeroConfig) -> bool {
        let over = (self.pos.x - target.x).abs() <= r + self.half_width(c);
        let feet_on_top = self.pos.y >= target.y - r - 12.0 && self.pos.y <= target.y + r * 0.3;
        if !self.alive() || self.vel.y < 140.0 || !over || !feet_on_top {
            return false;
        }
        self.vel.y = -c.stomp_bounce;
        self.ground = None;
        self.air_jumps = 1;
        true
    }

    /// Um quadro: teclas -> movimento, com colisao nos apoios e escadas.
    pub fn step(
        &mut self,
        dt: f32,
        inp: HeroInput,
        platforms: &[Platform],
        ladders: &[Segment],
        bounds: &Rect,
        c: &HeroConfig,
    ) -> HeroEvents {
        let mut ev = HeroEvents::default();
        let dt = dt.clamp(0.0, 0.25);
        if !self.placed {
            self.place(bounds, c);
            self.stand_on_floor(platforms);
            ev.respawned = true;
            return ev;
        }
        if let Some(t) = self.dead_for {
            let t = t + dt;
            if t >= c.respawn_secs {
                self.place(bounds, c);
                self.stand_on_floor(platforms);
                self.invuln = 2.0;
                ev.respawned = true;
            } else {
                self.dead_for = Some(t);
            }
            self.prev = inp;
            return ev;
        }

        for timer in [
            &mut self.invuln,
            &mut self.hurt_flash,
            &mut self.stagger,
            &mut self.attack,
            &mut self.attack_cd,
            &mut self.jump_buffer,
        ] {
            *timer = (*timer - dt).max(0.0);
        }
        self.squash = (self.squash - dt * 5.0).max(0.0);
        let inp = if self.stagger > 0.0 {
            HeroInput::default()
        } else {
            inp
        };
        let pressed = |now: bool, before: bool| now && !before;
        let half = self.half_width(c);

        // golpe de espada
        if pressed(inp.attack, self.prev.attack) && self.attack_cd <= 0.0 {
            self.attack = SWING_SECS;
            self.attack_cd = c.attack_cooldown_secs.max(SWING_SECS);
            self.swing_id += 1;
            ev.swung = true;
        }

        let dir = (inp.right as i32 - inp.left as i32) as f32;
        if dir != 0.0 {
            self.facing = dir;
        }
        let near_ladder = |pos: Vec2| {
            ladders
                .iter()
                .filter(|l| {
                    (l.a.x - pos.x).abs() <= LADDER_REACH
                        && pos.y >= l.a.y.min(l.b.y) - 2.0
                        && pos.y <= l.a.y.max(l.b.y) + 6.0
                })
                .min_by(|a, b| (a.a.x - pos.x).abs().total_cmp(&(b.a.x - pos.x).abs()))
                .copied()
        };

        // cima (W) e pulo; no ar, segurando cima perto de uma escada, agarra e escala
        if self.climbing.is_none() {
            if pressed(inp.jump, self.prev.jump) || pressed(inp.up, self.prev.up) {
                self.jump_buffer = JUMP_BUFFER_SECS;
            }
            if inp.up
                && self.ground.is_none()
                && self.vel.y > GRAB_RISE
                && let Some(l) = near_ladder(self.pos)
                && self.pos.y > l.a.y.min(l.b.y) + 4.0
            {
                self.climbing = Some(l.a.x);
                self.vel = Vec2::ZERO;
                self.jump_buffer = 0.0;
                ev.grabbed_ladder = true;
            }
        }

        let steps = (dt / MAX_STEP).ceil().clamp(1.0, 40.0);
        let h = dt / steps;
        for _ in 0..steps as u32 {
            // --- na escada
            if let Some(lx) = self.climbing {
                let ladder = ladders.iter().find(|l| {
                    (l.a.x - lx).abs() < 2.0
                        && self.pos.y >= l.a.y.min(l.b.y) - 8.0
                        && self.pos.y <= l.a.y.max(l.b.y) + 8.0
                });
                let Some(l) = ladder else {
                    self.climbing = None;
                    continue;
                };
                let (top, bottom) = (l.a.y.min(l.b.y), l.a.y.max(l.b.y));
                if dir != 0.0 {
                    // solta para o lado; com cima junto, salta da parede
                    let leap = inp.up || inp.jump;
                    self.climbing = None;
                    self.vel = Vec2::new(
                        dir * c.walk_speed,
                        if leap { -c.jump_speed * 0.85 } else { 0.0 },
                    );
                    self.air_jumps = 1;
                    ev.jumped |= leap;
                    self.jump_buffer = 0.0;
                    continue;
                }
                let v = (inp.down as i32 - inp.up as i32) as f32 * c.climb_speed;
                self.pos.x = lx;
                self.pos.y += v * h;
                self.vel = Vec2::new(0.0, v);
                self.stride = (self.stride + v.abs() * h / (c.size_px * 0.9)).fract();
                if self.pos.y <= top {
                    // fim desta escada: se outra continua para cima logo ao lado (fio de seda ->
                    // lateral da janela), passa para ela; senao salta para o topo
                    let next = ladders.iter().find(|n| {
                        let (nt, nb) = (n.a.y.min(n.b.y), n.a.y.max(n.b.y));
                        (n.a.x - lx).abs() > 2.0
                            && (n.a.x - lx).abs() <= LADDER_HANDOVER
                            && nb >= top - 12.0
                            && nt < top - 20.0
                    });
                    match next {
                        Some(n) => {
                            self.climbing = Some(n.a.x);
                            self.pos = Vec2::new(n.a.x, top - 1.0);
                        }
                        None => {
                            self.pos.y = top - 1.0;
                            self.vel = Vec2::new(0.0, -160.0);
                            self.climbing = None;
                            self.air_jumps = 1;
                        }
                    }
                } else if self.pos.y >= bottom {
                    self.pos.y = bottom;
                    self.climbing = None;
                }
                continue;
            }

            // --- andando / no ar
            let grounded = self.ground.is_some();
            let speed = if inp.sneak {
                c.sneak_speed
            } else if inp.run {
                c.run_speed
            } else {
                c.walk_speed
            };
            let target = dir * speed;
            let accel = if grounded { 3200.0 } else { 1900.0 };
            if self.stagger <= 0.0 {
                let dv = target - self.vel.x;
                self.vel.x += dv.clamp(-accel * h, accel * h);
            }

            if self.jump_buffer > 0.0 {
                if grounded || self.coyote > 0.0 {
                    self.vel.y = -c.jump_speed;
                    self.ground = None;
                    self.coyote = 0.0;
                    self.jump_buffer = 0.0;
                    self.air_jumps = c.double_jump as u8;
                    ev.jumped = true;
                } else if self.air_jumps > 0 {
                    self.air_jumps -= 1;
                    self.vel.y = -c.jump_speed * 0.88;
                    self.jump_buffer = 0.0;
                    ev.double_jumped = true;
                }
            }
            // pulo curto: soltou a tecla subindo
            let holding = inp.jump || inp.up;
            if !holding && self.vel.y < -c.jump_speed * 0.42 && self.stagger <= 0.0 {
                self.vel.y = -c.jump_speed * 0.42;
            }
            // baixo no chao: atravessa o apoio (menos o chao da tela)
            if inp.down && self.ground.is_some_and(|g| !g.solid) {
                self.ground = None;
                self.pos.y += 3.0;
            }

            let prev_y = self.pos.y;
            let prev_x = self.pos.x;
            if self.ground.is_none() {
                self.vel.y = (self.vel.y + c.gravity * h).min(TERMINAL_SPEED);
                self.coyote = (self.coyote - h).max(0.0);
            }
            self.pos += self.vel * h;

            // devagar (Shift) nao cai da ponta: para exatamente onde andando ele comecaria a
            // cair (vaos pequenos, como entre palavras, continuam passaveis)
            if inp.sneak
                && let Some(g) = self.ground
                && !platforms
                    .iter()
                    .any(|p| (p.y - g.y).abs() <= 1.0 && p.holds(self.pos.x, half * 0.5))
            {
                self.pos.x = prev_x;
                self.vel.x = 0.0;
            }
            // paredes e teto da tela
            let (lo, hi) = (bounds.min.x + half, bounds.max.x - half);
            if self.pos.x < lo || self.pos.x > hi {
                self.pos.x = self.pos.x.clamp(lo, hi.max(lo));
                self.vel.x = 0.0;
            }
            let ceiling = bounds.min.y + c.size_px;
            if self.pos.y < ceiling {
                self.pos.y = ceiling;
                self.vel.y = self.vel.y.max(0.0);
            }

            if let Some(g) = self.ground {
                // continua em cima do mesmo apoio? (ele pode ter se mexido um pouco)
                let same = platforms
                    .iter()
                    .filter(|p| {
                        p.owner == g.owner
                            && p.what == g.what
                            && (p.y - g.y).abs() <= GROUND_FOLLOW
                            && p.holds(self.pos.x, half * 0.5)
                    })
                    .min_by(|a, b| (a.y - g.y).abs().total_cmp(&(b.y - g.y).abs()));
                match same {
                    Some(p) => {
                        self.pos.y = p.y;
                        self.vel.y = 0.0;
                        self.ground = Some(*p);
                    }
                    None => {
                        self.ground = None;
                        self.coyote = COYOTE_SECS;
                    }
                }
            } else if self.vel.y >= 0.0 {
                // caindo: pousa no primeiro apoio cruzado neste passo
                let hit = platforms
                    .iter()
                    .filter(|p| {
                        p.y >= prev_y - 0.5
                            && p.y <= self.pos.y
                            && p.holds(self.pos.x, half * 0.5)
                            && (p.solid || !inp.down)
                    })
                    .min_by(|a, b| a.y.total_cmp(&b.y));
                if let Some(p) = hit {
                    self.squash = (self.vel.y / 900.0).clamp(0.25, 1.0);
                    self.pos.y = p.y;
                    self.vel.y = 0.0;
                    self.ground = Some(*p);
                    self.air_jumps = c.double_jump as u8;
                    ev.landed = Some(*p);
                }
            }
            // rede de seguranca: nunca abaixo do chao da tela
            if self.pos.y > bounds.max.y {
                self.pos.y = bounds.max.y;
                self.vel.y = 0.0;
            }
            if self.ground.is_some() {
                self.stride = (self.stride + self.vel.x.abs() * h / (c.size_px * 1.1)).fract();
            }
        }
        self.prev = inp;
        ev
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PickupKind {
    Coin,
    /// Missao: fica longe, de preferencia dentro de um aplicativo.
    Star,
    Heart,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pickup {
    pub id: u32,
    pub kind: PickupKind,
    pub pos: Vec2,
    /// Apoio em que esta (para o painel dizer onde fica a estrela).
    pub on: Platform,
    pub age: f32,
}

/// Altura (px) em que os itens flutuam acima do apoio.
pub const PICKUP_HOVER: f32 = 18.0;
const PICKUP_REACH: f32 = 26.0;
const RECHECK_SECS: f32 = 1.5;

/// Itens espalhados pelos apoios: moedas, a estrela da missao e coracoes.
#[derive(Debug, Clone)]
pub struct Loot {
    pub items: Vec<Pickup>,
    pub coins: u32,
    pub stars: u32,
    next_id: u32,
    recheck: f32,
    rng: Rng,
}

impl Loot {
    pub fn new(seed: u64) -> Self {
        Self {
            items: Vec::new(),
            coins: 0,
            stars: 0,
            next_id: 1,
            recheck: 0.0,
            rng: Rng::new(seed ^ 0x10_07),
        }
    }

    pub fn star(&self) -> Option<&Pickup> {
        self.items.iter().find(|i| i.kind == PickupKind::Star)
    }

    /// Sorteia um ponto sobre um apoio (mais largos tem mais chance). Com `inside_first`, tenta
    /// primeiro os apoios de dentro dos aplicativos.
    fn spot(
        &mut self,
        platforms: &[Platform],
        bounds: &Rect,
        inside_first: bool,
    ) -> Option<(Vec2, Platform)> {
        let usable = |p: &&Platform| p.width() >= 30.0 && p.y >= bounds.min.y + 70.0;
        let mut pool: Vec<&Platform> = platforms
            .iter()
            .filter(usable)
            .filter(|p| !inside_first || p.what.is_inside_app())
            .collect();
        if pool.is_empty() {
            pool = platforms.iter().filter(usable).collect();
        }
        let total: f32 = pool.iter().map(|p| p.width().min(400.0)).sum();
        if total <= 0.0 {
            return None;
        }
        let mut pick = self.rng.range(0.0, total);
        let p = *pool
            .iter()
            .find(|p| {
                pick -= p.width().min(400.0);
                pick <= 0.0
            })
            .or(pool.last())?;
        let x = self.rng.range(p.x0 + 10.0, p.x1 - 10.0);
        Some((Vec2::new(x, p.y - PICKUP_HOVER), *p))
    }

    fn add(&mut self, kind: PickupKind, pos: Vec2, on: Platform) {
        self.items.push(Pickup {
            id: self.next_id,
            kind,
            pos,
            on,
            age: 0.0,
        });
        self.next_id += 1;
    }

    /// Um quadro: recolhe o que o heroi tocou (devolvido), repoe itens e tira os que ficaram sem
    /// apoio (janela fechada/rolada).
    pub fn update(
        &mut self,
        dt: f32,
        hero: &mut Hero,
        platforms: &[Platform],
        bounds: &Rect,
        c: &HeroConfig,
    ) -> Vec<Pickup> {
        for i in &mut self.items {
            i.age += dt;
        }
        let mut got = Vec::new();
        if hero.alive() {
            let center = hero.center(c);
            let reach = PICKUP_REACH + hero.radius(c) * 0.5;
            self.items.retain(|i| {
                // (coracao so com vida faltando)
                let wanted = i.kind != PickupKind::Heart || hero.hp < c.max_hp as i32;
                let take = wanted && i.age > 0.3 && i.pos.distance(center) <= reach;
                if take {
                    got.push(*i);
                }
                !take
            });
            for i in &got {
                match i.kind {
                    PickupKind::Coin => self.coins += 1,
                    PickupKind::Star => self.stars += 1,
                    PickupKind::Heart => hero.heal(1, c),
                }
            }
        }

        self.recheck -= dt;
        if self.recheck > 0.0 && got.is_empty() {
            return got;
        }
        self.recheck = RECHECK_SECS;
        // item cujo apoio sumiu vai embora (sera reposto em outro lugar)
        self.items.retain(|i| {
            platforms
                .iter()
                .any(|p| (p.y - (i.pos.y + PICKUP_HOVER)).abs() <= 4.0 && p.holds(i.pos.x, 0.0))
        });
        // a missao e dentro de um aplicativo sempre que der: se a estrela nasceu fora (as imagens
        // das janelas ainda nao tinham chegado) e agora ha apoios la dentro, ela se muda
        if platforms
            .iter()
            .any(|p| p.what.is_inside_app() && p.width() >= 30.0)
        {
            self.items
                .retain(|i| i.kind != PickupKind::Star || i.on.what.is_inside_app());
        }
        let count = |items: &[Pickup], k: PickupKind| items.iter().filter(|i| i.kind == k).count();
        for _ in count(&self.items, PickupKind::Coin)..c.coins as usize {
            if let Some((pos, on)) = self.spot(platforms, bounds, false) {
                self.add(PickupKind::Coin, pos, on);
            }
        }
        if count(&self.items, PickupKind::Star) == 0 {
            // a estrela e a missao: o candidato mais longe do heroi, de preferencia dentro de um app
            let far = (0..8)
                .filter_map(|_| self.spot(platforms, bounds, true))
                .max_by(|a, b| a.0.distance(hero.pos).total_cmp(&b.0.distance(hero.pos)));
            if let Some((pos, on)) = far {
                self.add(PickupKind::Star, pos, on);
            }
        }
        if hero.alive()
            && hero.hp < c.max_hp as i32
            && count(&self.items, PickupKind::Heart) == 0
            && self.rng.chance(0.2)
            && let Some((pos, on)) = self.spot(platforms, bounds, false)
        {
            self.add(PickupKind::Heart, pos, on);
        }
        got
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{ELEMENT_ID_BASE, WindowState};

    const DT: f32 = 1.0 / 60.0;

    fn cfg() -> HeroConfig {
        HeroConfig::default()
    }

    fn screen() -> Rect {
        Rect::from_xywh(0.0, 0.0, 1920.0, 1080.0)
    }

    fn floor() -> Platform {
        Platform {
            x0: 0.0,
            x1: 1920.0,
            y: 1080.0,
            owner: 0,
            what: Ground::Floor,
            solid: true,
        }
    }

    fn shelf(x0: f32, x1: f32, y: f32, owner: u64) -> Platform {
        Platform {
            x0,
            x1,
            y,
            owner,
            what: Ground::Text,
            solid: false,
        }
    }

    fn win(id: u64, x: f32, y: f32, w: f32, h: f32) -> DesktopWindow {
        let r = Rect::from_xywh(x, y, w, h);
        DesktopWindow {
            id,
            rect: r,
            raw_rect: r,
            state: WindowState::Normal,
            pid: 1,
            title: String::new(),
        }
    }

    /// Heroi ja em pe no chao, em `x`.
    fn hero_at(x: f32, world: &[Platform]) -> Hero {
        let c = cfg();
        let mut h = Hero::new(&c);
        h.step(DT, HeroInput::default(), world, &[], &screen(), &c);
        h.pos.x = x;
        run(&mut h, HeroInput::default(), 0.2, world, &[]);
        assert!(h.ground.is_some());
        h
    }

    fn run(h: &mut Hero, inp: HeroInput, secs: f32, world: &[Platform], ladders: &[Segment]) {
        let c = cfg();
        let mut t = 0.0;
        while t < secs {
            h.step(DT, inp, world, ladders, &screen(), &c);
            t += DT;
        }
    }

    fn key(f: impl Fn(&mut HeroInput)) -> HeroInput {
        let mut i = HeroInput::default();
        f(&mut i);
        i
    }

    #[test]
    fn walks_runs_and_stops_at_the_screen_edges() {
        let c = cfg();
        let world = [floor()];
        let mut h = hero_at(900.0, &world);
        run(&mut h, key(|i| i.right = true), 1.0, &world, &[]);
        let walked = h.pos.x - 900.0;
        assert!(
            (walked - c.walk_speed).abs() < c.walk_speed * 0.15,
            "{walked}"
        );
        assert_eq!(h.facing, 1.0);

        let mut r = hero_at(900.0, &world);
        run(
            &mut r,
            key(|i| (i.left, i.run) = (true, true)),
            1.0,
            &world,
            &[],
        );
        assert!(900.0 - r.pos.x > walked * 1.4, "correr e mais rapido");
        assert_eq!(r.facing, -1.0);

        run(
            &mut r,
            key(|i| (i.left, i.run) = (true, true)),
            6.0,
            &world,
            &[],
        );
        assert!(
            r.pos.x >= r.half_width(&c) - 0.01 && r.pos.x < 30.0,
            "parede"
        );
        assert_eq!(r.pos.y, 1080.0);
        run(&mut r, HeroInput::default(), 0.5, &world, &[]);
        assert!(r.vel.x.abs() < 1.0, "para ao soltar");
    }

    #[test]
    fn jump_height_depends_on_holding_and_double_jump_goes_higher() {
        let world = [floor()];
        let peak = |hold: f32, second: bool| {
            let mut h = hero_at(500.0, &world);
            let mut top = h.pos.y;
            let mut t = 0.0;
            let mut again = second;
            while t < 2.0 {
                let mut inp = key(|i| i.jump = t < hold);
                if again && h.vel.y > -50.0 && t > 0.1 {
                    // novo toque no auge
                    h.step(DT, HeroInput::default(), &world, &[], &screen(), &cfg());
                    inp.jump = true;
                    again = false;
                }
                h.step(DT, inp, &world, &[], &screen(), &cfg());
                top = top.min(h.pos.y);
                t += DT;
            }
            assert!(h.ground.is_some(), "voltou ao chao");
            1080.0 - top
        };
        let tap = peak(0.03, false);
        let full = peak(1.0, false);
        let double = peak(1.0, true);
        assert!(tap > 20.0 && tap < full * 0.7, "toque {tap} x cheio {full}");
        assert!((100.0..220.0).contains(&full), "{full}");
        assert!(double > full * 1.5, "duplo {double}");
    }

    #[test]
    fn lands_on_ledges_from_above_passes_through_from_below_and_drops_with_down() {
        // prateleira a 100 px do chao: da para subir pulando por baixo dela
        let world = [floor(), shelf(400.0, 700.0, 980.0, 7)];
        let mut h = hero_at(550.0, &world);
        let mut landed = None;
        for _ in 0..120 {
            let ev = h.step(DT, key(|i| i.jump = true), &world, &[], &screen(), &cfg());
            landed = landed.or(ev.landed);
        }
        let p = landed.expect("pousou");
        assert_eq!((p.y, p.owner, p.what), (980.0, 7, Ground::Text));
        assert_eq!(h.pos.y, 980.0);

        // anda ate a ponta e cai
        run(&mut h, key(|i| i.right = true), 1.5, &world, &[]);
        assert_eq!(h.pos.y, 1080.0);
        assert_eq!(h.ground.unwrap().what, Ground::Floor);

        // de volta em cima; "baixo" atravessa; no chao da tela nao atravessa
        h.pos = Vec2::new(550.0, 900.0);
        h.ground = None;
        run(&mut h, HeroInput::default(), 0.6, &world, &[]);
        assert_eq!(h.pos.y, 980.0);
        run(&mut h, key(|i| i.down = true), 0.6, &world, &[]);
        assert_eq!(h.pos.y, 1080.0);
        run(&mut h, key(|i| i.down = true), 0.5, &world, &[]);
        assert_eq!(h.pos.y, 1080.0);
    }

    #[test]
    fn a_fast_fall_never_tunnels_through_a_ledge_even_at_low_frame_rates() {
        let world = [floor(), shelf(0.0, 1920.0, 700.0, 3)];
        let c = cfg();
        let mut h = hero_at(500.0, &world);
        h.pos.y = 100.0 + c.size_px;
        h.ground = None;
        h.vel.y = 1600.0;
        for _ in 0..10 {
            h.step(0.1, HeroInput::default(), &world, &[], &screen(), &c); // 10 qps
        }
        assert_eq!(h.pos.y, 700.0);
    }

    #[test]
    fn rides_a_ledge_that_scrolls_a_little_and_falls_when_it_vanishes() {
        let mut world = vec![floor(), shelf(400.0, 700.0, 800.0, 9)];
        let mut h = hero_at(550.0, &world);
        h.pos.y = 790.0;
        h.ground = None;
        run(&mut h, HeroInput::default(), 0.3, &world, &[]);
        assert_eq!(h.pos.y, 800.0);
        world[1].y = 794.0; // a pagina rolou 6 px
        run(&mut h, HeroInput::default(), 0.1, &world, &[]);
        assert_eq!(h.pos.y, 794.0);
        world.pop(); // sumiu
        run(&mut h, HeroInput::default(), 1.0, &world, &[]);
        assert_eq!(h.pos.y, 1080.0);
    }

    #[test]
    fn climbs_a_ladder_with_up_and_hops_onto_the_top() {
        // lateral de uma janela: escada de y=400 a y=800; o topo da janela e um apoio
        let top = Platform {
            x0: 600.0,
            x1: 1100.0,
            y: 400.0,
            owner: 5,
            what: Ground::WindowTop,
            solid: false,
        };
        let world = [floor(), top];
        let ladders = [
            Segment::new(Vec2::new(600.0, 400.0), Vec2::new(600.0, 800.0)),
            // fio de seda da base da janela ate o chao
            Segment::new(Vec2::new(626.0, 800.0), Vec2::new(626.0, 1080.0)),
        ];
        let mut h = hero_at(630.0, &world);
        // W no chao e pulo; segurando, agarra o fio quando a subida perde forca
        let ev = h.step(
            DT,
            key(|i| i.up = true),
            &world,
            &ladders,
            &screen(),
            &cfg(),
        );
        assert!(!ev.grabbed_ladder && h.climbing.is_none());
        let mut grabbed_at = None;
        for _ in 0..60 {
            let ev = h.step(
                DT,
                key(|i| i.up = true),
                &world,
                &ladders,
                &screen(),
                &cfg(),
            );
            if ev.grabbed_ladder {
                grabbed_at = Some(h.pos.y);
                break;
            }
        }
        let y = grabbed_at.expect("agarrou sem precisar de tempo certo");
        assert!(y < 1000.0 && y > 900.0, "agarrou no alto do pulo: {y}");
        assert_eq!(h.climbing, Some(626.0));
        // parado na escada nao cai
        run(&mut h, HeroInput::default(), 0.5, &world, &ladders);
        assert_eq!(h.pos.y, y);
        // segurando "cima": sobe o fio, troca sozinho para a lateral da janela e chega ao topo
        let mut switched = false;
        for _ in 0..600 {
            h.step(
                DT,
                key(|i| i.up = true),
                &world,
                &ladders,
                &screen(),
                &cfg(),
            );
            switched |= h.climbing == Some(600.0);
            if h.ground.is_some() {
                break;
            }
        }
        assert!(switched, "passou do fio para a lateral");
        assert_eq!(
            h.ground.map(|g| g.what),
            Some(Ground::WindowTop),
            "{:?}",
            h.pos
        );
        assert_eq!(h.pos.y, 400.0);
    }

    #[test]
    fn a_short_tap_next_to_a_wall_is_just_a_jump_and_side_keys_let_go_of_the_ladder() {
        let world = [floor()];
        let ladders = [Segment::new(
            Vec2::new(300.0, 200.0),
            Vec2::new(300.0, 1080.0),
        )];
        // toque curto em W encostado na escada: pulinho, sem grudar
        let mut h = hero_at(305.0, &world);
        let ev = h.step(
            DT,
            key(|i| i.up = true),
            &world,
            &ladders,
            &screen(),
            &cfg(),
        );
        assert!(ev.jumped);
        run(&mut h, HeroInput::default(), 1.0, &world, &ladders);
        assert!(h.climbing.is_none());
        assert_eq!(h.pos.y, 1080.0);

        // segurando: escala; A/D solta; A/D + W salta da parede
        run(&mut h, key(|i| i.up = true), 1.0, &world, &ladders);
        assert!(h.climbing.is_some() && h.pos.y < 950.0);
        let ev = h.step(
            DT,
            key(|i| (i.right, i.up) = (true, true)),
            &world,
            &ladders,
            &screen(),
            &cfg(),
        );
        assert!(ev.jumped && h.climbing.is_none() && h.vel.y < -500.0);
        run(&mut h, key(|i| i.right = true), 1.5, &world, &ladders);
        assert_eq!(h.pos.y, 1080.0);
        assert!(h.pos.x > 320.0);
    }

    #[test]
    fn sneaking_is_slow_and_never_walks_off_a_ledge() {
        let c = cfg();
        // dois trechos na mesma altura com um vao de 4 px (palavras), e um vao grande depois
        let world = [
            floor(),
            shelf(400.0, 600.0, 800.0, 3),
            shelf(604.0, 700.0, 800.0, 3),
            shelf(760.0, 900.0, 800.0, 3),
        ];
        let mut h = hero_at(450.0, &world);
        h.pos.y = 780.0;
        h.ground = None;
        run(&mut h, HeroInput::default(), 0.3, &world, &[]);
        assert_eq!(h.pos.y, 800.0);
        let x0 = h.pos.x;
        run(
            &mut h,
            key(|i| (i.right, i.sneak, i.run) = (true, true, true)),
            0.5,
            &world,
            &[],
        );
        let slow = h.pos.x - x0;
        assert!(
            slow < c.walk_speed * 0.5 * 0.6,
            "devagar mesmo com Ctrl: {slow}"
        );
        // segura Shift+D por muito tempo: para na ponta do trecho alcancavel, nao cai
        run(
            &mut h,
            key(|i| (i.right, i.sneak) = (true, true)),
            6.0,
            &world,
            &[],
        );
        assert_eq!(h.pos.y, 800.0);
        let edge = 700.0 + h.half_width(&c) * 0.5;
        assert!(
            h.pos.x <= edge && h.pos.x > 690.0,
            "parou na ponta: {}",
            h.pos.x
        );
        // soltar o Shift: cai do vao
        run(&mut h, key(|i| i.right = true), 0.6, &world, &[]);
        assert!(h.pos.y > 800.0);
        // S continua descendo de proposito, mesmo devagar
        let mut d = hero_at(450.0, &world);
        d.pos.y = 780.0;
        d.ground = None;
        run(&mut d, HeroInput::default(), 0.3, &world, &[]);
        run(
            &mut d,
            key(|i| (i.down, i.sneak) = (true, true)),
            0.8,
            &world,
            &[],
        );
        assert_eq!(d.pos.y, 1080.0);
    }

    #[test]
    fn sword_swing_has_a_box_in_front_and_a_cooldown() {
        let c = cfg();
        let world = [floor()];
        let mut h = hero_at(500.0, &world);
        assert!(h.attack_box(&c).is_none());
        let ev = h.step(DT, key(|i| i.attack = true), &world, &[], &screen(), &c);
        assert!(ev.swung);
        let b = h.attack_box(&c).unwrap();
        assert!(b.contains(Vec2::new(500.0 + c.attack_reach_px - 5.0, 1050.0)));
        assert!(!b.contains(Vec2::new(440.0, 1050.0)), "nao acerta atras");
        // segurar a tecla nao repete; soltar e apertar durante a recarga tambem nao
        let id = h.swing_id;
        run(&mut h, key(|i| i.attack = true), 0.1, &world, &[]);
        h.step(DT, HeroInput::default(), &world, &[], &screen(), &c);
        h.step(DT, key(|i| i.attack = true), &world, &[], &screen(), &c);
        assert_eq!(h.swing_id, id);
        run(
            &mut h,
            HeroInput::default(),
            c.attack_cooldown_secs + 0.1,
            &world,
            &[],
        );
        assert!(h.attack_box(&c).is_none(), "golpe acabou");
        h.step(DT, key(|i| i.attack = true), &world, &[], &screen(), &c);
        assert_eq!(h.swing_id, id + 1);
        // virado para a esquerda, a caixa vai para a esquerda
        let mut l = hero_at(500.0, &world);
        run(&mut l, key(|i| i.left = true), 0.05, &world, &[]);
        l.step(DT, key(|i| i.attack = true), &world, &[], &screen(), &c);
        assert!(
            l.attack_box(&c)
                .unwrap()
                .contains(Vec2::new(l.pos.x - 40.0, 1050.0))
        );
    }

    #[test]
    fn getting_hurt_knocks_back_gives_mercy_time_and_death_respawns_with_full_health() {
        let c = cfg();
        let world = [floor()];
        let mut h = hero_at(800.0, &world);
        assert!(h.hurt(1, Vec2::new(830.0, 1060.0), &c));
        assert_eq!(h.hp, c.max_hp as i32 - 1);
        assert!(
            h.vel.x < 0.0 && h.vel.y < 0.0,
            "arremessado para longe do bicho"
        );
        assert!(
            !h.hurt(1, Vec2::new(830.0, 1060.0), &c),
            "invulneravel por um tempo"
        );
        // durante o tranco as teclas nao mandam
        h.step(DT, key(|i| i.right = true), &world, &[], &screen(), &c);
        assert!(h.vel.x < 0.0);
        run(
            &mut h,
            HeroInput::default(),
            c.invuln_secs + 0.2,
            &world,
            &[],
        );
        assert!(h.ground.is_some());

        h.hp = 1;
        assert!(h.hurt(3, Vec2::ZERO, &c));
        assert!(!h.alive() && h.hp == 0);
        assert!(h.attack_box(&c).is_none());
        run(
            &mut h,
            key(|i| i.right = true),
            c.respawn_secs - 0.3,
            &world,
            &[],
        );
        assert!(!h.alive());
        let mut respawned = false;
        for _ in 0..60 {
            respawned |= h
                .step(DT, HeroInput::default(), &world, &[], &screen(), &c)
                .respawned;
        }
        assert!(respawned && h.alive());
        assert_eq!(h.hp, c.max_hp as i32);
        assert!(h.invuln > 0.0);
    }

    #[test]
    fn system_bars_are_solid_the_top_bar_is_a_ceiling_and_a_taskbar_is_a_floor() {
        use crate::platform::panel_windows;
        let c = cfg();
        // barra superior de 29 px; barra de tarefas de 40 px embaixo
        let work = [Rect::from_xywh(0.0, 29.0, 1920.0, 1011.0)];
        let ws = panel_windows(screen(), &work);
        assert_eq!(ws.len(), 2);
        let (p, l) = build_world(&screen(), &ws, &|_| None, &[], 24.0);
        assert!(
            p.iter().all(|p| p.y != 1080.0),
            "chao da tela esta atras da barra"
        );
        let floor: Vec<_> = p.iter().filter(|p| p.what == Ground::Floor).collect();
        assert_eq!(floor.len(), 1);
        assert_eq!((floor[0].y, floor[0].solid), (1040.0, true));
        assert!(
            l.iter()
                .all(|s| s.a.y.min(s.b.y) >= 29.0 && s.a.y.max(s.b.y) <= 1040.0)
        );

        let blocks: Vec<Rect> = ws.iter().map(|w| w.rect).collect();
        let mut h = Hero::new(&c);
        h.step(DT, HeroInput::default(), &p, &l, &screen(), &c);
        run(&mut h, HeroInput::default(), 0.5, &p, &l);
        assert_eq!(h.pos.y, 1040.0, "em pe na barra de tarefas");
        // pulo enorme: bate a cabeca na barra de cima
        h.vel.y = -5000.0;
        h.ground = None;
        let mut top = h.pos.y;
        for _ in 0..30 {
            h.step(DT, HeroInput::default(), &p, &l, &screen(), &c);
            h.keep_out_of(&blocks, &screen(), &c);
            top = top.min(h.pos.y - c.size_px);
        }
        assert!(top >= 29.0 - 0.01, "cabeca nunca entra na barra: {top}");

        // escalando a parede da tela ate o fim (que agora para na barra): nunca passa por cima
        let mut h = Hero::new(&c);
        h.step(DT, HeroInput::default(), &p, &l, &screen(), &c);
        h.pos.x = 12.0;
        run(&mut h, HeroInput::default(), 0.2, &p, &l);
        let mut climbed = false;
        for _ in 0..400 {
            h.step(DT, key(|i| i.up = true), &p, &l, &screen(), &c);
            h.keep_out_of(&blocks, &screen(), &c);
            climbed |= h.climbing.is_some();
            assert!(
                h.pos.y - c.size_px >= 29.0 - 0.01,
                "cabeca na barra: {:?}",
                h.pos
            );
            assert!(h.pos.x < 100.0, "nao pula de lado: {:?}", h.pos);
        }
        assert!(climbed);
    }

    #[test]
    fn stomping_needs_a_fall_onto_the_target_and_bounces() {
        let c = cfg();
        let world = [floor()];
        let mob = Vec2::new(600.0, 1060.0);
        let mut h = hero_at(600.0, &world);
        assert!(!h.try_stomp(mob, 20.0, &c), "parado ao lado nao e pisao");
        h.pos = Vec2::new(606.0, 1035.0);
        h.ground = None;
        h.vel = Vec2::new(0.0, 500.0);
        assert!(h.try_stomp(mob, 20.0, &c));
        assert!(h.vel.y < -300.0, "quicou");
        h.pos = Vec2::new(700.0, 1035.0);
        h.vel.y = 500.0;
        assert!(!h.try_stomp(mob, 20.0, &c), "longe na horizontal");
    }

    #[test]
    fn world_has_window_tops_side_ladders_and_inside_ledges_clipped_by_windows_in_front() {
        // janela 1 na frente cobre a metade direita da janela 2
        let ws = [
            win(1, 1000.0, 200.0, 600.0, 600.0),
            win(2, 500.0, 300.0, 800.0, 500.0),
            win(ELEMENT_ID_BASE + 3, 100.0, 900.0, 89.0, 76.0),
        ];
        // dentro da janela 2 (imagem 400x250, metade da escala): texto e uma borda
        let inside = |id: u64| {
            (id == 2).then(|| WindowLedges {
                ledges: vec![
                    Ledge {
                        x0: 20.0,
                        x1: 380.0,
                        y: 50.0,
                        kind: LedgeKind::Text,
                    },
                    Ledge {
                        x0: 300.0,
                        x1: 390.0,
                        y: 100.0,
                        kind: LedgeKind::Line,
                    },
                    Ledge {
                        x0: 20.0,
                        x1: 200.0,
                        y: 1.0,
                        kind: LedgeKind::Line,
                    }, // colado no topo
                ],
                image_size: (400, 250),
            })
        };
        let thread = Segment::new(Vec2::new(526.0, 800.0), Vec2::new(526.0, 1080.0));
        let (p, l) = build_world(&screen(), &ws, &inside, &[thread], 24.0);

        let find = |what: Ground, owner: u64| {
            p.iter()
                .filter(|p| p.what == what && p.owner == owner)
                .copied()
                .collect::<Vec<_>>()
        };
        assert_eq!(p[0].what, Ground::Floor);
        assert!(p[0].solid && p.iter().skip(1).all(|p| !p.solid));
        assert_eq!(
            find(Ground::WindowTop, 1),
            vec![Platform {
                x0: 1000.0,
                x1: 1600.0,
                y: 200.0,
                owner: 1,
                what: Ground::WindowTop,
                solid: false
            }]
        );
        // topo da janela 2 so ate onde a janela 1 comeca
        let top2 = find(Ground::WindowTop, 2);
        assert_eq!((top2[0].x0, top2[0].x1, top2[0].y), (500.0, 1000.0, 300.0));
        assert_eq!(find(Ground::Element, ELEMENT_ID_BASE + 3).len(), 1);
        // texto: x 20..380 da imagem -> 540..1260 na tela, cortado em 1000; y 50 -> 400
        let text = find(Ground::Text, 2);
        assert_eq!((text[0].x0, text[0].x1, text[0].y), (540.0, 1000.0, 400.0));
        // a borda (x 1100..1280) fica toda atras da janela 1; a colada no topo e descartada
        assert!(find(Ground::Line, 2).is_empty());

        // escadas: 2 paredes + fio + laterais (a direita da janela 2 esta coberta)
        let at = |x: f32| l.iter().filter(|s| (s.a.x - x).abs() < 0.5).count();
        assert_eq!((at(6.0), at(1914.0), at(526.0)), (1, 1, 1));
        assert_eq!((at(1000.0), at(1600.0), at(500.0)), (1, 1, 1));
        assert_eq!(at(1300.0), 0);
        assert_eq!(at(100.0) + at(189.0), 0, "icone nao vira escada");
    }

    #[test]
    fn loot_keeps_coins_and_a_far_star_and_the_hero_collects_them() {
        let c = cfg();
        let world = [
            floor(),
            shelf(1200.0, 1700.0, 600.0, 4),
            shelf(200.0, 500.0, 900.0, 4),
        ];
        let mut h = hero_at(300.0, &world);
        let mut loot = Loot::new(5);
        assert!(loot.update(DT, &mut h, &world, &screen(), &c).is_empty());
        let coins = loot
            .items
            .iter()
            .filter(|i| i.kind == PickupKind::Coin)
            .count();
        assert_eq!(coins, c.coins as usize);
        let star = *loot.star().expect("uma estrela");
        assert!(
            star.on.what.is_inside_app(),
            "missao dentro de um aplicativo"
        );
        for i in &loot.items {
            assert!(
                world
                    .iter()
                    .any(|p| (p.y - i.pos.y - PICKUP_HOVER).abs() < 0.01 && p.holds(i.pos.x, 0.0))
            );
        }

        // encosta numa moeda
        let coin = *loot
            .items
            .iter()
            .find(|i| i.kind == PickupKind::Coin)
            .unwrap();
        h.pos = coin.pos + Vec2::new(0.0, PICKUP_HOVER);
        let got = loot.update(0.5, &mut h, &world, &screen(), &c);
        assert_eq!(got.len(), 1);
        assert_eq!(loot.coins, 1);
        assert_eq!(
            loot.items
                .iter()
                .filter(|i| i.kind == PickupKind::Coin)
                .count(),
            c.coins as usize,
            "reposta"
        );

        // pega a estrela: conta e nasce outra
        h.pos = star.pos + Vec2::new(0.0, PICKUP_HOVER);
        let got = loot.update(0.5, &mut h, &world, &screen(), &c);
        assert!(got.iter().any(|i| i.kind == PickupKind::Star));
        assert_eq!(loot.stars, 1);
        assert!(loot.star().is_some_and(|s| s.id != star.id));
    }

    #[test]
    fn loot_on_a_ledge_that_disappears_is_moved_elsewhere() {
        let c = cfg();
        let mut world = vec![floor(), shelf(1200.0, 1700.0, 600.0, 4)];
        let mut h = hero_at(300.0, &world);
        let mut loot = Loot::new(8);
        loot.update(DT, &mut h, &world, &screen(), &c);
        assert!(loot.items.iter().any(|i| i.on.owner == 4));
        world.pop(); // janela fechada
        loot.update(2.0, &mut h, &world, &screen(), &c);
        assert!(
            loot.items.iter().all(|i| i.on.what == Ground::Floor),
            "{:?}",
            loot.items
        );
        assert!(loot.star().is_some());
    }

    #[test]
    fn star_moves_inside_an_app_as_soon_as_one_can_be_seen() {
        let c = cfg();
        let mut world = vec![floor()];
        let mut h = hero_at(300.0, &world);
        let mut loot = Loot::new(3);
        loot.update(DT, &mut h, &world, &screen(), &c);
        assert_eq!(loot.star().unwrap().on.what, Ground::Floor, "so ha o chao");
        world.push(shelf(900.0, 1500.0, 500.0, 6)); // chegou a imagem de uma janela
        loot.update(2.0, &mut h, &world, &screen(), &c);
        assert_eq!(loot.star().unwrap().on.what, Ground::Text);
    }

    #[test]
    fn hearts_only_appear_and_heal_when_hurt() {
        let c = cfg();
        let world = [floor()];
        let mut h = hero_at(300.0, &world);
        let mut loot = Loot::new(2);
        for _ in 0..60 {
            loot.update(2.0, &mut h, &world, &screen(), &c);
        }
        assert!(loot.items.iter().all(|i| i.kind != PickupKind::Heart));
        h.hp = 2;
        let mut healed = false;
        for _ in 0..200 {
            loot.update(2.0, &mut h, &world, &screen(), &c);
            if let Some(heart) = loot
                .items
                .iter()
                .find(|i| i.kind == PickupKind::Heart)
                .copied()
            {
                h.pos = heart.pos + Vec2::new(0.0, PICKUP_HOVER);
                loot.update(0.5, &mut h, &world, &screen(), &c);
                healed = h.hp == 3;
                break;
            }
        }
        assert!(healed);
    }
}
