//! Maquina de estados e "personalidade" da aranha. Logica pura: recebe percepcoes, devolve
//! intencoes. Quem move, anima e renderiza esta em `spider.rs`/`animation.rs`.

use crate::config::BehaviorConfig;
use crate::rng::Rng;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SpiderState {
    Idle,
    Walking,
    Climbing,
    Fleeing,
    /// Reservado para evolucao futura (a aranha nunca entra neste estado hoje).
    Attacking,
    Hit,
    Dead,
    Respawning,
}

impl SpiderState {
    /// Viva e livre para reagir ao mundo.
    pub fn is_alive_free(self) -> bool {
        matches!(
            self,
            Self::Idle | Self::Walking | Self::Climbing | Self::Fleeing
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorReaction {
    Flee,
    Freeze,
    SpeedUp,
    Reroute,
}

/// Percepcoes de um tick.
#[derive(Debug, Clone, Copy, Default)]
pub struct BrainInput {
    pub dt: f32,
    /// Distancia do cursor ao centro da aranha (px), se conhecida.
    pub cursor_dist: Option<f32>,
    pub on_vertical: bool,
    pub has_route: bool,
    /// A rota terminou neste tick.
    pub route_finished: bool,
    /// O usuario clicou na aranha neste tick (golpe letal).
    pub clicked: bool,
    /// Levou um golpe nao letal neste tick: foge do cursor.
    pub hurt: bool,
    /// Em queda livre (nao planeja rotas).
    pub falling: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanRequest {
    /// Escolher destino longe do cursor.
    pub flee_from_cursor: bool,
}

/// Intencoes de um tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrainOutput {
    pub state: SpiderState,
    /// Multiplicador da velocidade base do estado (0 = parada).
    pub speed_factor: f32,
    pub plan: Option<PlanRequest>,
    /// Acabou de ser atingida (pontuar, efeito visual).
    pub killed: bool,
    /// Deve reaparecer agora (escolher ponto de nascimento).
    pub spawn: bool,
    pub reaction: Option<CursorReaction>,
}

const HIT_FLINCH_SECS: f32 = 0.18;
const RESPAWN_ANIM_SECS: f32 = 0.7;
const SPEEDUP_SECS: f32 = 1.6;
/// Pausa minima entre dois planejamentos consecutivos quando nao ha rota (evita laco quente).
const REPLAN_COOLDOWN: f32 = 0.5;

#[derive(Debug, Clone)]
pub struct Brain {
    state: SpiderState,
    /// Tempo no estado atual.
    state_time: f32,
    /// Temporizador regressivo do estado (Idle/Fleeing).
    timer: f32,
    rng: Rng,
    // variacao natural de velocidade
    speed_now: f32,
    speed_target: f32,
    retarget_in: f32,
    pause_left: f32,
    boost_left: f32,
    // reacao ao cursor
    cursor_inside: bool,
    react_cooldown: f32,
    replan_cooldown: f32,
}

impl Brain {
    pub fn new(seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let timer = rng.range(0.3, 1.2);
        Self {
            state: SpiderState::Idle,
            state_time: 0.0,
            timer,
            rng,
            speed_now: 1.0,
            speed_target: 1.0,
            retarget_in: 0.5,
            pause_left: 0.0,
            boost_left: 0.0,
            cursor_inside: false,
            react_cooldown: 0.0,
            replan_cooldown: 0.0,
        }
    }

    pub fn state(&self) -> SpiderState {
        self.state
    }

    pub fn state_time(&self) -> f32 {
        self.state_time
    }

    pub fn rng(&mut self) -> &mut Rng {
        &mut self.rng
    }

    fn enter(&mut self, s: SpiderState) {
        self.state = s;
        self.state_time = 0.0;
    }

    fn idle_for(&mut self, cfg: &BehaviorConfig) {
        self.timer = self.rng.range(cfg.idle_min_secs, cfg.idle_max_secs);
        self.enter(SpiderState::Idle);
    }

    /// Volta ao repouso (ex.: apos o grafo ficar sem caminhos).
    pub fn force_idle(&mut self, cfg: &BehaviorConfig) {
        if self.state.is_alive_free() {
            self.idle_for(cfg);
        }
    }

    pub fn update(&mut self, inp: BrainInput, cfg: &BehaviorConfig) -> BrainOutput {
        let dt = inp.dt.clamp(0.0, 1.0);
        self.state_time += dt;
        self.react_cooldown = (self.react_cooldown - dt).max(0.0);
        self.replan_cooldown = (self.replan_cooldown - dt).max(0.0);

        let mut out = BrainOutput {
            state: self.state,
            speed_factor: 0.0,
            plan: None,
            killed: false,
            spawn: false,
            reaction: None,
        };

        // clique tem prioridade sobre qualquer estado "vivo"
        if inp.clicked && self.state.is_alive_free() {
            self.enter(SpiderState::Hit);
            out.killed = true;
            out.state = self.state;
            return out;
        }

        // golpe nao letal: foge sempre (sem sorteio nem cooldown)
        if inp.hurt && self.state.is_alive_free() {
            self.timer = cfg.flee_duration_secs;
            self.enter(SpiderState::Fleeing);
            self.react_cooldown = cfg.react_cooldown_secs;
            self.replan_cooldown = REPLAN_COOLDOWN;
            out.plan = Some(PlanRequest {
                flee_from_cursor: true,
            });
            out.reaction = Some(CursorReaction::Flee);
            out.state = self.state;
            out.speed_factor = self.speed_factor(dt, cfg);
            return out;
        }

        match self.state {
            SpiderState::Hit => {
                if self.state_time >= HIT_FLINCH_SECS {
                    self.enter(SpiderState::Dead);
                }
            }
            SpiderState::Dead => {
                if self.state_time >= cfg.death_anim_secs + cfg.respawn_delay_secs {
                    self.enter(SpiderState::Respawning);
                    out.spawn = true;
                }
            }
            SpiderState::Respawning => {
                if self.state_time >= RESPAWN_ANIM_SECS {
                    self.idle_for(cfg);
                }
            }
            SpiderState::Idle => {
                self.timer -= dt;
                self.react_to_cursor(&inp, cfg, &mut out);
                if self.state == SpiderState::Idle && self.timer <= 0.0 && !inp.falling {
                    self.enter(SpiderState::Walking);
                    out.plan = Some(PlanRequest {
                        flee_from_cursor: false,
                    });
                    self.replan_cooldown = REPLAN_COOLDOWN;
                }
            }
            SpiderState::Walking | SpiderState::Climbing | SpiderState::Attacking => {
                // Walking <-> Climbing seguem a superficie, nao o acaso
                let want = if inp.on_vertical {
                    SpiderState::Climbing
                } else {
                    SpiderState::Walking
                };
                if self.state != want {
                    let t = self.state_time;
                    self.enter(want);
                    self.state_time = t; // continuidade de animacao
                }
                self.react_to_cursor(&inp, cfg, &mut out);
                if self.state.is_alive_free() && self.state != SpiderState::Fleeing {
                    if inp.route_finished {
                        self.idle_for(cfg);
                    } else if !inp.has_route && !inp.falling && self.replan_cooldown <= 0.0 {
                        out.plan = Some(PlanRequest {
                            flee_from_cursor: false,
                        });
                        self.replan_cooldown = REPLAN_COOLDOWN;
                    }
                }
            }
            SpiderState::Fleeing => {
                self.timer -= dt;
                if inp.route_finished || self.timer <= 0.0 {
                    self.idle_for(cfg);
                } else if !inp.has_route && !inp.falling && self.replan_cooldown <= 0.0 {
                    out.plan = Some(PlanRequest {
                        flee_from_cursor: true,
                    });
                    self.replan_cooldown = REPLAN_COOLDOWN;
                }
            }
        }

        // memoriza se o cursor esta dentro (gatilho por entrada, nao por permanencia)
        self.cursor_inside = inp.cursor_dist.is_some_and(|d| d < cfg.detect_radius_px);

        out.state = self.state;
        out.speed_factor = self.speed_factor(dt, cfg);
        out
    }

    fn react_to_cursor(&mut self, inp: &BrainInput, cfg: &BehaviorConfig, out: &mut BrainOutput) {
        let inside = inp.cursor_dist.is_some_and(|d| d < cfg.detect_radius_px);
        let entered = inside && !self.cursor_inside;
        if !entered || self.react_cooldown > 0.0 || !self.state.is_alive_free() {
            return;
        }
        self.react_cooldown = cfg.react_cooldown_secs;
        if !self.rng.chance(cfg.react_probability) {
            return;
        }
        let weights = [
            cfg.weight_flee,
            cfg.weight_freeze,
            cfg.weight_speedup,
            cfg.weight_reroute,
        ];
        let Some(idx) = self.rng.weighted(&weights) else {
            return;
        };
        let reaction = [
            CursorReaction::Flee,
            CursorReaction::Freeze,
            CursorReaction::SpeedUp,
            CursorReaction::Reroute,
        ][idx];
        out.reaction = Some(reaction);
        match reaction {
            CursorReaction::Flee => {
                self.timer = cfg.flee_duration_secs;
                self.enter(SpiderState::Fleeing);
                out.plan = Some(PlanRequest {
                    flee_from_cursor: true,
                });
                self.replan_cooldown = REPLAN_COOLDOWN;
            }
            CursorReaction::Freeze => {
                self.timer = self.rng.range(0.5, 1.5);
                self.enter(SpiderState::Idle);
            }
            CursorReaction::SpeedUp => self.boost_left = SPEEDUP_SECS,
            CursorReaction::Reroute => {
                out.plan = Some(PlanRequest {
                    flee_from_cursor: false,
                });
                self.replan_cooldown = REPLAN_COOLDOWN;
            }
        }
    }

    /// Variacao natural: alvo de velocidade sorteado a cada ~0.5-1.5 s, suavizado, com
    /// micro-pausas ocasionais. Estados de repouso/morte nao andam.
    fn speed_factor(&mut self, dt: f32, cfg: &BehaviorConfig) -> f32 {
        if !matches!(
            self.state,
            SpiderState::Walking | SpiderState::Climbing | SpiderState::Fleeing
        ) {
            self.speed_now = 1.0;
            self.pause_left = 0.0;
            return 0.0;
        }

        self.retarget_in -= dt;
        if self.retarget_in <= 0.0 {
            self.retarget_in = self.rng.range(0.5, 1.5);
            self.speed_target = self.rng.range(0.75, 1.2);
        }
        if self.pause_left <= 0.0
            && self.state != SpiderState::Fleeing
            && self
                .rng
                .chance((cfg.pause_chance_per_sec * dt).clamp(0.0, 1.0))
        {
            self.pause_left = self.rng.range(0.3, 1.0);
        }
        self.boost_left = (self.boost_left - dt).max(0.0);

        let target = if self.pause_left > 0.0 {
            self.pause_left -= dt;
            0.0
        } else {
            self.speed_target
        };
        // aproximacao exponencial: partidas e paradas suaves
        let k = 1.0 - (-8.0 * dt).exp();
        self.speed_now += (target - self.speed_now) * k;

        let boost = if self.boost_left > 0.0 { 1.8 } else { 1.0 };
        (self.speed_now * boost).clamp(0.0, 2.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> BehaviorConfig {
        BehaviorConfig {
            idle_min_secs: 1.0,
            idle_max_secs: 1.0,
            respawn_delay_secs: 5.0,
            death_anim_secs: 1.0,
            flee_duration_secs: 2.0,
            react_probability: 1.0,
            react_cooldown_secs: 1.0,
            weight_flee: 1.0,
            weight_freeze: 0.0,
            weight_speedup: 0.0,
            weight_reroute: 0.0,
            pause_chance_per_sec: 0.0,
            detect_radius_px: 100.0,
            ..BehaviorConfig::default()
        }
    }

    fn tick(
        b: &mut Brain,
        c: &BehaviorConfig,
        dt: f32,
        f: impl FnOnce(&mut BrainInput),
    ) -> BrainOutput {
        let mut i = BrainInput {
            dt,
            ..Default::default()
        };
        f(&mut i);
        b.update(i, c)
    }

    fn run_until(
        b: &mut Brain,
        c: &BehaviorConfig,
        secs: f32,
        mut f: impl FnMut(&mut BrainInput),
    ) -> Vec<BrainOutput> {
        let mut outs = Vec::new();
        let mut t = 0.0;
        while t < secs {
            let mut i = BrainInput {
                dt: 0.05,
                ..Default::default()
            };
            f(&mut i);
            outs.push(b.update(i, c));
            t += 0.05;
        }
        outs
    }

    #[test]
    fn idle_becomes_walking_with_a_plan_request() {
        let c = cfg();
        let mut b = Brain::new(1);
        let outs = run_until(&mut b, &c, 3.0, |_| {});
        let first_plan = outs
            .iter()
            .position(|o| o.plan.is_some())
            .expect("deve pedir rota");
        assert_eq!(outs[first_plan].state, SpiderState::Walking);
        assert!(!outs[first_plan].plan.unwrap().flee_from_cursor);
    }

    #[test]
    fn finishing_route_goes_back_to_idle() {
        let c = cfg();
        let mut b = Brain::new(2);
        run_until(&mut b, &c, 3.0, |i| i.has_route = true);
        assert_eq!(b.state(), SpiderState::Walking);
        let o = tick(&mut b, &c, 0.05, |i| i.route_finished = true);
        assert_eq!(o.state, SpiderState::Idle);
        assert_eq!(o.speed_factor, 0.0);
    }

    #[test]
    fn climbing_follows_surface() {
        let c = cfg();
        let mut b = Brain::new(3);
        run_until(&mut b, &c, 3.0, |i| i.has_route = true);
        let o = tick(&mut b, &c, 0.05, |i| {
            i.has_route = true;
            i.on_vertical = true;
        });
        assert_eq!(o.state, SpiderState::Climbing);
        let o = tick(&mut b, &c, 0.05, |i| i.has_route = true);
        assert_eq!(o.state, SpiderState::Walking);
    }

    #[test]
    fn click_kills_then_respawns_exactly_once() {
        let c = cfg();
        let mut b = Brain::new(4);
        let o = tick(&mut b, &c, 0.05, |i| i.clicked = true);
        assert!(o.killed);
        assert_eq!(o.state, SpiderState::Hit);

        let mut killed = 0;
        let mut spawned = 0;
        let mut seen = vec![SpiderState::Hit];
        // cliques durante a morte/hit sao ignorados; apos reaparecer a aranha volta a ser alvo valido
        let mut outs = run_until(&mut b, &c, 5.5, |i| i.clicked = true);
        outs.extend(run_until(&mut b, &c, 4.0, |_| {}));
        for o in &outs {
            killed += o.killed as u32;
            spawned += o.spawn as u32;
            if seen.last() != Some(&o.state) {
                seen.push(o.state);
            }
            if matches!(
                o.state,
                SpiderState::Hit | SpiderState::Dead | SpiderState::Respawning
            ) {
                assert_eq!(o.speed_factor, 0.0);
            }
        }
        assert_eq!(spawned, 1);
        assert_eq!(killed, 0, "morta nao pode ser morta de novo");
        assert_eq!(
            &seen[..4],
            &[
                SpiderState::Hit,
                SpiderState::Dead,
                SpiderState::Respawning,
                SpiderState::Idle
            ]
        );
    }

    #[test]
    fn dead_time_matches_config() {
        let c = cfg();
        let mut b = Brain::new(5);
        tick(&mut b, &c, 0.05, |i| i.clicked = true);
        let outs = run_until(&mut b, &c, 20.0, |_| {});
        let dead_ticks = outs.iter().filter(|o| o.state == SpiderState::Dead).count() as f32 * 0.05;
        // death_anim (1) + respawn_delay (5) = 6 s, menos o flinch ja consumido
        assert!((5.5..6.3).contains(&dead_ticks), "{dead_ticks}");
    }

    #[test]
    fn cursor_entering_triggers_flee_once_until_reentry() {
        let c = cfg();
        let mut b = Brain::new(6);
        run_until(&mut b, &c, 3.0, |i| i.has_route = true);
        // cursor longe
        tick(&mut b, &c, 0.05, |i| i.cursor_dist = Some(500.0));
        // cursor entra no raio
        let o = tick(&mut b, &c, 0.05, |i| {
            i.cursor_dist = Some(50.0);
            i.has_route = true;
        });
        assert_eq!(o.reaction, Some(CursorReaction::Flee));
        assert_eq!(o.state, SpiderState::Fleeing);
        assert_eq!(
            o.plan,
            Some(PlanRequest {
                flee_from_cursor: true
            })
        );

        // continua dentro: nenhuma nova reacao
        let outs = run_until(&mut b, &c, 1.0, |i| {
            i.cursor_dist = Some(50.0);
            i.has_route = true;
        });
        assert!(outs.iter().all(|o| o.reaction.is_none()));

        // fuga acaba por tempo
        let outs = run_until(&mut b, &c, 3.0, |i| {
            i.cursor_dist = Some(500.0);
            i.has_route = true;
        });
        assert!(outs.iter().any(|o| o.state == SpiderState::Idle));

        // sai e reentra depois do cooldown: reage de novo
        let o = tick(&mut b, &c, 0.05, |i| i.cursor_dist = Some(40.0));
        assert!(o.reaction.is_some());
    }

    #[test]
    fn non_lethal_hit_always_flees_even_with_zero_reaction_chance() {
        let mut c = cfg();
        c.react_probability = 0.0;
        let mut b = Brain::new(12);
        let o = tick(&mut b, &c, 0.05, |i| i.hurt = true);
        assert_eq!(o.state, SpiderState::Fleeing);
        assert_eq!(
            o.plan,
            Some(PlanRequest {
                flee_from_cursor: true
            })
        );
        assert!(!o.killed);
        // morta nao reage a golpe
        tick(&mut b, &c, 0.05, |i| i.clicked = true);
        let o = tick(&mut b, &c, 0.05, |i| i.hurt = true);
        assert_ne!(o.state, SpiderState::Fleeing);
    }

    #[test]
    fn reaction_probability_zero_never_reacts() {
        let mut c = cfg();
        c.react_probability = 0.0;
        let mut b = Brain::new(7);
        for k in 0..200 {
            let far = k % 2 == 0;
            let o = tick(&mut b, &c, 0.5, |i| {
                i.cursor_dist = Some(if far { 500.0 } else { 10.0 })
            });
            assert!(o.reaction.is_none());
        }
    }

    #[test]
    fn no_reaction_while_dead_or_hit() {
        let c = cfg();
        let mut b = Brain::new(8);
        tick(&mut b, &c, 0.05, |i| i.clicked = true);
        for k in 0..20 {
            let d = if k % 2 == 0 { 500.0 } else { 5.0 };
            let o = tick(&mut b, &c, 0.05, |i| i.cursor_dist = Some(d));
            assert!(o.reaction.is_none());
        }
    }

    #[test]
    fn speed_factor_is_bounded_varies_and_pauses() {
        let mut c = cfg();
        c.pause_chance_per_sec = 2.0;
        let mut b = Brain::new(9);
        run_until(&mut b, &c, 3.0, |i| i.has_route = true);
        let outs = run_until(&mut b, &c, 60.0, |i| i.has_route = true);
        assert!(
            outs.iter()
                .all(|o| o.speed_factor.is_finite() && (0.0..=2.5).contains(&o.speed_factor))
        );
        let walking: Vec<f32> = outs
            .iter()
            .filter(|o| o.state == SpiderState::Walking)
            .map(|o| o.speed_factor)
            .collect();
        let (min, max) = walking
            .iter()
            .fold((f32::MAX, f32::MIN), |(a, b), &v| (a.min(v), b.max(v)));
        assert!(max - min > 0.3, "velocidade quase constante: {min}..{max}");
        assert!(min < 0.25, "esperava micro-pausas, min {min}");
    }

    #[test]
    fn no_route_triggers_rate_limited_replans() {
        let c = cfg();
        let mut b = Brain::new(10);
        run_until(&mut b, &c, 3.0, |i| i.has_route = true);
        let outs = run_until(&mut b, &c, 2.0, |i| i.has_route = false);
        let plans = outs.iter().filter(|o| o.plan.is_some()).count();
        assert!((1..=5).contains(&plans), "plans={plans}");
    }

    #[test]
    fn huge_dt_does_not_panic_or_skip_states() {
        let c = cfg();
        let mut b = Brain::new(11);
        let o = tick(&mut b, &c, 1000.0, |_| {});
        assert!(o.speed_factor.is_finite());
    }
}
