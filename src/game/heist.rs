//! Sequestro do mouse (logica pura, sem Bevy/SO).
//!
//! A aranha so vem quando o mouse fica PARADO por `web.idle_secs` (a contagem do painel zera a
//! cada movimento) e ha uma aranha viva e uma teia inteira no canto. Entao:
//!
//! 1. **Fisgada**: a aranha lanca um fio de seda ate o cursor.
//! 2. **Passeio**: anda pelas superficies com o cursor pendurado no fio (pendulo de verdade:
//!    balanca, arrasta no chao, obedece aos puxoes do usuario).
//! 3. **Brincadeira**: em cada parada faz o cursor de ioio.
//! 4. **Teia**: leva o cursor para a teia do canto da tela, onde ele fica preso.
//!
//! O jogador se solta cortando o fio (cliques = golpes de espada, ou sacudindo o mouse) e, preso,
//! destruindo a teia a golpes. Seguranca: a teia solta sozinha apos `web.max_trap_secs`, cada
//! fase tem tempo maximo, e o atalho de emergencia encerra o app. O teclado nunca e afetado.

use bevy::math::Vec2;

use super::trap::confine;
use crate::config::WebConfig;
use crate::navigation::geometry::Rect;
use crate::rng::Rng;

/// Duracao do voo do fio ate o cursor (s).
pub const HOOK_SECS: f32 = 0.45;
/// Tempo mirando antes de lancar o fio (s).
pub const AIM_SECS: f32 = 0.25;
/// Mexer o mouse mais que isso (px) enquanto o fio voa = escapar.
pub const ESCAPE_DIST: f32 = 30.0;
/// Descanso da aranha depois de errar o bote (s).
const ESCAPE_REST_SECS: f32 = 3.0;
/// Movimento (px) abaixo disso entre leituras e tremor, nao "mexeu o mouse".
const STILL_PX: f32 = 3.0;
/// Espera maxima pela posicao do cursor depois de comecar a capturar (s).
const CURSOR_WAIT_SECS: f32 = 1.5;
/// Tempo maximo de um trecho de passeio/ida a teia (s): destino inalcancavel nao trava a cacada.
const LEG_MAX_SECS: f32 = 14.0;
/// Fracao do raio da teia em que o cursor preso se move.
pub const CONFINE_FRACTION: f32 = 0.8;
const GRAVITY: f32 = 2200.0;
/// Quadros de historico de posicoes do cursor (cobre o atraso do sistema ao move-lo).
const WARP_HISTORY: usize = 8;
/// Diferenca (px) entre leitura e posicao pedida tratada como arredondamento, nao como puxao.
const PULL_NOISE_PX: f32 = 1.5;
/// Velocidade com que a aranha recolhe/solta o fio (px/s).
const REEL_SPEED: f32 = 900.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HeistPhase {
    /// Contagem regressiva (`Heist::countdown`).
    Waiting,
    /// A teia foi arrancada: a aranha vai ate o canto e tece outra (`Heist::web_built` cresce).
    Weaving,
    /// Fio voando ate o cursor; `t` 0..1.
    Hooking { t: f32 },
    /// Andando ate o proximo ponto com o cursor pendurado.
    Carrying,
    /// Parada para brincar; `left` segundos restantes.
    Playing { left: f32 },
    /// Levando o cursor para a teia do canto.
    ToWeb,
    /// Cursor preso na teia.
    Trapped { elapsed: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeistEvent {
    /// A aranha lancou o fio.
    Started,
    /// O fio pegou o cursor.
    Hooked,
    /// O jogador cortou o fio (golpes ou sacudidas).
    ThreadCut,
    /// A aranha morreu/sumiu com o cursor na mao.
    HunterLost,
    /// O cursor chegou a teia e ficou preso.
    Trapped,
    /// O jogador destruiu a teia (com o cursor preso nela ou nao).
    WebDestroyed,
    /// A aranha terminou de tecer uma teia nova.
    WebRebuilt,
    /// A teia soltou sozinha (seguranca).
    Timeout,
    /// Nao foi possivel saber onde o cursor esta; cacada cancelada.
    NoCursor,
    /// O jogador mexeu o mouse antes de o fio chegar.
    Escaped,
}

/// A aranha cacadora neste quadro.
#[derive(Debug, Clone, Copy)]
pub struct Hunter {
    /// De onde o fio sai (px locais do overlay).
    pub pos: Vec2,
    /// Chegou ao destino pedido no quadro anterior.
    pub arrived: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct HeistInput {
    pub dt: f32,
    /// `None` = nao ha aranha viva/disponivel.
    pub hunter: Option<Hunter>,
    /// Cursor real (px locais), se conhecido.
    pub cursor: Option<Vec2>,
    pub screen: Rect,
    /// Centro da teia do canto.
    pub web_center: Vec2,
    /// Segundos desde a ultima entrada do usuario segundo o sistema, quando a posicao do cursor
    /// nao e confiavel (ex.: Wayland sem a extensao). Limita o tempo de "mouse parado".
    pub idle_hint: Option<f32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HeistOutput {
    /// Para onde a cacadora deve andar e a que velocidade (px/s).
    pub goal: Option<(Vec2, f32)>,
    /// Onde o cursor real deve ser colocado agora.
    pub warp: Option<Vec2>,
    /// O overlay deve capturar o mouse inteiro (nada de click-through).
    pub capture: bool,
    pub events: Vec<HeistEvent>,
}

#[derive(Debug, Clone)]
pub struct Heist {
    pub phase: HeistPhase,
    /// Segundos ate a proxima cacada: o que falta de mouse parado (ou de descanso da aranha).
    pub countdown: f32,
    /// Ha quanto tempo o mouse esta parado (s).
    pub still: f32,
    /// Descanso da aranha depois de uma cacada (s).
    pub rest_left: f32,
    last_seen: Option<Vec2>,
    /// Onde o cursor estava quando a aranha mirou (mexer dali = escapar).
    anchor: Option<Vec2>,
    /// Posicao do cursor pendurado (px locais).
    pub bob: Vec2,
    bob_vel: Vec2,
    pub thread_hp: f32,
    pub thread_max: f32,
    pub web_hp: f32,
    pub web_max: f32,
    /// Quanto da teia do canto existe: 0 = arrancada, 1 = inteira (cresce enquanto a aranha tece).
    pub web_built: f32,
    /// Segundos ate a aranha ir refazer a teia arrancada.
    pub rebuild_left: f32,
    stops_left: u32,
    waypoint: Vec2,
    phase_time: f32,
    /// Comprimento atual do fio (muda no ioio).
    rope: f32,
    /// Ultimas posicoes em que o cursor foi colocado (o sistema aplica com atraso de alguns
    /// quadros; ler uma delas de volta nao e puxao do usuario).
    recent_warps: Vec<Vec2>,
    /// Golpe recente (para o desenho piscar).
    pub flash: f32,
    rng: Rng,
    /// Cacadas que chegaram a fisgar o cursor.
    pub hooked_total: u32,
}

impl Heist {
    pub fn new(seed: u64, cfg: &WebConfig) -> Self {
        Self {
            phase: HeistPhase::Waiting,
            countdown: cfg.idle_secs,
            still: 0.0,
            rest_left: 0.0,
            last_seen: None,
            anchor: None,
            bob: Vec2::ZERO,
            bob_vel: Vec2::ZERO,
            thread_hp: cfg.thread_hp,
            thread_max: cfg.thread_hp,
            web_hp: cfg.web_hp,
            web_max: cfg.web_hp,
            stops_left: 0,
            waypoint: Vec2::ZERO,
            phase_time: 0.0,
            rope: cfg.rope_px,
            recent_warps: Vec::new(),
            flash: 0.0,
            rng: Rng::new(seed ^ 0x05ED_AF10),
            hooked_total: 0,
            web_built: 1.0,
            rebuild_left: 0.0,
        }
    }

    /// A aranha esta com o cursor no fio (passeando, brincando ou indo para a teia).
    pub fn has_cursor(&self) -> bool {
        matches!(
            self.phase,
            HeistPhase::Carrying | HeistPhase::Playing { .. } | HeistPhase::ToWeb
        )
    }

    pub fn is_trapped(&self) -> bool {
        matches!(self.phase, HeistPhase::Trapped { .. })
    }

    /// Cacada em andamento (qualquer fase menos a espera).
    pub fn active(&self) -> bool {
        self.phase != HeistPhase::Waiting
    }

    /// Fio a desenhar: (origem na aranha, ponta). `None` se nao ha fio.
    pub fn thread(&self, hunter: Vec2, cursor: Option<Vec2>) -> Option<(Vec2, Vec2)> {
        match self.phase {
            HeistPhase::Hooking { t } => cursor.map(|c| (hunter, hunter.lerp(c, t))),
            _ if self.has_cursor() => Some((hunter, self.bob)),
            _ => None,
        }
    }

    /// Comprimento atual do fio (px).
    pub fn rope(&self) -> f32 {
        self.rope
    }

    fn release(&mut self, extra_wait: f32, cfg: &WebConfig) {
        self.phase = HeistPhase::Waiting;
        self.rest_left = extra_wait.max(0.0);
        self.still = 0.0;
        self.last_seen = None;
        self.anchor = None;
        self.countdown = cfg.idle_secs.max(self.rest_left);
        self.recent_warps.clear();
        self.bob_vel = Vec2::ZERO;
        self.phase_time = 0.0;
    }

    fn pick_waypoint(&mut self, screen: &Rect) {
        let m = 60.0;
        let lo = screen.min + Vec2::splat(m);
        let hi = (screen.max - Vec2::splat(m)).max(lo);
        // longe do ponto atual, para o passeio atravessar a tela
        let mut best = self.bob;
        let mut best_d = -1.0;
        for _ in 0..6 {
            let p = Vec2::new(self.rng.range(lo.x, hi.x), self.rng.range(lo.y, hi.y));
            let d = p.distance(self.bob);
            if d > best_d {
                best = p;
                best_d = d;
            }
        }
        self.waypoint = best;
    }

    fn enter(&mut self, phase: HeistPhase) {
        self.phase = phase;
        self.phase_time = 0.0;
    }

    pub fn update(&mut self, inp: HeistInput, cfg: &WebConfig) -> HeistOutput {
        let dt = inp.dt.clamp(0.0, 0.25);
        let mut out = HeistOutput::default();
        self.flash = (self.flash - dt).max(0.0);
        if !cfg.enabled {
            if self.active() {
                self.release(0.0, cfg);
            }
            return out;
        }
        self.phase_time += dt;

        match self.phase {
            HeistPhase::Waiting => {
                // (o overlay ocioso atualiza devagar: a contagem usa o tempo real)
                let real = inp.dt.clamp(0.0, 2.0);
                self.rest_left = (self.rest_left - real).max(0.0);
                // regra: so rouba mouse PARADO. Qualquer movimento zera a contagem.
                let moved = match (inp.cursor, self.last_seen) {
                    (Some(c), Some(last)) => c.distance(last) > STILL_PX,
                    _ => false,
                };
                if inp.cursor.is_some() && (moved || self.last_seen.is_none()) {
                    self.last_seen = inp.cursor;
                }
                if moved {
                    self.still = 0.0;
                } else if inp.hunter.is_some() {
                    self.still += real;
                }
                if let Some(idle) = inp.idle_hint {
                    self.still = self.still.min(idle.max(0.0));
                }
                self.countdown = (cfg.idle_secs - self.still).max(self.rest_left).max(0.0);
                if inp.hunter.is_some() {
                    if self.web_built < 1.0 {
                        // sem teia nao ha para onde levar o cursor: primeiro refaz
                        self.rebuild_left = (self.rebuild_left - real).max(0.0);
                        if self.rebuild_left <= 0.0 || self.countdown <= 0.0 {
                            self.enter(HeistPhase::Weaving);
                        }
                    } else if self.countdown <= 0.0 {
                        self.anchor = None;
                        self.enter(HeistPhase::Hooking { t: 0.0 });
                        out.capture = true;
                        out.events.push(HeistEvent::Started);
                    }
                }
            }
            HeistPhase::Weaving => {
                let Some(hunter) = inp.hunter else {
                    // a tecela morreu: outra aranha tenta daqui a pouco (a teia fica pela metade)
                    self.rebuild_left = 5.0;
                    self.enter(HeistPhase::Waiting);
                    return out;
                };
                out.goal = Some((inp.web_center, cfg.hunt_speed));
                let near = hunter.pos.distance(inp.web_center) <= cfg.radius_px * 1.6;
                if (hunter.arrived && near) || self.phase_time > LEG_MAX_SECS {
                    self.web_built = (self.web_built + dt / cfg.build_secs.max(0.2)).min(1.0);
                }
                if self.web_built >= 1.0 {
                    self.web_max = cfg.web_hp;
                    self.web_hp = cfg.web_hp;
                    // da um respiro antes de vir atras do mouse
                    self.rest_left = self.rest_left.max(5.0);
                    self.enter(HeistPhase::Waiting);
                    out.goal = None;
                    out.events.push(HeistEvent::WebRebuilt);
                }
            }
            HeistPhase::Hooking { t } => {
                out.capture = true;
                let Some(hunter) = inp.hunter else {
                    out.events.push(HeistEvent::HunterLost);
                    self.release(0.0, cfg);
                    return out;
                };
                let Some(cursor) = inp.cursor.filter(|c| inp.screen.contains(*c)) else {
                    if self.phase_time > CURSOR_WAIT_SECS {
                        out.events.push(HeistEvent::NoCursor);
                        self.release(10.0, cfg);
                    }
                    return out;
                };
                // mira primeiro (a leitura do cursor assenta), depois lanca; mexeu = escapou
                if self.phase_time < AIM_SECS {
                    return out;
                }
                let anchor = *self.anchor.get_or_insert(cursor);
                if cursor.distance(anchor) > ESCAPE_DIST {
                    out.events.push(HeistEvent::Escaped);
                    self.release(ESCAPE_REST_SECS, cfg);
                    out.capture = false;
                    return out;
                }
                let t = t + dt / HOOK_SECS;
                if t >= 1.0 {
                    self.bob = cursor;
                    self.bob_vel = Vec2::ZERO;
                    self.recent_warps.clear();
                    self.thread_max = cfg.thread_hp;
                    self.thread_hp = cfg.thread_hp;
                    // o fio comeca do tamanho da distancia e e recolhido aos poucos
                    self.rope = hunter.pos.distance(cursor).max(cfg.rope_px);
                    self.stops_left = cfg.play_stops;
                    self.hooked_total += 1;
                    self.pick_waypoint(&inp.screen);
                    self.enter(HeistPhase::Carrying);
                    out.events.push(HeistEvent::Hooked);
                } else {
                    self.phase = HeistPhase::Hooking { t };
                }
            }
            HeistPhase::Carrying | HeistPhase::Playing { .. } | HeistPhase::ToWeb => {
                out.capture = true;
                let Some(hunter) = inp.hunter else {
                    out.events.push(HeistEvent::HunterLost);
                    self.release(0.0, cfg);
                    return out;
                };
                let mut swing = 0.0;
                let mut rope = cfg.rope_px;
                match self.phase {
                    HeistPhase::Carrying => {
                        out.goal = Some((self.waypoint, cfg.drag_speed));
                        if hunter.arrived || self.phase_time > LEG_MAX_SECS {
                            if self.stops_left > 0 {
                                self.stops_left -= 1;
                                self.enter(HeistPhase::Playing {
                                    left: cfg.play_secs,
                                });
                            } else {
                                self.enter(HeistPhase::ToWeb);
                            }
                        }
                    }
                    HeistPhase::Playing { left } => {
                        // ioio: o fio encolhe e estica; um empurrao lateral faz balancar
                        let w = self.phase_time * std::f32::consts::TAU * 0.9;
                        rope = cfg.rope_px * (0.6 + 0.4 * w.cos());
                        swing = (self.phase_time * 3.1).sin() * 900.0;
                        let left = left - dt;
                        if left <= 0.0 {
                            if self.stops_left > 0 {
                                self.pick_waypoint(&inp.screen);
                                self.enter(HeistPhase::Carrying);
                            } else {
                                self.enter(HeistPhase::ToWeb);
                            }
                        } else {
                            self.phase = HeistPhase::Playing { left };
                        }
                    }
                    _ => {
                        out.goal = Some((inp.web_center, cfg.drag_speed));
                        let near = hunter.pos.distance(inp.web_center) <= cfg.radius_px * 1.6;
                        if (hunter.arrived && near) || self.phase_time > LEG_MAX_SECS {
                            self.bob = inp.web_center;
                            self.web_max = cfg.web_hp;
                            if self.web_built < 1.0 {
                                // arrancada durante a cacada: a aranha embrulha o cursor numa nova
                                self.web_built = 1.0;
                                self.web_hp = cfg.web_hp;
                            }
                            self.recent_warps.clear();
                            self.enter(HeistPhase::Trapped { elapsed: 0.0 });
                            out.warp = Some(inp.web_center);
                            out.events.push(HeistEvent::Trapped);
                            return out;
                        }
                    }
                }
                let reel = REEL_SPEED * dt;
                self.rope += (rope - self.rope).clamp(-reel, reel);

                // puxao do usuario: o quanto o cursor real saiu de onde o colocamos
                let nearest = inp.cursor.and_then(|c| {
                    self.recent_warps
                        .iter()
                        .map(|w| c - *w)
                        .min_by(|a, b| a.length_squared().total_cmp(&b.length_squared()))
                });
                if let Some(pull) = nearest {
                    let len = pull.length();
                    if len > PULL_NOISE_PX && len < 600.0 && dt > 0.0 {
                        self.bob_vel += (pull / dt * 0.25).clamp_length_max(1800.0);
                        self.thread_hp -= len / 1000.0 * cfg.struggle_damage;
                    }
                }
                self.step_bob(dt, hunter.pos, swing, &inp.screen);
                out.warp = Some(self.bob);
                if self.recent_warps.len() >= WARP_HISTORY {
                    self.recent_warps.remove(0);
                }
                self.recent_warps.push(self.bob);
                if self.thread_hp <= 0.0 {
                    out.events.push(HeistEvent::ThreadCut);
                    self.release(cfg.cooldown_secs, cfg);
                    out.goal = None;
                }
            }
            HeistPhase::Trapped { elapsed } => {
                out.capture = true;
                let elapsed = elapsed + dt;
                self.phase = HeistPhase::Trapped { elapsed };
                if elapsed >= cfg.max_trap_secs {
                    out.events.push(HeistEvent::Timeout);
                    self.release(cfg.cooldown_secs, cfg);
                    return out;
                }
                match inp.cursor {
                    Some(c) => {
                        out.warp = confine(c, inp.web_center, cfg.radius_px * CONFINE_FRACTION);
                        self.bob = out.warp.unwrap_or(c);
                    }
                    None => out.warp = Some(inp.web_center),
                }
            }
        }
        out
    }

    /// Pendulo: gravidade + fio de comprimento `self.rope` preso em `anchor`, dentro da tela.
    fn step_bob(&mut self, dt: f32, anchor: Vec2, swing: f32, screen: &Rect) {
        // passos pequenos: o vinculo do fio fica estavel mesmo a 15 qps
        let steps = (dt / 0.008).ceil().clamp(1.0, 32.0);
        let h = dt / steps;
        for _ in 0..steps as u32 {
            self.bob_vel += Vec2::new(swing, GRAVITY) * h;
            self.bob_vel *= 1.0 - (1.6 * h).min(0.5);
            self.bob += self.bob_vel * h;
            let d = self.bob - anchor;
            let len = d.length();
            if len > self.rope && len > 1e-3 {
                let n = d / len;
                self.bob = anchor + n * self.rope;
                let outward = self.bob_vel.dot(n);
                if outward > 0.0 {
                    self.bob_vel -= n * outward;
                }
            }
            let lo = screen.min + Vec2::splat(3.0);
            let hi = (screen.max - Vec2::splat(3.0)).max(lo);
            let c = self.bob.clamp(lo, hi);
            if c.x != self.bob.x {
                self.bob_vel.x = 0.0;
            }
            if c.y != self.bob.y {
                self.bob_vel.y = 0.0;
                self.bob_vel.x *= 1.0 - (6.0 * h).min(0.9); // atrito no chao/teto
            }
            self.bob = c;
        }
    }

    /// Clique do jogador em `p` com `damage` de espada. Devolve o evento, se algo se rompeu.
    pub fn click(
        &mut self,
        p: Vec2,
        damage: f32,
        web_center: Vec2,
        cfg: &WebConfig,
    ) -> Option<HeistEvent> {
        let damage = damage.max(0.0);
        if self.has_cursor() {
            self.thread_hp -= damage;
            self.flash = 0.15;
            if self.thread_hp <= 0.0 {
                self.release(cfg.cooldown_secs, cfg);
                return Some(HeistEvent::ThreadCut);
            }
        } else if self.web_built > 0.0 && p.distance(web_center) <= cfg.radius_px {
            // a teia pode ser arrancada a qualquer hora, com o cursor preso nela ou nao
            self.web_hp = (self.web_hp - damage).max(0.0);
            self.flash = 0.15;
            if self.web_hp <= 0.0 {
                self.web_built = 0.0;
                self.rebuild_left = cfg.rebuild_secs;
                match self.phase {
                    HeistPhase::Trapped { .. } => self.release(cfg.cooldown_secs, cfg),
                    HeistPhase::Weaving => self.enter(HeistPhase::Waiting),
                    _ => {}
                }
                return Some(HeistEvent::WebDestroyed);
            }
        }
        None
    }
}

/// Centro da teia para o canto configurado (`top_left`, `top_right`, `bottom_left`,
/// `bottom_right`), afastado das bordas o bastante para o cursor preso caber na tela.
pub fn web_center(corner: &str, screen: &Rect, cfg: &WebConfig) -> Vec2 {
    let inset = cfg.radius_px * 0.62;
    let (right, bottom) = corner_sides(corner);
    Vec2::new(
        if right {
            screen.max.x - inset
        } else {
            screen.min.x + inset
        },
        if bottom {
            screen.max.y - inset
        } else {
            screen.min.y + inset
        },
    )
}

/// (direita?, embaixo?) do canto pelo texto do TOML; desconhecido = canto superior esquerdo.
pub fn corner_sides(corner: &str) -> (bool, bool) {
    (corner.contains("right"), corner.contains("bottom"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn cfg() -> WebConfig {
        WebConfig {
            idle_secs: 5.0,
            ..WebConfig::default()
        }
    }

    fn screen() -> Rect {
        Rect::from_xywh(0.0, 0.0, 1920.0, 1080.0)
    }

    /// Mundo de brinquedo: a cacadora voa em linha reta ate o destino; o cursor obedece aos warps.
    struct World {
        heist: Heist,
        hunter: Option<Vec2>,
        arrived: bool,
        cursor: Vec2,
        events: Vec<HeistEvent>,
        captured: bool,
        /// O usuario puxa o mouse isso (px) a cada quadro.
        pull: Vec2,
    }

    impl World {
        fn new(c: &WebConfig) -> Self {
            Self {
                heist: Heist::new(7, c),
                hunter: Some(Vec2::new(900.0, 0.0)),
                arrived: false,
                cursor: Vec2::new(600.0, 500.0),
                events: Vec::new(),
                captured: false,
                pull: Vec2::ZERO,
            }
        }

        fn run(&mut self, secs: f32, c: &WebConfig) {
            let web = web_center(&c.corner, &screen(), c);
            let mut t = 0.0;
            while t < secs {
                self.cursor += self.pull;
                let out = self.heist.update(
                    HeistInput {
                        dt: DT,
                        hunter: self.hunter.map(|pos| Hunter {
                            pos,
                            arrived: self.arrived,
                        }),
                        cursor: Some(self.cursor),
                        screen: screen(),
                        web_center: web,
                        idle_hint: None,
                    },
                    c,
                );
                self.arrived = false;
                if let (Some((to, speed)), Some(h)) = (out.goal, self.hunter.as_mut()) {
                    let d = to - *h;
                    *h += d.normalize_or_zero() * (speed * DT).min(d.length());
                    self.arrived = h.distance(to) < 1.0;
                }
                if let Some(w) = out.warp {
                    self.cursor = w;
                }
                self.captured = out.capture;
                self.events.extend(out.events);
                t += DT;
            }
        }

        fn has(&self, e: HeistEvent) -> bool {
            self.events.contains(&e)
        }
    }

    #[test]
    fn countdown_only_runs_with_a_spider_alive() {
        let c = cfg();
        let mut w = World::new(&c);
        w.hunter = None;
        w.run(20.0, &c);
        assert_eq!(w.heist.phase, HeistPhase::Waiting);
        assert!((w.heist.countdown - 5.0).abs() < 1e-3, "sem aranha: parada");
        assert!(!w.captured, "esperando: o mouse e todo do usuario");

        w.hunter = Some(Vec2::new(900.0, 0.0));
        w.run(4.0, &c);
        assert_eq!(w.heist.phase, HeistPhase::Waiting);
        w.run(1.2, &c);
        assert!(w.has(HeistEvent::Started));
    }

    #[test]
    fn a_moving_mouse_is_never_stolen_only_one_that_stays_still() {
        let c = cfg(); // idle_secs = 5
        let mut w = World::new(&c);
        // mexendo o tempo todo (mesmo devagar): nada acontece, a contagem nao sai do topo
        for i in 0..(30.0 / DT) as u32 {
            w.pull = Vec2::new(if (i / 40) % 2 == 0 { 2.0 } else { -2.0 }, 0.5);
            w.run(DT, &c);
            w.cursor = w.cursor.clamp(Vec2::splat(50.0), Vec2::new(1800.0, 1000.0));
            assert!(w.heist.countdown > 4.5, "contagem: {}", w.heist.countdown);
        }
        assert!(w.events.is_empty(), "{:?}", w.events);
        assert!(!w.captured);

        // parou: a contagem desce; um toque no mouse a 4 s zera tudo
        w.pull = Vec2::ZERO;
        w.run(4.0, &c);
        assert!(w.heist.countdown < 1.2 && w.events.is_empty());
        w.cursor += Vec2::new(25.0, 0.0);
        w.run(0.1, &c);
        assert!(w.heist.countdown > 4.8);
        w.run(4.0, &c);
        assert!(w.events.is_empty());
        // 5 s parado de verdade: ai sim
        w.run(1.2, &c);
        assert!(w.has(HeistEvent::Started));
    }

    #[test]
    fn system_idle_time_caps_stillness_when_the_cursor_cannot_be_tracked() {
        // cursor "congelado" (leitura velha), mas o sistema diz que o usuario esta ativo
        let c = cfg();
        let web = web_center(&c.corner, &screen(), &c);
        let mut h = Heist::new(1, &c);
        let mut events = Vec::new();
        let step = |h: &mut Heist, idle: f32| {
            let out = h.update(
                HeistInput {
                    dt: DT,
                    hunter: Some(Hunter {
                        pos: Vec2::ZERO,
                        arrived: false,
                    }),
                    cursor: Some(Vec2::new(500.0, 500.0)),
                    screen: screen(),
                    web_center: web,
                    idle_hint: Some(idle),
                },
                &c,
            );
            out.events
        };
        for _ in 0..(20.0 / DT) as u32 {
            events.extend(step(&mut h, 0.4));
        }
        assert!(events.is_empty(), "usuario ativo: {events:?}");
        let mut idle = 0.0;
        for _ in 0..(6.0 / DT) as u32 {
            idle += DT;
            events.extend(step(&mut h, idle));
        }
        assert!(events.contains(&HeistEvent::Started));
    }

    #[test]
    fn moving_the_mouse_while_the_thread_flies_escapes() {
        let c = cfg();
        let mut w = World::new(&c);
        w.run(5.0 + AIM_SECS + 0.1, &c);
        assert!(matches!(w.heist.phase, HeistPhase::Hooking { .. }));
        w.cursor += Vec2::new(80.0, 0.0);
        w.run(0.1, &c);
        assert!(w.has(HeistEvent::Escaped));
        assert!(!w.has(HeistEvent::Hooked));
        assert_eq!(w.heist.phase, HeistPhase::Waiting);
        assert!(!w.captured);
    }

    #[test]
    fn full_heist_hooks_carries_plays_and_traps_in_the_corner_web() {
        let c = cfg();
        let mut w = World::new(&c);
        w.run(5.2, &c);
        assert!(matches!(w.heist.phase, HeistPhase::Hooking { .. }));
        assert!(w.captured);
        assert_eq!(
            w.cursor,
            Vec2::new(600.0, 500.0),
            "fio voando: cursor intacto"
        );

        w.run(1.0, &c);
        assert!(w.has(HeistEvent::Hooked));
        assert!(w.heist.has_cursor());
        // pendurado: o cursor nunca fica mais longe da aranha do que o fio
        for _ in 0..600 {
            w.run(DT, &c);
            if let (true, Some(h)) = (w.heist.has_cursor(), w.hunter) {
                assert!(w.cursor.distance(h) <= w.heist.rope() + 2.0);
                assert!(screen().contains(w.cursor));
            }
        }
        for _ in 0..(90.0 / DT) as u32 {
            w.run(DT, &c);
            if w.heist.is_trapped() {
                break;
            }
        }
        assert!(w.has(HeistEvent::Trapped), "eventos: {:?}", w.events);
        let web = web_center(&c.corner, &screen(), &c);
        assert!(w.heist.is_trapped());
        assert!(w.cursor.distance(web) <= c.radius_px);
    }

    #[test]
    fn trapped_cursor_stays_in_the_web_until_it_is_destroyed() {
        let c = cfg();
        let mut w = World::new(&c);
        w.run(200.0, &c);
        if !w.heist.is_trapped() {
            // a primeira ja soltou por tempo; a proxima prende de novo
            w.run(60.0, &c);
        }
        let web = web_center(&c.corner, &screen(), &c);
        // forca um estado preso conhecido
        w.heist.phase = HeistPhase::Trapped { elapsed: 0.0 };
        w.heist.web_hp = c.web_hp;
        w.pull = Vec2::new(40.0, 25.0); // usuario tentando fugir
        w.run(2.0, &c);
        assert!(w.cursor.distance(web) <= c.radius_px * CONFINE_FRACTION + 50.0);
        w.pull = Vec2::ZERO;
        w.run(0.1, &c);
        assert!(w.cursor.distance(web) <= c.radius_px * CONFINE_FRACTION + 0.5);

        // clique fora da teia nao faz nada; dentro, tira vida ate destruir
        assert_eq!(w.heist.click(web + Vec2::splat(900.0), 4.0, web, &c), None);
        assert_eq!(w.heist.web_hp, c.web_hp);
        let mut out = None;
        for _ in 0..10 {
            out = w.heist.click(web, 4.0, web, &c);
            if out.is_some() {
                break;
            }
        }
        assert_eq!(out, Some(HeistEvent::WebDestroyed));
        assert_eq!(w.heist.phase, HeistPhase::Waiting);
        assert!(w.heist.countdown >= c.cooldown_secs);
        w.run(0.5, &c);
        assert!(!w.captured, "solto: nada de capturar o mouse");
    }

    #[test]
    fn web_can_be_torn_off_anytime_and_a_spider_weaves_a_new_one_before_hunting() {
        let c = WebConfig {
            idle_secs: 12.0,
            rebuild_secs: 6.0,
            build_secs: 2.0,
            ..WebConfig::default()
        };
        let web = web_center(&c.corner, &screen(), &c);
        let mut w = World::new(&c);
        w.run(1.0, &c);
        assert_eq!(w.heist.web_built, 1.0);

        // clique fora nao arranha; 3 golpes de 4 arrancam a teia (12 de vida), sem cursor preso
        assert_eq!(w.heist.click(web + Vec2::splat(300.0), 4.0, web, &c), None);
        assert_eq!(w.heist.web_hp, c.web_hp);
        assert_eq!(w.heist.click(web, 4.0, web, &c), None);
        assert_eq!(w.heist.click(web, 4.0, web, &c), None);
        assert_eq!(
            w.heist.click(web, 4.0, web, &c),
            Some(HeistEvent::WebDestroyed)
        );
        assert_eq!(w.heist.web_built, 0.0);
        assert_eq!(w.heist.phase, HeistPhase::Waiting);
        // sem teia nao ha o que golpear
        assert_eq!(w.heist.click(web, 4.0, web, &c), None);

        // fica sem teia pelo tempo configurado; depois a aranha vai ao canto e tece
        w.run(5.0, &c);
        assert_eq!(w.heist.web_built, 0.0);
        assert_eq!(w.heist.phase, HeistPhase::Waiting);
        w.run(1.5, &c);
        assert_eq!(w.heist.phase, HeistPhase::Weaving);
        assert!(!w.captured, "tecendo: o mouse continua livre");
        let mut growing = false;
        for _ in 0..(30.0 / DT) as u32 {
            w.run(DT, &c);
            growing |= w.heist.web_built > 0.0 && w.heist.web_built < 1.0;
            if w.has(HeistEvent::WebRebuilt) {
                break;
            }
        }
        assert!(growing, "a teia cresce aos poucos");
        assert!(w.has(HeistEvent::WebRebuilt));
        assert_eq!(w.heist.web_built, 1.0);
        assert_eq!(w.heist.web_hp, c.web_hp);
        assert!(w.hunter.unwrap().distance(web) < c.radius_px * 1.6);
        assert!(!w.has(HeistEvent::Started), "nao caca sem teia pronta");

        // com a teia de volta, a cacada acontece
        w.run(20.0, &c);
        assert!(w.has(HeistEvent::Hooked));
    }

    #[test]
    fn tearing_the_web_while_the_spider_weaves_sends_it_back_to_wait() {
        let c = WebConfig {
            rebuild_secs: 1.0,
            build_secs: 50.0,
            ..cfg()
        };
        let web = web_center(&c.corner, &screen(), &c);
        let mut w = World::new(&c);
        w.heist.web_built = 0.0;
        w.heist.rebuild_left = 0.0;
        w.run(12.0, &c);
        assert_eq!(w.heist.phase, HeistPhase::Weaving);
        assert!(w.heist.web_built > 0.0 && w.heist.web_built < 1.0);
        w.heist.web_hp = 1.0;
        assert_eq!(
            w.heist.click(web, 4.0, web, &c),
            Some(HeistEvent::WebDestroyed)
        );
        assert_eq!(w.heist.phase, HeistPhase::Waiting);
        assert_eq!(w.heist.web_built, 0.0);
        assert_eq!(w.heist.rebuild_left, 1.0);
    }

    #[test]
    fn web_releases_by_itself_after_the_safety_timeout() {
        let c = cfg();
        let mut w = World::new(&c);
        w.heist.phase = HeistPhase::Trapped { elapsed: 0.0 };
        w.run(c.max_trap_secs + 0.5, &c);
        assert!(w.has(HeistEvent::Timeout));
        assert_eq!(w.heist.phase, HeistPhase::Waiting);
    }

    #[test]
    fn clicks_or_shaking_cut_the_thread() {
        let c = cfg();
        // cliques
        let mut w = World::new(&c);
        w.run(6.5, &c);
        assert!(w.heist.has_cursor());
        let web = web_center(&c.corner, &screen(), &c);
        let mut cut = None;
        for _ in 0..20 {
            cut = w.heist.click(w.cursor, 2.0, web, &c);
            if cut.is_some() {
                break;
            }
        }
        assert_eq!(cut, Some(HeistEvent::ThreadCut));
        assert!(!w.heist.active());

        // sacudidas
        let mut w = World::new(&c);
        w.run(6.5, &c);
        assert!(w.heist.has_cursor());
        for i in 0..600 {
            w.pull = Vec2::new(if i % 2 == 0 { 90.0 } else { -90.0 }, 0.0);
            w.run(DT, &c);
        }
        assert!(w.has(HeistEvent::ThreadCut), "eventos: {:?}", w.events);
    }

    #[test]
    fn delayed_cursor_readback_is_not_mistaken_for_struggling() {
        // o sistema aplica cada movimento do cursor com 3 quadros de atraso e arredonda p/ inteiro
        let c = cfg();
        let web = web_center(&c.corner, &screen(), &c);
        let mut h = Heist::new(7, &c);
        let mut hunter = Vec2::new(900.0, 0.0);
        let mut pending: Vec<Vec2> = Vec::new();
        let mut cursor = Vec2::new(600.0, 500.0);
        let mut arrived = false;
        let mut carried = 0.0;
        for _ in 0..(40.0 / DT) as u32 {
            let out = h.update(
                HeistInput {
                    dt: DT,
                    hunter: Some(Hunter {
                        pos: hunter,
                        arrived,
                    }),
                    cursor: Some(cursor),
                    screen: screen(),
                    web_center: web,
                    idle_hint: None,
                },
                &c,
            );
            arrived = false;
            if let Some((to, speed)) = out.goal {
                let d = to - hunter;
                hunter += d.normalize_or_zero() * (speed * DT).min(d.length());
                arrived = hunter.distance(to) < 1.0;
            }
            if let Some(w) = out.warp {
                pending.push(w.round());
            }
            if pending.len() > 3 {
                cursor = pending.remove(0);
            }
            if h.has_cursor() {
                carried += DT;
                assert!(
                    h.thread_hp > c.thread_hp * 0.9,
                    "fio gasto sozinho: {}",
                    h.thread_hp
                );
            }
            if h.is_trapped() {
                break;
            }
        }
        assert!(carried > 5.0, "passeou so {carried} s");
    }

    #[test]
    fn hunter_death_frees_the_cursor() {
        let c = cfg();
        let mut w = World::new(&c);
        w.run(6.5, &c);
        assert!(w.heist.has_cursor());
        w.hunter = None;
        w.run(0.1, &c);
        assert!(w.has(HeistEvent::HunterLost));
        assert!(!w.heist.active());
        assert!(!w.captured);
    }

    #[test]
    fn unknown_cursor_cancels_instead_of_capturing_forever() {
        let c = cfg();
        let mut h = Heist::new(1, &c);
        let web = web_center(&c.corner, &screen(), &c);
        let mut events = Vec::new();
        let mut capture = false;
        for _ in 0..(10.0 / DT) as u32 {
            let out = h.update(
                HeistInput {
                    dt: DT,
                    hunter: Some(Hunter {
                        pos: Vec2::ZERO,
                        arrived: false,
                    }),
                    cursor: None,
                    screen: screen(),
                    web_center: web,
                    idle_hint: None,
                },
                &c,
            );
            capture = out.capture;
            events.extend(out.events);
        }
        assert!(events.contains(&HeistEvent::NoCursor));
        assert!(!capture);
        assert_eq!(h.phase, HeistPhase::Waiting);
    }

    #[test]
    fn disabled_never_starts() {
        let c = WebConfig {
            enabled: false,
            ..cfg()
        };
        let mut w = World::new(&c);
        w.run(30.0, &c);
        assert!(w.events.is_empty());
        assert!(!w.captured);
    }

    #[test]
    fn web_sits_inside_the_chosen_corner() {
        let c = cfg();
        let s = screen();
        let tl = web_center("top_left", &s, &c);
        let br = web_center("bottom_right", &s, &c);
        assert!(tl.x < 100.0 && tl.y < 100.0);
        assert!(br.x > 1820.0 && br.y > 980.0);
        assert_eq!(web_center("qualquer", &s, &c), tl);
    }
}
