//! Mobs como entidades do jogo: simulacao (movimento + comportamento) de cada um, sincronizacao
//! com a cena 3D e interacao com o mouse.

use bevy::prelude::*;

use bevy::input::ButtonState;
use bevy::input::mouse::MouseButtonInput;
use bevy::window::PrimaryWindow;

use super::animation::{
    ease_out_back, ease_out_cubic, gait_frequency, orientation, screen_to_world_dir, smooth_factor,
};
use super::behavior::{Brain, BrainInput, SpiderState};
use super::effects::{EffectAssets, EffectsActive, spawn_burst};
use super::glb::PendingModel;
use super::species::{MAX_HITS, Profile};
use crate::config::Config;
use crate::desktop::Desktop;
use crate::game::{Progress, Score, Strike};
use crate::navigation::geometry::Rect;
use crate::navigation::pathfinding::{choose_destination, plan_route, random_point};
use crate::navigation::walker::Walker;
use crate::navigation::window_graph::PathGraph;
use crate::overlay::{Activity, CursorState, HitCircle, HitTargets, OverlayGeometry};

const HURT_FLASH_SECS: f32 = 0.15;
/// Elites (filhotes de ninho velho) sao desenhados maiores.
pub const ELITE_SCALE: f32 = 1.3;

/// Raio de clique de referencia (`spider.hit_radius_px` padrao); o valor configurado escala os
/// circulos de cada especie proporcionalmente.
const REF_HIT_RADIUS_PX: f32 = 34.0;

/// Tamanho de 1 unidade local do modelo em pixels (com `spider.scale = 1`).
pub const PX_PER_UNIT: f32 = 28.0;

/// Camera ortografica 1 unidade = 1 pixel fisico.
#[derive(Component)]
pub struct OverlayCamera;

/// Um mob na cena (entidade raiz; o modelo 3D e filho dela).
#[derive(Component)]
pub struct Mob {
    pub sim: SpiderSim,
    /// Indice em [`crate::mobs::MobCatalog::kinds`].
    pub kind: usize,
}

/// Ordem externa: "va ate este ponto" (a cacada do mouse manda a aranha passear com o cursor).
/// Enquanto existir, o mob ignora o cursor e o proprio planejamento.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Errand {
    /// Destino em px locais; o mob vai ate o ponto do grafo mais proximo dele.
    pub target: Vec2,
    /// Velocidade em px/s.
    pub speed: f32,
    /// Chegou (fica `true` enquanto estiver parado no destino).
    pub arrived: bool,
    /// Nao ha caminho ate o destino.
    pub unreachable: bool,
    planned: Option<Vec2>,
}

/// Simulacao completa de um mob (logica pura: movimento + comportamento).
pub struct SpiderSim {
    pub walker: Walker,
    pub brain: Brain,
    pub placed: bool,
    pub visible: bool,
    /// O usuario clicou neste mob neste quadro (preenchido pela entrada).
    pub clicked: bool,
    route_finished: bool,
    /// Velocidade real em px/s neste frame.
    pub speed_px: f32,
    /// Fase do ciclo de marcha (voltas).
    pub gait_phase: f32,
    pub orientation: Quat,
    /// Tamanho, marcha e hitbox da especie.
    pub profile: Profile,
    /// Centro do corpo em coordenadas locais do overlay (px).
    pub body_center: Vec2,
    /// Raio do circulo que envolve o bicho (barra de vida, efeitos).
    pub hit_radius: f32,
    /// Circulos clicaveis (centro em px locais, raio em px); raio 0 = sem uso.
    pub hits: [(Vec2, f32); MAX_HITS],
    pub game_time: f32,
    pub pending_burst: Option<Vec2>,
    pub kills_logged: u32,
    /// Vida atual e maxima (`combat.mob_hp`).
    pub hp: f32,
    pub max_hp: f32,
    /// Golpe nao letal a entregar ao cerebro no proximo passo.
    hurt_pending: bool,
    /// Tempo restante do "tranco" visual de um golpe nao letal.
    pub hurt_flash: f32,
    /// `game_time` do ultimo golpe (barra de vida fica visivel por um tempo).
    pub last_hit: Option<f32>,
    /// Filhote de ninho: nasce neste ponto (px locais) em vez de num ponto aleatorio.
    pub spawn_at: Option<Vec2>,
    /// Filhote de ninho: nao reaparece depois de morto (quem repoe a ninhada e o ninho).
    pub brood: bool,
    /// Elite (ninho velho): mais vida, maior, e morde o cursor que fica parado por perto.
    pub elite: bool,
    /// Foi arremessado por uma sacudida e ainda nao pousou.
    shaken: bool,
    /// Tempo restante atordoado (nao anda nem planeja rota).
    pub stun_left: f32,
    /// Dano levado no pouso, a ser mostrado pela interface.
    pub fall_hurt: Option<f32>,
    bite_charge: f32,
    bite_cooldown: f32,
    /// Mordeu neste passo (o jogo aplica a perda de XP).
    pub pending_bite: bool,
    /// Ordem externa em andamento (ver [`Errand`]).
    pub errand: Option<Errand>,
}

