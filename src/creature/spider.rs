//! Mobs como entidades do jogo: simulacao (movimento + comportamento) de cada um, sincronizacao
//! com a cena 3D e interacao com o mouse.

use bevy::prelude::*;

use bevy::input::ButtonState;
use bevy::input::mouse::MouseButtonInput;
use bevy::window::PrimaryWindow;

use super::animation::{
    BODY_HEIGHT, ease_out_back, ease_out_cubic, gait_frequency, orientation, screen_to_world_dir,
    smooth_factor,
};
use super::behavior::{Brain, BrainInput, SpiderState};
use super::effects::{EffectAssets, EffectsActive, spawn_burst};
use crate::config::Config;
use crate::desktop::Desktop;
use crate::game::Score;
use crate::navigation::geometry::Rect;
use crate::navigation::pathfinding::{choose_destination, random_point};
use crate::navigation::walker::Walker;
use crate::navigation::window_graph::PathGraph;
use crate::overlay::{Activity, CursorState, HitCircle, HitTargets, OverlayGeometry};

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
    /// Centro do corpo em coordenadas locais do overlay (px).
    pub body_center: Vec2,
    pub hit_radius: f32,
    pub game_time: f32,
    pub pending_burst: Option<Vec2>,
    pub kills_logged: u32,
}

impl SpiderSim {
    pub fn new(seed: u64) -> Self {
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
            body_center: Vec2::ZERO,
            hit_radius: 34.0,
            game_time: 0.0,
            pending_burst: None,
            kills_logged: 0,
        }
    }

    /// Velocidade-base (px/s) do estado atual.
    pub fn base_speed(&self, cfg: &Config) -> f32 {
        let s = &cfg.spider;
        match self.brain.state() {
            SpiderState::Walking => s.walk_speed,
            SpiderState::Climbing => s.climb_speed,
            SpiderState::Fleeing if self.walker.is_climbing() => s.run_speed * 0.7,
            SpiderState::Fleeing => s.run_speed,
            _ => 0.0,
        }
    }

    /// Pode ser atingido agora (visivel e vivo).
    pub fn hittable(&self) -> bool {
        self.visible && self.brain.state().is_alive_free()
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
    mut mobs: Query<&mut Mob>,
    mut auto_kills_done: Local<u32>,
) {
    let overlay = overlay.single().ok();
    let pressed = clicks.read().any(|ev| {
        ev.button == MouseButton::Left && ev.state == ButtonState::Pressed && Some(ev.window) == overlay
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

    let Some(c) = cursor.local.filter(|_| pressed) else {
        return;
    };
    let target = mobs
        .iter_mut()
        .filter(|m| m.sim.hittable() && c.distance(m.sim.body_center) <= m.sim.hit_radius)
        .min_by(|a, b| {
            c.distance(a.sim.body_center)
                .total_cmp(&c.distance(b.sim.body_center))
        });
    if let Some(mut m) = target {
        m.sim.clicked = true;
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
            let Self { walker, brain, .. } = self;
            if let Some(p) = random_point(graph, brain.rng()) {
                walker.place(p, graph);
                self.placed = true;
                self.visible = true;
                self.snap_orientation(cfg);
                events.placed = true;
            }
            return events;
        }

        // queda livre apos a superficie sumir
        if self.walker.is_falling() {
            self.walker.update_fall(dt_move, graph);
            if self.walker.pos.y > bounds.max.y + 200.0 {
                // caiu para fora da tela sem chao: recoloca em algum ponto do grafo
                let Self { walker, brain, .. } = self;
                if let Some(p) = random_point(graph, brain.rng()) {
                    walker.place(p, graph);
                }
            }
        }

        // percepcao -> comportamento
        let cursor_dist = cursor.map(|c| c.distance(self.body_center));
        let input = BrainInput {
            dt,
            cursor_dist,
            on_vertical: self.walker.is_climbing(),
            has_route: self.walker.has_route(),
            route_finished: self.route_finished,
            clicked: self.clicked,
            falling: self.walker.is_falling(),
        };
        self.clicked = false;
        self.route_finished = false;
        let out = self.brain.update(input, &cfg.behavior);

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
                events.respawned = true;
            }
        }

        // planejamento de rota (A* sobre o grafo)
        if let Some(req) = out.plan {
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
        if moving_state && self.walker.has_route() {
            dist = self.base_speed(cfg) * out.speed_factor * dt_move;
            if self.walker.advance(dist) {
                self.route_finished = true;
            }
            self.walker.clamp_to(bounds);
        }
        self.speed_px = if dt_move > 0.0 { dist / dt_move } else { 0.0 };

        // marcha: o ciclo avanca com a distancia percorrida (sem "patinar")
        let units_per_px = 1.0 / (PX_PER_UNIT * cfg.spider.scale);
        self.gait_phase =
            (self.gait_phase + gait_frequency(self.speed_px * units_per_px) * dt_move).fract();

        // orientacao suavizada em direcao ao alvo
        let target = self.target_orientation(cfg);
        self.orientation = self
            .orientation
            .slerp(target, smooth_factor(14.0, dt_move))
            .normalize();

        // regiao clicavel: centro do corpo acima da superficie
        let scale = cfg.spider.scale;
        let lift = BODY_HEIGHT * PX_PER_UNIT * scale * 0.9;
        self.body_center = self.walker.pos + self.walker.normal * lift;
        self.hit_radius = cfg.spider.hit_radius_px * scale;

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
    mut mobs: Query<&mut Mob>,
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

    for mut mob in &mut mobs {
        let sim = &mut mob.sim;
        let events = sim.step(dt, &desktop.graph, &bounds, cursor.local, &cfg);
        if events.placed || events.respawned {
            debug!(
                "mob {} em ({:.0}, {:.0})",
                if events.placed { "posicionado" } else { "reapareceu" },
                sim.walker.pos.x,
                sim.walker.pos.y
            );
        }
        if events.killed {
            let gained = score.register_kill(sim.game_time, cfg.game.points_per_kill);
            sim.kills_logged += 1;
            info!(
                "mob atingido! +{gained} pontos (total {}, abates {}, sequencia {})",
                score.points, score.kills, score.streak
            );
        }

        let state = sim.brain.state();
        if sim.hittable() {
            targets.circles.push(HitCircle {
                center: sim.body_center,
                radius: sim.hit_radius,
            });
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
                roll = std::f32::consts::PI * ease_out_cubic(p);
                scale_mul = 0.82 - 0.12 * p;
            }
            SpiderState::Respawning => scale_mul = ease_out_back(t / 0.7).max(0.01),
            _ => {}
        }

        let forward = screen_to_world_dir(sim.walker.heading).normalize_or(Vec3::X);
        let w = geo.local_to_world(sim.walker.pos);
        tf.translation = Vec3::new(w.x, w.y, 0.0);
        tf.rotation = Quat::from_axis_angle(forward, roll) * sim.orientation;
        tf.scale = Vec3::splat(PX_PER_UNIT * cfg.spider.scale * scale_mul);

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
        spawn_burst(&mut commands, &assets, Vec3::new(w.x, w.y, 0.0), mob.sim.brain.rng());
    }
}
