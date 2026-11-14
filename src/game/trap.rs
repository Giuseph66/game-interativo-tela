//! Teia que prende o cursor (logica pura, sem Bevy/SO).
//!
//! Quando o mouse fica parado por `web.idle_secs`, um mob "caca" o cursor. Dois comportamentos:
//!
//! * **Constroi e leva** (`BuildFirst`): o mob vai a um ponto proximo, tece a teia, volta ate o
//!   cursor, agarra e o arrasta ate a teia, onde ele fica preso.
//! * **Leva e embrulha** (`Wrap`): o mob agarra o cursor, arrasta ate o ponto e tece a teia sobre ele.
//!
//! Preso, o cursor so se move dentro da teia; ela sai com golpes de espada (vida `web.web_hp`).
//! Seguranca: a teia se desfaz sozinha apos `web.max_trap_secs`, e o atalho de emergencia
//! encerra o app. O teclado nunca e afetado.

use bevy::math::Vec2;

use crate::config::WebConfig;
use crate::navigation::geometry::Rect;
use crate::rng::Rng;

/// Distancia (px) que o cursor precisa andar para o usuario "escapar" antes de ser agarrado.
pub const ESCAPE_DIST: f32 = 30.0;
/// Fracao do raio da teia em que o cursor preso fica contido.
pub const CONFINE_FRACTION: f32 = 0.8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebMode {
    BuildFirst,
    Wrap,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrapPhase {
    /// (BuildFirst) indo ao ponto da teia.
    GoToSpot,
    /// Tecendo; `progress` 0..1.
    Building { progress: f32 },
    /// (BuildFirst) teia pronta, indo buscar o cursor.
    GoToCursor,
    /// Arrastando o cursor ate o ponto.
    Dragging,
    /// Cursor preso; `elapsed` desde a prisao.
    Trapped { elapsed: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrapEnd {
    /// O usuario mexeu o mouse antes de ser agarrado.
    Escaped,
    /// O mob cacador morreu/sumiu antes de prender.
    HunterLost,
    /// Teia destruida pelo jogador.
    Destroyed,
    /// Tempo maximo de seguranca atingido.
    Timeout,
}

/// Entradas de um tick.
#[derive(Debug, Clone, Copy)]
pub struct TrapInput {
    pub dt: f32,
    pub hunter_pos: Vec2,
    pub hunter_alive: bool,
    pub cursor: Option<Vec2>,
}

/// O que o mundo deve fazer neste tick.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TrapOutput {
    /// Para onde o cacador anda, com que velocidade (px/s). `None` = parado.
    pub hunter_move: Option<(Vec2, f32)>,
    /// Posicao para onde o cursor deve ser levado agora.
    pub warp_cursor: Option<Vec2>,
    /// Fim da armadilha neste tick.
    pub ended: Option<TrapEnd>,
    /// O cursor acabou de ficar preso.
    pub trapped_now: bool,
}

#[derive(Debug, Clone)]
pub struct WebTrap {
    pub mode: WebMode,
    pub phase: TrapPhase,
    /// Centro da teia (px locais do overlay).
    pub spot: Vec2,
    /// Onde o cursor estava quando a caca comecou.
    pub anchor: Vec2,
    pub radius: f32,
    pub hp: f32,
    pub max_hp: f32,
}

impl WebTrap {
    pub fn new(mode: WebMode, anchor: Vec2, spot: Vec2, cfg: &WebConfig) -> Self {
        Self {
            mode,
            phase: match mode {
                WebMode::BuildFirst => TrapPhase::GoToSpot,
                WebMode::Wrap => TrapPhase::GoToCursor,
            },
            spot,
            anchor,
            radius: cfg.radius_px,
            hp: cfg.web_hp,
            max_hp: cfg.web_hp,
        }
    }

    pub fn is_trapped(&self) -> bool {
        matches!(self.phase, TrapPhase::Trapped { .. })
    }

    /// O cursor ja foi agarrado (o usuario nao consegue mais fugir sem destruir a teia).
    pub fn has_cursor(&self) -> bool {
        match self.phase {
            TrapPhase::Dragging | TrapPhase::Trapped { .. } => true,
            TrapPhase::Building { .. } => self.mode == WebMode::Wrap,
            _ => false,
        }
    }

    /// Quanto da teia esta visivel (0..1): cresce ao tecer, fica cheia depois.
    pub fn built(&self) -> f32 {
        match (self.mode, self.phase) {
            (_, TrapPhase::Building { progress }) => progress,
            (WebMode::BuildFirst, TrapPhase::GoToSpot) => 0.0,
            (WebMode::BuildFirst, _) => 1.0,
            (WebMode::Wrap, TrapPhase::Trapped { .. }) => 1.0,
            (WebMode::Wrap, _) => 0.0,
        }
    }

    /// Clique `p` acerta a teia (so quando o cursor esta preso)?
    pub fn contains(&self, p: Vec2) -> bool {
        self.is_trapped() && p.distance(self.spot) <= self.radius
    }

    pub fn update(&mut self, inp: TrapInput, cfg: &WebConfig) -> TrapOutput {
        let dt = inp.dt.clamp(0.0, 0.25);
        let mut out = TrapOutput::default();
        let arrived = |target: Vec2| inp.hunter_pos.distance(target) <= 3.0;

        // antes de prender: cacador morto cancela; mouse mexido (antes de ser agarrado) cancela
        if !self.has_cursor() && !self.is_trapped() {
            if !inp.hunter_alive {
                out.ended = Some(TrapEnd::HunterLost);
                return out;
            }
            if inp
                .cursor
                .is_some_and(|c| c.distance(self.anchor) > ESCAPE_DIST)
            {
                out.ended = Some(TrapEnd::Escaped);
                return out;
            }
        }
        // agarrado mas ainda nao preso: se o cacador morre, o cursor e solto
        if self.has_cursor() && !self.is_trapped() && !inp.hunter_alive {
            out.ended = Some(TrapEnd::HunterLost);
            return out;
        }

        match self.phase {
            TrapPhase::GoToSpot => {
                out.hunter_move = Some((self.spot, cfg.hunt_speed));
                if arrived(self.spot) {
                    self.phase = TrapPhase::Building { progress: 0.0 };
                }
            }
            TrapPhase::Building { progress } => {
                let p = (progress + dt / cfg.build_secs.max(0.05)).min(1.0);
                self.phase = TrapPhase::Building { progress: p };
                // circula pela teia enquanto tece
                let angle = p * std::f32::consts::TAU * 2.0;
                let orbit = self.spot + Vec2::from_angle(angle) * self.radius * 0.6 * p;
                out.hunter_move = Some((orbit, cfg.hunt_speed));
                if self.mode == WebMode::Wrap {
                    out.warp_cursor = Some(self.spot); // segura o cursor no lugar enquanto embrulha
                }
                if p >= 1.0 {
                    match self.mode {
                        WebMode::BuildFirst => self.phase = TrapPhase::GoToCursor,
                        WebMode::Wrap => {
                            self.phase = TrapPhase::Trapped { elapsed: 0.0 };
                            out.trapped_now = true;
                        }
                    }
                }
            }
            TrapPhase::GoToCursor => {
                out.hunter_move = Some((self.anchor, cfg.hunt_speed));
                if arrived(self.anchor) {
                    self.phase = TrapPhase::Dragging;
                }
            }
            TrapPhase::Dragging => {
                // o cursor vai grudado no cacador
                out.hunter_move = Some((self.spot, cfg.drag_speed));
                out.warp_cursor = Some(inp.hunter_pos);
                if arrived(self.spot) {
                    out.warp_cursor = Some(self.spot);
                    match self.mode {
                        WebMode::BuildFirst => {
                            self.phase = TrapPhase::Trapped { elapsed: 0.0 };
                            out.trapped_now = true;
                        }
                        WebMode::Wrap => self.phase = TrapPhase::Building { progress: 0.0 },
                    }
                }
            }
            TrapPhase::Trapped { elapsed } => {
                let elapsed = elapsed + dt;
                self.phase = TrapPhase::Trapped { elapsed };
                if elapsed >= cfg.max_trap_secs {
                    out.ended = Some(TrapEnd::Timeout);
                    return out;
                }
                // mantem o cursor dentro da teia
                if let Some(c) = inp.cursor {
                    out.warp_cursor = confine(c, self.spot, self.radius * CONFINE_FRACTION);
                }
            }
        }
        out
    }

    /// Golpe na teia. Devolve `true` se ela foi destruida.
    pub fn damage(&mut self, amount: f32) -> bool {
        self.hp = (self.hp - amount.max(0.0)).max(0.0);
        self.hp <= 0.0
    }
}

/// Se `p` saiu do circulo (centro, raio), devolve o ponto da borda mais proximo.
pub fn confine(p: Vec2, center: Vec2, radius: f32) -> Option<Vec2> {
    let d = p - center;
    (d.length() > radius).then(|| center + d.normalize_or_zero() * radius)
}

/// Escolhe o ponto da teia a ~`spot_distance_px` do cursor, totalmente dentro da tela.
pub fn choose_spot(cursor: Vec2, screen: &Rect, cfg: &WebConfig, rng: &mut Rng) -> Vec2 {
    let margin = cfg.radius_px + 8.0;
    let lo = screen.min + Vec2::splat(margin);
    let hi = (screen.max - Vec2::splat(margin)).max(lo);
    let mut best = cursor.clamp(lo, hi);
    let mut best_err = f32::MAX;
    for _ in 0..12 {
        let dir = Vec2::from_angle(rng.range(0.0, std::f32::consts::TAU));
        let p = (cursor + dir * cfg.spot_distance_px).clamp(lo, hi);
        let err = (p.distance(cursor) - cfg.spot_distance_px).abs();
        if err < best_err {
            best = p;
            best_err = err;
        }
    }
    best
}

/// Modo pelo texto do TOML.
pub fn pick_mode(setting: &str, rng: &mut Rng) -> WebMode {
    match setting {
        "build_first" => WebMode::BuildFirst,
        "wrap" => WebMode::Wrap,
        _ => {
            if rng.chance(0.5) {
                WebMode::BuildFirst
            } else {
                WebMode::Wrap
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn cfg() -> WebConfig {
        WebConfig::default()
    }

    /// Simula o mundo: o cacador anda ate o alvo pedido e o cursor obedece aos "warps".
    fn run(
        trap: &mut WebTrap,
        hunter: &mut Vec2,
        cursor: &mut Vec2,
        secs: f32,
        alive: bool,
    ) -> Vec<TrapOutput> {
        let c = cfg();
        let mut outs = Vec::new();
        let mut t = 0.0;
        while t < secs {
            let out = trap.update(
                TrapInput {
                    dt: DT,
                    hunter_pos: *hunter,
                    hunter_alive: alive,
                    cursor: Some(*cursor),
                },
                &c,
            );
            if let Some((to, speed)) = out.hunter_move {
                let d = to - *hunter;
                let step = (speed * DT).min(d.length());
                *hunter += d.normalize_or_zero() * step;
            }
            if let Some(w) = out.warp_cursor {
                *cursor = w;
            }
            outs.push(out);
            t += DT;
            if out.ended.is_some() {
                break;
            }
        }
        outs
    }

    #[test]
    fn build_first_builds_then_drags_cursor_into_web() {
        let c = cfg();
        let (anchor, spot) = (Vec2::new(500.0, 500.0), Vec2::new(760.0, 500.0));
        let mut trap = WebTrap::new(WebMode::BuildFirst, anchor, spot, &c);
        let mut hunter = Vec2::new(900.0, 900.0);
        let mut cursor = anchor;
        // avanca um quadro por vez, registrando cada fase nova
        let mut phases = vec![std::mem::discriminant(&trap.phase)];
        for _ in 0..(60 * 30) {
            run(&mut trap, &mut hunter, &mut cursor, DT * 0.5, true);
            let tag = std::mem::discriminant(&trap.phase);
            if phases.last() != Some(&tag) {
                phases.push(tag);
            }
            if trap.is_trapped() {
                break;
            }
        }
        use std::mem::discriminant as d;
        assert_eq!(
            phases,
            vec![
                d(&TrapPhase::GoToSpot),
                d(&TrapPhase::Building { progress: 0.0 }),
                d(&TrapPhase::GoToCursor),
                d(&TrapPhase::Dragging),
                d(&TrapPhase::Trapped { elapsed: 0.0 }),
            ]
        );
        assert!(
            cursor.distance(spot) < 4.0,
            "cursor levado ate a teia: {cursor}"
        );
        assert_eq!(trap.built(), 1.0);
    }

    #[test]
    fn wrap_drags_cursor_first_then_builds_over_it() {
        let c = cfg();
        let (anchor, spot) = (Vec2::new(300.0, 300.0), Vec2::new(300.0, 560.0));
        let mut trap = WebTrap::new(WebMode::Wrap, anchor, spot, &c);
        let mut hunter = Vec2::new(300.0, 900.0);
        let mut cursor = anchor;
        let mut saw_building_with_cursor_held = false;
        let outs = run(&mut trap, &mut hunter, &mut cursor, 30.0, true);
        assert!(outs.iter().any(|o| o.trapped_now));
        for o in &outs {
            if o.warp_cursor == Some(spot) {
                saw_building_with_cursor_held = true;
            }
        }
        assert!(trap.is_trapped());
        assert!(
            saw_building_with_cursor_held,
            "cursor segurado enquanto tece"
        );
        assert!(cursor.distance(spot) < 4.0);
    }

    #[test]
    fn moving_the_mouse_before_capture_escapes() {
        let c = cfg();
        let anchor = Vec2::new(500.0, 500.0);
        let mut trap = WebTrap::new(WebMode::BuildFirst, anchor, Vec2::new(700.0, 500.0), &c);
        let out = trap.update(
            TrapInput {
                dt: DT,
                hunter_pos: Vec2::ZERO,
                hunter_alive: true,
                cursor: Some(anchor + Vec2::new(80.0, 0.0)),
            },
            &c,
        );
        assert_eq!(out.ended, Some(TrapEnd::Escaped));
    }

    #[test]
    fn hunter_death_cancels_and_frees_cursor_while_dragging() {
        let c = cfg();
        let anchor = Vec2::new(500.0, 500.0);
        let mut trap = WebTrap::new(WebMode::Wrap, anchor, Vec2::new(700.0, 500.0), &c);
        trap.phase = TrapPhase::Dragging;
        let out = trap.update(
            TrapInput {
                dt: DT,
                hunter_pos: anchor,
                hunter_alive: false,
                cursor: Some(anchor),
            },
            &c,
        );
        assert_eq!(out.ended, Some(TrapEnd::HunterLost));
        assert_eq!(out.warp_cursor, None, "nao arrasta mais");
    }

    #[test]
    fn trapped_cursor_is_confined_and_timeout_releases() {
        let c = cfg();
        let spot = Vec2::new(400.0, 400.0);
        let mut trap = WebTrap::new(WebMode::BuildFirst, spot, spot, &c);
        trap.phase = TrapPhase::Trapped { elapsed: 0.0 };
        // cursor tentou sair: volta para a borda
        let far = spot + Vec2::new(500.0, 0.0);
        let out = trap.update(
            TrapInput {
                dt: DT,
                hunter_pos: Vec2::ZERO,
                hunter_alive: false,
                cursor: Some(far),
            },
            &c,
        );
        let w = out.warp_cursor.expect("deve reconter o cursor");
        assert!((w.distance(spot) - c.radius_px * CONFINE_FRACTION).abs() < 0.01);
        // dentro: nao mexe
        let out = trap.update(
            TrapInput {
                dt: DT,
                hunter_pos: Vec2::ZERO,
                hunter_alive: false,
                cursor: Some(spot),
            },
            &c,
        );
        assert_eq!(out.warp_cursor, None);
        // cacador morto nao solta depois de preso
        assert!(out.ended.is_none());
        // seguranca: tempo maximo
        trap.phase = TrapPhase::Trapped {
            elapsed: c.max_trap_secs - 0.01,
        };
        let out = trap.update(
            TrapInput {
                dt: 0.1,
                hunter_pos: Vec2::ZERO,
                hunter_alive: true,
                cursor: Some(spot),
            },
            &c,
        );
        assert_eq!(out.ended, Some(TrapEnd::Timeout));
    }

    #[test]
    fn damage_destroys_web_after_enough_hits() {
        let c = cfg();
        let mut trap = WebTrap::new(WebMode::Wrap, Vec2::ZERO, Vec2::ZERO, &c);
        let mut hits = 0;
        while !trap.damage(2.0) {
            hits += 1;
            assert!(hits < 1000);
        }
        assert_eq!(hits + 1, (c.web_hp / 2.0).ceil() as i32);
        assert_eq!(trap.hp, 0.0);
    }

    #[test]
    fn contains_only_when_trapped() {
        let c = cfg();
        let mut trap = WebTrap::new(WebMode::Wrap, Vec2::ZERO, Vec2::new(100.0, 100.0), &c);
        assert!(!trap.contains(Vec2::new(100.0, 100.0)));
        trap.phase = TrapPhase::Trapped { elapsed: 0.0 };
        assert!(trap.contains(Vec2::new(100.0, 100.0)));
        assert!(!trap.contains(Vec2::new(100.0 + c.radius_px + 1.0, 100.0)));
    }

    #[test]
    fn spot_is_inside_screen_and_near_requested_distance() {
        let c = cfg();
        let screen = Rect::from_xywh(0.0, 0.0, 1920.0, 1080.0);
        let mut rng = Rng::new(3);
        for cursor in [
            Vec2::new(5.0, 5.0),
            Vec2::new(960.0, 540.0),
            Vec2::new(1915.0, 1075.0),
        ] {
            let s = choose_spot(cursor, &screen, &c, &mut rng);
            assert!(screen.inflate(-c.radius_px).contains(s), "{s}");
            assert!(
                s.distance(cursor) > c.spot_distance_px * 0.5,
                "{cursor} -> {s}"
            );
        }
    }

    #[test]
    fn mode_parsing() {
        let mut rng = Rng::new(1);
        assert_eq!(pick_mode("build_first", &mut rng), WebMode::BuildFirst);
        assert_eq!(pick_mode("wrap", &mut rng), WebMode::Wrap);
        let modes: Vec<WebMode> = (0..40).map(|_| pick_mode("random", &mut rng)).collect();
        assert!(modes.contains(&WebMode::BuildFirst) && modes.contains(&WebMode::Wrap));
    }
}