impl SpiderSim {
    pub fn new(seed: u64) -> Self {
        Self::with_profile(seed, Profile::default())
    }

    pub fn with_profile(seed: u64, profile: Profile) -> Self {
        Self {
            walker: Walker::new(Vec2::new(100.0, 100.0)),
            brain: Brain::new(seed),
            placed: false,
            visible: false,
            clicked: false,
            route_finished: false,
            speed_px: 0.0,
            gait_phase: 0.0,
            orientation: Quat::IDENTITY,
            profile,
            body_center: Vec2::ZERO,
            hit_radius: 34.0,
            hits: [(Vec2::ZERO, 0.0); MAX_HITS],
            game_time: 0.0,
            pending_burst: None,
            kills_logged: 0,
            hp: 10.0,
            max_hp: 10.0,
            hurt_pending: false,
            hurt_flash: 0.0,
            last_hit: None,
            spawn_at: None,
            brood: false,
            elite: false,
            shaken: false,
            stun_left: 0.0,
            fall_hurt: None,
            bite_charge: 0.0,
            bite_cooldown: 0.0,
            pending_bite: false,
            errand: None,
        }
    }

    /// Manda o mob andar ate `target` (px locais) a `speed` px/s. Chamar todo quadro e barato:
    /// a rota so e refeita quando o destino muda.
    pub fn send_to(&mut self, target: Vec2, speed: f32) {
        match &mut self.errand {
            Some(e) => {
                if e.target.distance(target) > 1.0 {
                    e.arrived = false;
                    e.unreachable = false;
                }
                e.target = target;
                e.speed = speed;
            }
            None => {
                self.errand = Some(Errand {
                    target,
                    speed,
                    arrived: false,
                    unreachable: false,
                    planned: None,
                })
            }
        }
    }

    /// Encerra a ordem externa; o cerebro volta a mandar.
    pub fn end_errand(&mut self) {
        if self.errand.take().is_some() {
            self.walker.stop();
        }
    }

    /// Anda em direcao ao destino da ordem. Devolve a distancia percorrida (px).
    fn run_errand(&mut self, dt: f32, graph: &PathGraph, bounds: &Rect) -> f32 {
        let Some(mut e) = self.errand else { return 0.0 };
        let Some(goal) = graph.nearest_point(e.target, f32::MAX) else {
            return 0.0;
        };
        let mut dist = 0.0;
        if self.walker.pos.distance(goal.pos) <= 4.0 {
            e.arrived = true;
        } else {
            e.arrived = false;
            let moved_goal = e.planned.is_none_or(|p| p.distance(goal.pos) > 12.0);
            if moved_goal || (!self.walker.has_route() && !e.unreachable) {
                e.planned = Some(goal.pos);
                let route = graph
                    .nearest_point(self.walker.pos, 60.0)
                    .and_then(|from| plan_route(graph, from, goal));
                e.unreachable = route.is_none();
                if let Some(r) = route {
                    self.walker.set_route(r);
                }
            }
            if self.walker.has_route() {
                dist = e.speed * dt;
                if self.walker.advance(dist) {
                    e.arrived = self.walker.pos.distance(goal.pos) <= 12.0;
                    e.planned = None;
                }
                self.walker.clamp_to(bounds);
            }
        }
        self.errand = Some(e);
        dist
    }

    /// Velocidade-base (px/s) do estado atual.
    pub fn base_speed(&self, cfg: &Config) -> f32 {
        let s = &cfg.spider;
        let base = match self.brain.state() {
            SpiderState::Walking => s.walk_speed,
            SpiderState::Climbing => s.climb_speed,
            SpiderState::Fleeing if self.walker.is_climbing() => s.run_speed * 0.7,
            SpiderState::Fleeing => s.run_speed,
            _ => 0.0,
        };
        base * self.profile.speed_mul
    }

    /// Pixels por unidade local do modelo desta especie.
    pub fn px_per_unit(&self, cfg: &Config) -> f32 {
        PX_PER_UNIT * cfg.spider.scale * self.profile.size
    }

    /// Distancia do ponto `p` ate a borda do circulo clicavel mais proximo (negativa = dentro).
    pub fn hit_distance(&self, p: Vec2) -> f32 {
        self.hits
            .iter()
            .filter(|(_, r)| *r > 0.0)
            .map(|(c, r)| p.distance(*c) - r)
            .fold(f32::MAX, f32::min)
    }

    /// Recalcula os circulos clicaveis a partir da posicao/direcao atuais.
    fn update_hits(&mut self, cfg: &Config) {
        let px = self.px_per_unit(cfg);
        let r_mul = cfg.spider.hit_radius_px / REF_HIT_RADIUS_PX;
        // o dorso esta inclinado para a camera: a altura aparece encurtada na tela
        let up = cfg.spider.view_tilt_deg.to_radians().cos();
        let fwd = self.walker.heading.normalize_or(Vec2::X);
        let nrm = self.walker.normal.normalize_or(Vec2::NEG_Y);
        let at = |x: f32, y: f32| self.walker.pos + fwd * (x * px) + nrm * (y * px * up);
        let mut hits = [(Vec2::ZERO, 0.0); MAX_HITS];
        for (h, spot) in hits.iter_mut().zip(self.profile.hits) {
            if spot.z > 0.0 {
                *h = (at(spot.x, spot.y), spot.z * px * r_mul);
            }
        }
        self.hits = hits;
        let (c, r) = self.profile.bounds();
        self.body_center = at(c.x, c.y);
        self.hit_radius = r * px * r_mul;
    }

    /// Pode ser atingido agora (visivel e vivo).
    pub fn hittable(&self) -> bool {
        self.visible && self.brain.state().is_alive_free()
    }

    /// Golpe do jogador com a espada atual: tira vida; se zerar, mata (pontos + animacao).
    pub fn take_hit(&mut self, progress: &mut Progress, cfg: &Config) -> Strike {
        self.take_hit_scaled(progress, cfg, 1.0)
    }

    /// Golpe com o dano da espada multiplicado por `scale` (espada do heroi, pisao).
    pub fn take_hit_scaled(&mut self, progress: &mut Progress, cfg: &Config, scale: f32) -> Strike {
        let strike = progress.strike_scaled(&mut self.hp, &cfg.combat, scale);
        self.last_hit = Some(self.game_time);
        if strike.killed {
            self.clicked = true;
        } else {
            self.hurt_pending = true;
            self.hurt_flash = HURT_FLASH_SECS;
        }
        strike
    }

    fn reset_health(&mut self, cfg: &Config) {
        let elite = if self.elite {
            cfg.nests.elite_hp_multiplier
        } else {
            1.0
        };
        self.max_hp = cfg.combat.mob_hp * elite;
        self.hp = self.max_hp;
        self.last_hit = None;
        self.hurt_pending = false;
        self.hurt_flash = 0.0;
        self.shaken = false;
        self.stun_left = 0.0;
        self.fall_hurt = None;
        self.bite_charge = 0.0;
    }

    /// A janela em que o mob esta foi sacudida: perde a aderencia e cai. O dano e o atordoamento
    /// vem no pouso. `false` se nao havia o que derrubar (morto, invisivel ou ja em queda).
    pub fn shake_off(&mut self) -> bool {
        if !self.hittable() || self.walker.is_falling() {
            return false;
        }
        self.walker.throw_off();
        self.shaken = true;
        true
    }

    /// Pouso depois de uma sacudida: dano de queda; se sobreviver, fica atordoado.
    fn land_hard(&mut self, cfg: &Config) {
        let dmg = cfg.nests.fall_damage;
        self.hp = (self.hp - dmg).max(0.0);
        self.last_hit = Some(self.game_time);
        self.fall_hurt = Some(dmg);
        if self.hp <= 0.0 {
            self.clicked = true; // morre neste mesmo passo (pontua como abate)
        } else {
            self.hurt_flash = HURT_FLASH_SECS;
            self.stun_left = cfg.nests.stun_secs;
        }
    }
}

/// Leitura de clique: so conta cliques recebidos pela janela do OVERLAY (um clique no painel
/// nao pode matar um mob que esteja atras dele). Atinge apenas o mob mais proximo do cursor.
#[allow(clippy::too_many_arguments)]
pub fn read_click(
    mut clicks: MessageReader<MouseButtonInput>,
    overlay: Query<Entity, With<PrimaryWindow>>,
    cursor: Res<CursorState>,
    geo: Res<OverlayGeometry>,
    opts: Res<crate::overlay::RunOptions>,
    time: Res<Time<Real>>,
    cfg: Res<Config>,
    mut progress: ResMut<Progress>,
    mut fx: MessageWriter<crate::hud::HitFx>,
    mut mobs: Query<&mut Mob, Without<PendingModel>>,
    mut auto_kills_done: Local<u32>,
) {
    let overlay = overlay.single().ok();
    let pressed = clicks.read().any(|ev| {
        ev.button == MouseButton::Left
            && ev.state == ButtonState::Pressed
            && Some(ev.window) == overlay
    });
    if !geo.ready {
        return;
    }

    // gancho de teste (`--auto-kill N`): um clique simulado a cada N segundos no primeiro mob vivo
    if let Some(after) = opts.auto_kill_secs
        && time.elapsed_secs() >= after * (*auto_kills_done + 1) as f32
        && let Some(mut m) = mobs.iter_mut().find(|m| m.sim.hittable())
    {
        *auto_kills_done += 1;
        info!("--auto-kill: clique simulado");
        m.sim.clicked = true;
        return;
    }

    // gancho de teste (`--auto-hit N`): um golpe de espada simulado a cada N s no primeiro mob vivo
    let simulated = opts.auto_hit_secs.and_then(|every| {
        let due = time.elapsed_secs() >= every * (*auto_kills_done + 1) as f32;
        due.then(|| {
            *auto_kills_done += 1;
            mobs.iter()
                .find(|m| m.sim.hittable())
                .map(|m| m.sim.body_center)
        })
        .flatten()
    });
    let Some(c) = simulated.or(cursor.local.filter(|_| pressed)) else {
        return;
    };
    let target = mobs
        .iter_mut()
        .filter(|m| m.sim.hittable() && m.sim.hit_distance(c) <= 0.0)
        .min_by(|a, b| a.sim.hit_distance(c).total_cmp(&b.sim.hit_distance(c)));
    if let Some(mut m) = target {
        let sword = progress.sword;
        let strike = m.sim.take_hit(&mut progress, &cfg);
        if let Some(new) = strike.upgraded {
            info!("espada evoluiu: {} (XP {})", new.label(), progress.xp);
        }
        fx.write(crate::hud::HitFx {
            at: m.sim.body_center,
            radius: m.sim.hit_radius,
            sword,
            strike,
        });
    }
}

/// Eventos de um passo de simulacao (o ECS reage a eles: pontuacao, efeitos, logs).
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct StepEvents {
    pub placed: bool,
    pub killed: bool,
    pub respawned: bool,
}

impl SpiderSim {
    /// Um passo completo e puro: percepcao -> comportamento -> rota -> movimento.
    /// Nao depende de Bevy ECS nem de Win32, entao pode ser simulado em testes.
    pub fn step(
        &mut self,
        dt: f32,
        graph: &PathGraph,
        bounds: &Rect,
        cursor: Option<Vec2>,
        cfg: &Config,
    ) -> StepEvents {
        let mut events = StepEvents::default();
        // temporizadores (morte/respawn/ociosidade) usam o tempo real decorrido (ate 1 s, p.ex. a
        // 4 Hz com a aranha oculta); movimento/fisica usam um passo menor para nunca "teletransportar"
        let dt = dt.clamp(0.0, 1.0);
        let dt_move = dt.min(0.25);
        self.game_time += dt;

        // primeira colocacao: ponto aleatorio do grafo
        if !self.placed {
            let Self {
                walker,
                brain,
                spawn_at,
                ..
            } = self;
            // filhote nasce no ninho; os demais, num ponto aleatorio do grafo
            let at = spawn_at
                .take()
                .and_then(|p| graph.nearest_point(p, 48.0))
                .or_else(|| random_point(graph, brain.rng()));
            if let Some(p) = at {
                walker.place(p, graph);
                self.placed = true;
                self.visible = true;
                self.snap_orientation(cfg);
                self.reset_health(cfg);
                events.placed = true;
            }
            return events;
        }

        // queda livre apos a superficie sumir
        if self.walker.is_falling() {
            let landed = self.walker.update_fall(dt_move, graph).is_some();
            if landed && std::mem::take(&mut self.shaken) {
                self.land_hard(cfg);
            }
            if self.walker.pos.y > bounds.max.y + 200.0 {
                self.shaken = false;
                // caiu para fora da tela sem chao: recoloca em algum ponto do grafo
                let Self { walker, brain, .. } = self;
                if let Some(p) = random_point(graph, brain.rng()) {
                    walker.place(p, graph);
                }
            }
        }

        // percepcao -> comportamento
        // em missao (cacada do mouse) o cursor esta no fio da propria aranha: nao reage a ele
        let on_errand = self.errand.is_some();
        let cursor_dist = cursor
            .filter(|_| !on_errand)
            .map(|c| c.distance(self.body_center));
        let input = BrainInput {
            dt,
            cursor_dist,
            on_vertical: self.walker.is_climbing(),
            has_route: self.walker.has_route(),
            route_finished: self.route_finished,
            clicked: self.clicked,
            hurt: self.hurt_pending,
            falling: self.walker.is_falling(),
        };
        self.clicked = false;
        self.hurt_pending = false;
        self.hurt_flash = (self.hurt_flash - dt).max(0.0);
        self.route_finished = false;
        let out = self.brain.update(input, &cfg.behavior);
        self.stun_left = (self.stun_left - dt).max(0.0);
        let stunned = self.stun_left > 0.0;

        // elite: cursor parado por perto tempo demais leva uma mordida
        self.bite_cooldown = (self.bite_cooldown - dt).max(0.0);
        let lingering = cursor_dist.is_some_and(|d| d < cfg.nests.bite_radius_px);
        if self.elite && self.hittable() && !stunned && !self.walker.is_falling() && lingering {
            self.bite_charge += dt;
            if self.bite_charge >= cfg.nests.bite_windup_secs && self.bite_cooldown <= 0.0 {
                self.pending_bite = true;
                self.bite_charge = 0.0;
                self.bite_cooldown = cfg.nests.bite_cooldown_secs;
            }
        } else {
            self.bite_charge = 0.0;
        }

        if out.killed {
            events.killed = true;
            self.pending_burst = Some(self.body_center);
            self.walker.stop();
            self.speed_px = 0.0;
        }

        if out.spawn {
            let Self { walker, brain, .. } = self;
            if let Some(p) = random_point(graph, brain.rng()) {
                walker.place(p, graph);
                self.visible = true;
                self.snap_orientation(cfg);
                self.reset_health(cfg);
                events.respawned = true;
            }
        }

        // planejamento de rota (A* sobre o grafo)
        // (atordoado nao planeja: o cerebro volta a pedir rota sozinho depois)
        if let Some(req) = out.plan.filter(|_| !stunned && !on_errand) {
            let away = if req.flee_from_cursor { cursor } else { None };
            let planned = {
                let Self { walker, brain, .. } = self;
                graph
                    .nearest_point(walker.pos, 60.0)
                    .and_then(|from| choose_destination(graph, from, away, 140.0, brain.rng()))
                    .map(|route| walker.set_route(route))
                    .is_some()
            };
            if !planned {
                self.brain.force_idle(&cfg.behavior);
            }
        }

        // movimento
        let moving_state = matches!(
            out.state,
            SpiderState::Walking | SpiderState::Climbing | SpiderState::Fleeing
        );
        let mut dist = 0.0;
        if on_errand {
            if self.hittable() && !stunned && !self.walker.is_falling() {
                dist = self.run_errand(dt_move, graph, bounds);
            }
        } else if moving_state && self.walker.has_route() && !stunned {
            dist = self.base_speed(cfg) * out.speed_factor * dt_move;
            if self.walker.advance(dist) {
                self.route_finished = true;
            }
            self.walker.clamp_to(bounds);
        }
        self.speed_px = if dt_move > 0.0 { dist / dt_move } else { 0.0 };

        // marcha: o ciclo avanca com a distancia percorrida (sem "patinar")
        let v = self.speed_px / self.px_per_unit(cfg);
        let stride = self.profile.gait_stride(v);
        self.gait_phase =
            (self.gait_phase + gait_frequency(v, stride, self.profile.stance) * dt_move).fract();

        // orientacao suavizada em direcao ao alvo
        let target = self.target_orientation(cfg);
        self.orientation = self
            .orientation
            .slerp(target, smooth_factor(14.0, dt_move))
            .normalize();

        // regiao clicavel: circulos ao longo do corpo da especie
        self.update_hits(cfg);

        // some quando a animacao de morte termina
        self.visible = match self.brain.state() {
            SpiderState::Dead => self.brain.state_time() < cfg.behavior.death_anim_secs,
            _ => true,
        };
        events
    }

    fn target_orientation(&self, cfg: &Config) -> Quat {
        orientation(
            screen_to_world_dir(self.walker.heading),
            screen_to_world_dir(self.walker.normal),
            cfg.spider.view_tilt_deg.to_radians(),
        )
    }

    fn snap_orientation(&mut self, cfg: &Config) {
        self.orientation = self.target_orientation(cfg);
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spider_think(
    cfg: Res<Config>,
    time: Res<Time<Real>>,
    geo: Res<OverlayGeometry>,
    desktop: Res<Desktop>,
    cursor: Res<CursorState>,
    effects: Res<EffectsActive>,
    hero: Option<Res<crate::hero::HeroState>>,
    mut mobs: Query<(&mut Mob, Has<PendingModel>)>,
    mut score: ResMut<Score>,
    mut targets: ResMut<HitTargets>,
    mut activity: ResMut<Activity>,
) {
    if !geo.ready || desktop.version == 0 {
        return;
    }
    let bounds = geo.local_rect();
    let dt = time.delta_secs();
    targets.circles.clear();
    let (mut any_visible, mut any_busy) = (false, false);
    let hero_at = hero.and_then(|h| h.threat(&cfg.hero));

    for (mut mob, pending) in &mut mobs {
        let sim = &mut mob.sim;
        // ameaca: o cursor ou o heroi, o que estiver mais perto deste bicho
        let near = |p: &Vec2| p.distance(sim.body_center);
        let threat = match (cursor.local, hero_at) {
            (Some(c), Some(h)) => Some(if near(&h) < near(&c) { h } else { c }),
            (c, h) => c.or(h),
        };
        let events = sim.step(dt, &desktop.graph, &bounds, threat, &cfg);
        if events.placed || events.respawned {
            debug!(
                "mob {} em ({:.0}, {:.0})",
                if events.placed {
                    "posicionado"
                } else {
                    "reapareceu"
                },
                sim.walker.pos.x,
                sim.walker.pos.y
            );
        }
        if events.killed {
            // relogio global (cada mob tem o seu; a sequencia de abates e do jogador)
            let gained = score.register_kill(time.elapsed_secs(), cfg.game.points_per_kill);
            sim.kills_logged += 1;
            info!(
                "mob atingido! +{gained} pontos (total {}, abates {}, sequencia {})",
                score.points, score.kills, score.streak
            );
        }

        let state = sim.brain.state();
        // modelo ainda carregando: invisivel, logo nao clicavel
        if sim.hittable() && !pending {
            for &(center, radius) in sim.hits.iter().filter(|(_, r)| *r > 0.0) {
                targets.circles.push(HitCircle { center, radius });
            }
        }
        any_visible |= sim.visible;
        let near_cursor = cursor
            .local
            .is_some_and(|c| c.distance(sim.body_center) < cfg.behavior.detect_radius_px * 2.5);
        any_busy |= sim.visible && (state != SpiderState::Idle || near_cursor);
    }

    // ritmo do laco de eventos: o mob mais ativo manda
    let new_activity = if !any_visible && effects.0 == 0 {
        Activity::Hidden
    } else if !any_busy && effects.0 == 0 {
        Activity::Calm
    } else {
        Activity::Fast
    };
    if *activity != new_activity {
        *activity = new_activity;
    }
}

/// Aplica a simulacao ao `Transform`/`Visibility` de cada mob.
pub fn sync_spider_transform(
    cfg: Res<Config>,
    geo: Res<OverlayGeometry>,
    mut mobs: Query<(&Mob, &mut Transform, &mut Visibility)>,
) {
    if !geo.ready {
        return;
    }
    for (mob, mut tf, mut vis) in &mut mobs {
        let sim = &mob.sim;
        if !sim.placed {
            continue;
        }
        let state = sim.brain.state();
        let t = sim.brain.state_time();
        let mut scale_mul = 1.0;
        let mut roll = 0.0;
        match state {
            // encolhe num "tranco" rapido
            SpiderState::Hit => scale_mul = 1.0 - 0.18 * (t / 0.18).clamp(0.0, 1.0),
            SpiderState::Dead => {
                let p = (t / cfg.behavior.death_anim_secs.max(0.1)).clamp(0.0, 1.0);
                roll = sim.profile.death_roll * ease_out_cubic(p);
                scale_mul = 0.82 - 0.12 * p;
            }
            SpiderState::Respawning => scale_mul = ease_out_back(t / 0.7).max(0.01),
            _ => {}
        }
        // golpe nao letal: encolhe e volta rapido
        scale_mul *= 1.0 - 0.25 * (sim.hurt_flash / HURT_FLASH_SECS).clamp(0.0, 1.0);

        let forward = screen_to_world_dir(sim.walker.heading).normalize_or(Vec3::X);
        let w = geo.local_to_world(sim.walker.pos);
        tf.translation = Vec3::new(w.x, w.y, 0.0);
        tf.rotation = Quat::from_axis_angle(forward, roll) * sim.orientation;
        let size = if sim.elite { ELITE_SCALE } else { 1.0 };
        tf.scale = Vec3::splat(sim.px_per_unit(&cfg) * scale_mul * size);

        let want = if sim.visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
        }
    }
}

/// Dispara o efeito de morte quando a simulacao pede.
pub fn spawn_death_effect(
    mut commands: Commands,
    geo: Res<OverlayGeometry>,
    assets: Option<Res<EffectAssets>>,
    mut mobs: Query<&mut Mob>,
) {
    let Some(assets) = assets else { return };
    for mut mob in &mut mobs {
        let Some(at) = mob.sim.pending_burst.take() else {
            continue;
        };
        let w = geo.local_to_world(at);
        spawn_burst(
            &mut commands,
            &assets,
            Vec3::new(w.x, w.y, 0.0),
            mob.sim.brain.rng(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::species::Species;

    fn sim_at(species: Species, heading: Vec2, normal: Vec2) -> SpiderSim {
        let mut s = SpiderSim::with_profile(7, species.profile());
        s.walker.pos = Vec2::new(500.0, 400.0);
        s.walker.heading = heading;
        s.walker.normal = normal;
        s.update_hits(&Config::default());
        s
    }

    #[test]
    fn hit_circles_follow_heading_and_cover_head_and_rear() {
        let cfg = Config::default();
        // rato andando para a direita no chao (normal para cima na tela)
        let s = sim_at(Species::Rato, Vec2::X, Vec2::NEG_Y);
        let px = s.px_per_unit(&cfg);
        let pos = s.walker.pos;
        let head = pos + Vec2::new(1.0 * px, -0.4 * px); // focinho
        let tail_end = pos + Vec2::new(-2.0 * px, 0.0);
        assert!(s.hit_distance(head) <= 0.0, "cabeca clicavel");
        assert!(
            s.hit_distance(pos + Vec2::new(-0.5 * px, -0.4 * px)) <= 0.0,
            "traseira"
        );
        assert!(s.hit_distance(tail_end) > 0.0, "cauda nao conta");
        assert!(
            s.hit_distance(pos + Vec2::new(0.0, 3.0 * px)) > 0.0,
            "abaixo do chao"
        );

        // virado para a esquerda: a cabeca passa para o outro lado
        let l = sim_at(Species::Rato, Vec2::NEG_X, Vec2::NEG_Y);
        assert!(l.hit_distance(head) > 0.0);
        assert!(l.hit_distance(pos + Vec2::new(-1.0 * px, -0.4 * px)) <= 0.0);

        // o circulo envolvente contem todos os circulos
        for &(c, r) in s.hits.iter().filter(|(_, r)| *r > 0.0) {
            assert!(c.distance(s.body_center) + r <= s.hit_radius + 1e-3);
        }
    }

    #[test]
    fn hit_radius_config_scales_every_species() {
        let mut cfg = Config::default();
        let base = sim_at(Species::Barata, Vec2::X, Vec2::NEG_Y).hit_radius;
        cfg.spider.hit_radius_px *= 2.0;
        let mut s = SpiderSim::with_profile(7, Species::Barata.profile());
        s.walker.heading = Vec2::X;
        s.walker.normal = Vec2::NEG_Y;
        s.update_hits(&cfg);
        assert!((s.hit_radius - 2.0 * base).abs() < 1e-3);
    }
}
