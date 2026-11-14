//! Sequestro do mouse no ECS: teia no canto da tela, fio de seda, cursor real arrastado pela
//! aranha, contagem regressiva para o painel. As regras estao em [`crate::game::heist`].

use bevy::input::ButtonState;
use bevy::input::mouse::MouseButtonInput;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::config::{Config, WebConfig};
use crate::creature::behavior::SpiderState;
use crate::creature::glb::PendingModel;
use crate::creature::species::Species;
use crate::creature::spider::Mob;
use crate::game::Progress;
use crate::game::heist::{
    Heist, HeistEvent, HeistInput, HeistPhase, Hunter, corner_sides, web_center,
};
use crate::hud::FloatText;
use crate::mobs::{MobCatalog, MobSource};
use crate::overlay::{Activity, CursorState, HitCircle, HitTargets, OverlayGeometry};
use crate::platform;

/// Atras dos mobs (z ~ 0) e dos ninhos (-40).
const WEB_Z: f32 = -70.0;
const THREAD_Z: f32 = -46.0;
const COLOR_GOOD: Color = Color::srgb(0.55, 1.0, 0.6);
const COLOR_BAD: Color = Color::srgb(1.0, 0.45, 0.35);
const COLOR_SILK: Color = Color::srgb(0.92, 0.94, 1.0);
/// Atordoamento da aranha quando o jogador corta o fio (s).
const CUT_STUN_SECS: f32 = 2.5;

#[derive(Resource)]
pub struct HeistState {
    pub heist: Heist,
    pub hunter: Option<Entity>,
    /// Ultimo destino mandado para a cacadora ("chegou" so vale para ele).
    sent_goal: Option<Vec2>,
    /// Golpes de espada do heroi neste quadro (ponta da espada, dano): valem como cliques, entao
    /// o heroi corta o fio e arranca a teia pelo teclado, mesmo com o mouse preso.
    pub hero_swings: Vec<(Vec2, f32)>,
}

impl HeistState {
    pub fn new(seed: u64, cfg: &Config) -> Self {
        Self {
            heist: Heist::new(seed, &cfg.web),
            hunter: None,
            sent_goal: None,
            hero_swings: Vec::new(),
        }
    }
}

/// Linha de estado da cacada para o painel (vazia = recurso desligado).
#[derive(Resource, Default, Debug)]
pub struct HeistHud {
    pub line: String,
}

#[derive(Resource)]
pub struct WebAssets {
    quad: Handle<Mesh>,
    silk: Handle<StandardMaterial>,
    faint: Handle<StandardMaterial>,
    hot: Handle<StandardMaterial>,
}

/// Teia fixa do canto. Guarda com que parametros foi montada (refaz se mudarem).
#[derive(Component, PartialEq, Clone, Copy)]
pub struct CornerWeb {
    size: UVec2,
    sides: (bool, bool),
    radius: f32,
    /// Area util usada (cantos arredondados para px inteiros).
    area: (IVec2, IVec2),
}

/// Raiz dos fios da teia do canto (cresce do canto conforme `Heist::web_built`).
#[derive(Component)]
pub struct WebRoot;

/// Fios que embrulham o cursor preso.
#[derive(Component)]
pub struct WebWrap;

/// O fio entre a aranha e o cursor.
#[derive(Component)]
pub struct SilkThread;

/// Fio de seda que serve de ponte entre janelas (um por `PathGraph::threads`).
#[derive(Component)]
pub struct BridgeThread;

/// Redesenha as pontes de seda sempre que o grafo de janelas muda.
pub fn sync_bridge_threads(
    mut commands: Commands,
    geo: Res<OverlayGeometry>,
    desktop: Res<crate::desktop::Desktop>,
    assets: Option<Res<WebAssets>>,
    old: Query<Entity, With<BridgeThread>>,
    mut seen: Local<Option<u64>>,
) {
    let Some(assets) = assets else { return };
    if !geo.ready || *seen == Some(desktop.version) {
        return;
    }
    *seen = Some(desktop.version);
    for e in &old {
        commands.entity(e).despawn();
    }
    for s in &desktop.graph.threads {
        commands.spawn((
            BridgeThread,
            Mesh3d(assets.quad.clone()),
            MeshMaterial3d(assets.faint.clone()),
            strand(
                geo.local_to_world(s.a),
                geo.local_to_world(s.b),
                1.5,
                THREAD_Z - 6.0,
            ),
        ));
    }
}

/// Faixa fina de `a` ate `b` (coordenadas de mundo) feita com o quadrado unitario.
fn strand(a: Vec2, b: Vec2, width: f32, z: f32) -> Transform {
    let d = b - a;
    Transform {
        translation: ((a + b) * 0.5).extend(z),
        rotation: Quat::from_rotation_z(d.y.atan2(d.x)),
        scale: Vec3::new(d.length().max(0.01), width, 1.0),
    }
}

pub fn setup_web(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let flat = |c: Color| StandardMaterial {
        base_color: c,
        unlit: true,
        cull_mode: None,
        ..default()
    };
    let assets = WebAssets {
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
        silk: materials.add(flat(COLOR_SILK)),
        faint: materials.add(flat(Color::srgb(0.70, 0.73, 0.82))),
        hot: materials.add(flat(Color::srgb(1.0, 0.35, 0.3))),
    };
    commands.spawn((
        SilkThread,
        Mesh3d(assets.quad.clone()),
        MeshMaterial3d(assets.silk.clone()),
        Transform::default(),
        Visibility::Hidden,
    ));
    commands.insert_resource(assets);
    info!("janelas: {}", platform::window_source());
    if !platform::can_move_cursor() {
        warn!("cursor real nao pode ser movido aqui: o roubo do mouse fica desligado");
    }
}

/// So aranhas roubam o mouse.
fn is_spider(kind: usize, catalog: &MobCatalog) -> bool {
    match catalog.kinds.get(kind).map(|k| (&k.source, &k.label)) {
        Some((MobSource::Procedural(s), _)) => matches!(s, Species::Viuva | Species::AranhaLobo),
        Some((MobSource::Glb(path), label)) => {
            let t = format!("{path} {label}").to_lowercase();
            t.contains("spider") || t.contains("aranha")
        }
        None => false,
    }
}

fn alive(mob: &Mob) -> bool {
    mob.sim.placed
        && mob.sim.visible
        && !matches!(
            mob.sim.brain.state(),
            SpiderState::Dead | SpiderState::Respawning
        )
}

fn hud_line(h: &Heist, c: &WebConfig, can_move: bool, has_spider: bool) -> String {
    let max_trap = c.max_trap_secs;
    if !c.enabled {
        return String::new();
    }
    if !can_move {
        return "Roubo do mouse: indisponivel".into();
    }
    match h.phase {
        HeistPhase::Waiting if h.web_built < 1.0 && !has_spider => "Sem teia e sem aranha".into(),
        HeistPhase::Waiting if h.web_built < 1.0 => {
            let s = h.rebuild_left.min(h.countdown).ceil() as u32;
            format!("Sem teia: aranha refaz em {}:{:02}", s / 60, s % 60)
        }
        HeistPhase::Weaving => format!("Aranha tecendo a teia: {:.0}%", h.web_built * 100.0),
        HeistPhase::Waiting if !has_spider => "Roubo do mouse: sem aranha".into(),
        HeistPhase::Waiting => {
            let s = h.countdown.ceil() as u32;
            if h.rest_left > 0.0 && h.rest_left >= c.idle_secs - h.still {
                format!("Aranha descansando: {}:{:02}", s / 60, s % 60)
            } else if h.still < 1.0 {
                format!("Aranha rouba o mouse se parar {:.0}s", c.idle_secs)
            } else {
                format!("Mouse parado! Aranha vem em {}:{:02}", s / 60, s % 60)
            }
        }
        HeistPhase::Hooking { .. } => "A aranha lancou o fio!".into(),
        HeistPhase::Trapped { elapsed } => format!(
            "Mouse na teia! Clique: {:.0}/{:.0} ({:.0}s)",
            h.web_hp.ceil(),
            h.web_max,
            (max_trap - elapsed).max(0.0).ceil()
        ),
        _ => format!(
            "Fio no mouse! Clique: {:.0}/{:.0}",
            h.thread_hp.max(0.0).ceil(),
            h.thread_max
        ),
    }
}

/// Um quadro da cacada: escolhe a aranha, roda as regras, move o cursor real, captura o mouse,
/// trata cliques (cortar o fio / destruir a teia) e publica a linha do painel.
#[allow(clippy::too_many_arguments)]
pub fn heist_update(
    cfg: Res<Config>,
    time: Res<Time<Real>>,
    geo: Res<OverlayGeometry>,
    cursor: Res<CursorState>,
    catalog: Res<MobCatalog>,
    overlay: Query<Entity, With<PrimaryWindow>>,
    mut clicks: MessageReader<MouseButtonInput>,
    mut mobs: Query<(Entity, &mut Mob), Without<PendingModel>>,
    mut state: ResMut<HeistState>,
    mut hud: ResMut<HeistHud>,
    mut targets: ResMut<HitTargets>,
    mut progress: ResMut<Progress>,
    mut float: MessageWriter<FloatText>,
    mut activity: ResMut<Activity>,
    desktop: Res<crate::desktop::Desktop>,
) {
    let overlay = overlay.single().ok();
    let pressed = clicks.read().any(|ev| {
        ev.button == MouseButton::Left
            && ev.state == ButtonState::Pressed
            && Some(ev.window) == overlay
    });
    let c = &cfg.web;
    if !geo.ready {
        return;
    }
    // (a primeira chamada cria o mouse virtual no Linux, bem antes da primeira cacada)
    let can_move = c.enabled && platform::can_move_cursor();
    let screen = geo.local_rect();
    // (canto da area util: a teia nao fica em cima da barra do sistema)
    let arena = platform::corner_area(&c.corner, screen, &desktop.work_areas);
    let web = web_center(&c.corner, &arena, c);
    let state = &mut *state;

    // cacadora: mantem a atual enquanto viva; esperando, prefere a aranha mais perto do cursor
    let valid = |e: Entity, mobs: &Query<(Entity, &mut Mob), Without<PendingModel>>| {
        mobs.get(e).is_ok_and(|(_, m)| alive(m))
    };
    if state.hunter.is_some_and(|e| !valid(e, &mobs)) {
        state.hunter = None;
    }
    if !state.heist.active() {
        let near = cursor.local.unwrap_or(screen.center());
        state.hunter = mobs
            .iter()
            .filter(|(_, m)| alive(m) && is_spider(m.kind, &catalog))
            .min_by(|a, b| {
                let d = |m: &Mob| m.sim.body_center.distance(near);
                d(a.1).total_cmp(&d(b.1))
            })
            .map(|(e, _)| e);
    }
    let has_spider = state.hunter.is_some();
    let hunter_view = state
        .hunter
        .filter(|_| can_move)
        .and_then(|e| mobs.get(e).ok())
        .map(|(_, m)| Hunter {
            pos: m.sim.body_center,
            arrived: m.sim.errand.is_some_and(|e| {
                (e.arrived || e.unreachable)
                    && state.sent_goal.is_some_and(|g| g.distance(e.target) < 0.5)
            }),
        });

    let mut out = state.heist.update(
        HeistInput {
            dt: time.delta_secs(),
            hunter: hunter_view,
            cursor: cursor.local,
            screen,
            web_center: web,
            // sem leitura confiavel do ponteiro, vale o tempo ocioso informado pelo sistema
            idle_hint: if platform::pointer_tracking_live() {
                None
            } else {
                platform::input_idle_secs()
            },
        },
        c,
    );

    // clique (ou golpe do heroi): espada no fio (cursor pendurado) ou na teia do canto
    let mut strikes = std::mem::take(&mut state.hero_swings);
    if pressed && let Some(p) = cursor.local {
        strikes.push((p, progress.sword.damage(&cfg.combat)));
    }
    for (p, damage) in strikes {
        let before = (state.heist.thread_hp, state.heist.web_hp);
        match state.heist.click(p, damage, web, c) {
            Some(ev) => out.events.push(ev),
            None if before != (state.heist.thread_hp, state.heist.web_hp) => {
                float.write(FloatText {
                    at: p + Vec2::new(0.0, -18.0),
                    text: format!("-{damage:.0}"),
                    color: COLOR_SILK,
                    size: 18.0,
                });
            }
            None => {}
        }
    }
    let active = state.heist.active();
    let at = cursor.local.unwrap_or(state.heist.bob);
    let mut say = |text: &str, color: Color| {
        float.write(FloatText {
            at: at + Vec2::new(0.0, -26.0),
            text: text.to_string(),
            color,
            size: 20.0,
        });
    };
    let reward = |progress: &mut Progress| {
        if let Some(new) = progress.add_xp(c.xp_reward, &cfg.combat) {
            info!("espada evoluiu: {} (XP {})", new.label(), progress.xp);
        }
    };
    let mut stun = false;
    for ev in &out.events {
        match ev {
            HeistEvent::Started => info!("cacada: a aranha lancou o fio no cursor"),
            HeistEvent::Hooked => {
                info!("cacada: cursor fisgado");
                say("Peguei seu mouse!", COLOR_BAD);
            }
            HeistEvent::ThreadCut => {
                info!("cacada: fio cortado, cursor livre (+{} XP)", c.xp_reward);
                reward(&mut progress);
                say(&format!("Fio cortado! +{} XP", c.xp_reward), COLOR_GOOD);
                stun = true;
            }
            HeistEvent::HunterLost => {
                info!("cacada: a aranha se foi, cursor livre");
                say("Livre!", COLOR_GOOD);
            }
            HeistEvent::Trapped => {
                info!("cacada: cursor preso na teia");
                say("Preso na teia! Clique nela", COLOR_BAD);
            }
            HeistEvent::WebDestroyed => {
                info!("teia arrancada (+{} XP)", c.xp_reward);
                reward(&mut progress);
                say(&format!("Teia arrancada! +{} XP", c.xp_reward), COLOR_GOOD);
            }
            HeistEvent::WebRebuilt => info!("a aranha teceu uma teia nova no canto"),
            HeistEvent::Timeout => {
                info!("cacada: a teia soltou o cursor sozinha");
                say("A teia soltou", COLOR_SILK);
            }
            HeistEvent::NoCursor => info!("cacada cancelada: posicao do cursor desconhecida"),
            HeistEvent::Escaped => {
                info!("cacada: o mouse se mexeu, a aranha errou o bote");
                say("Errou!", COLOR_GOOD);
            }
        }
    }

    // ordens para a cacadora; ninguem mais fica com ordem pendurada
    let holding = active && !state.heist.is_trapped();
    for (entity, mut mob) in &mut mobs {
        if Some(entity) == state.hunter && holding {
            let (to, speed) = out.goal.unwrap_or((mob.sim.walker.pos, 1.0));
            mob.sim.send_to(to, speed);
            state.sent_goal = out.goal.map(|g| g.0);
        } else if mob.sim.errand.is_some() {
            mob.sim.end_errand();
            if stun && Some(entity) == state.hunter {
                mob.sim.stun_left = CUT_STUN_SECS;
            }
        }
    }
    if !holding {
        state.sent_goal = None;
    }

    // cursor real
    if active && let Some(w) = out.warp {
        platform::set_cursor_pos(geo.origin.as_vec2() + w);
    }
    let capture = active && out.capture;
    if targets.capture_all != capture {
        targets.capture_all = capture;
    }
    // a teia do canto e clicavel enquanto existir (da para arrancar a golpes)
    if c.enabled && state.heist.web_built > 0.0 {
        targets.circles.push(HitCircle {
            center: web,
            radius: c.radius_px,
        });
    }
    if (active || state.heist.countdown < 1.5) && *activity != Activity::Fast {
        *activity = Activity::Fast;
    }

    let line = hud_line(&state.heist, c, can_move, has_spider);
    if hud.line != line {
        hud.line = line;
    }
}

/// Pontos (em relacao ao canto, eixos "para dentro da tela") dos fios da teia: raios e aneis
/// com barriga, como uma teia de canto de parede.
fn corner_web_strands(reach: f32) -> Vec<(Vec2, Vec2, bool)> {
    const SPOKES: usize = 7;
    const RINGS: [f32; 6] = [0.16, 0.30, 0.45, 0.61, 0.78, 0.95];
    let dir = |i: usize| {
        let a = i as f32 / (SPOKES - 1) as f32 * std::f32::consts::FRAC_PI_2;
        Vec2::new(a.cos(), a.sin())
    };
    // raios de comprimentos um pouco diferentes: teia de verdade nao e regua
    let len = |i: usize| reach * [1.0, 0.93, 1.04, 0.97, 1.05, 0.92, 1.0][i % SPOKES];
    let mut v = Vec::new();
    for i in 0..SPOKES {
        v.push((Vec2::ZERO, dir(i) * len(i), true));
    }
    for (k, ring) in RINGS.iter().enumerate() {
        for i in 0..SPOKES - 1 {
            let a = dir(i) * len(i) * *ring;
            let b = dir(i + 1) * len(i + 1) * *ring;
            // barriga em direcao ao canto, maior nos aneis de fora
            let mid = (a + b) * 0.5 * (0.93 - 0.012 * k as f32);
            v.push((a, mid, false));
            v.push((mid, b, false));
        }
    }
    v
}

/// Desenha a teia do canto, o fio ate o cursor e o embrulho do cursor preso.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn sync_web_visuals(
    mut commands: Commands,
    cfg: Res<Config>,
    geo: Res<OverlayGeometry>,
    time: Res<Time<Real>>,
    cursor: Res<CursorState>,
    state: Res<HeistState>,
    assets: Option<Res<WebAssets>>,
    mobs: Query<&Mob>,
    webs: Query<(Entity, &CornerWeb)>,
    desktop: Res<crate::desktop::Desktop>,
    mut roots: Query<
        (&mut Transform, &mut Visibility),
        (With<WebRoot>, Without<WebWrap>, Without<SilkThread>),
    >,
    mut wraps: Query<
        (&mut Transform, &mut Visibility),
        (With<WebWrap>, Without<WebRoot>, Without<SilkThread>),
    >,
    mut threads: Query<
        (
            &mut Transform,
            &mut Visibility,
            &mut MeshMaterial3d<StandardMaterial>,
        ),
        With<SilkThread>,
    >,
) {
    let Some(assets) = assets else { return };
    if !geo.ready {
        return;
    }
    let c = &cfg.web;
    let sides = corner_sides(&c.corner);
    let screen = platform::corner_area(&c.corner, geo.local_rect(), &desktop.work_areas);

    // teia do canto: montada uma vez; refeita se a tela, a area util ou a configuracao mudarem
    let want = CornerWeb {
        size: geo.size,
        sides,
        radius: c.radius_px,
        area: (screen.min.round().as_ivec2(), screen.max.round().as_ivec2()),
    };
    let current = webs.iter().next();
    if current.is_some_and(|(_, w)| *w != want) || (current.is_some() && !c.enabled) {
        for (e, _) in &webs {
            commands.entity(e).despawn();
        }
    } else if current.is_none() && c.enabled {
        let corner_local = Vec2::new(
            if sides.0 { screen.max.x } else { screen.min.x },
            if sides.1 { screen.max.y } else { screen.min.y },
        );
        let corner = geo.local_to_world(corner_local);
        // para dentro da tela, em coordenadas de mundo (Y para cima)
        let inward = Vec2::new(
            if sides.0 { -1.0 } else { 1.0 },
            if sides.1 { 1.0 } else { -1.0 },
        );
        let center = geo.local_to_world(web_center(&c.corner, &screen, c));
        commands
            .spawn((
                WebRoot,
                want,
                Transform::from_translation(corner.extend(WEB_Z)),
                Visibility::Inherited,
            ))
            .with_children(|p| {
                for (a, b, spoke) in corner_web_strands(c.radius_px * 2.3) {
                    p.spawn((
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(if spoke {
                            assets.silk.clone()
                        } else {
                            assets.faint.clone()
                        }),
                        strand(a * inward, b * inward, if spoke { 1.8 } else { 1.2 }, 0.0),
                    ));
                }
            });
        // embrulho do cursor preso: fios cruzados sobre o centro da teia
        commands
            .spawn((
                WebWrap,
                want,
                Transform::from_translation(center.extend(WEB_Z + 2.0)),
                Visibility::Hidden,
            ))
            .with_children(|p| {
                let r = c.radius_px * 0.42;
                for i in 0..7 {
                    let a = i as f32 * 0.449 * std::f32::consts::PI;
                    let off = Vec2::from_angle(a + 1.3) * r * 0.25;
                    p.spawn((
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(assets.silk.clone()),
                        strand(
                            off - Vec2::from_angle(a) * r,
                            off + Vec2::from_angle(a) * r,
                            2.2,
                            0.0,
                        ),
                    ));
                }
            });
    }

    let h = &state.heist;
    let t = time.elapsed_secs();
    let hit = (h.flash / 0.15).clamp(0.0, 1.0);
    // teia do canto: some quando arrancada, cresce do canto enquanto a aranha tece, treme no golpe
    for (mut tf, mut vis) in &mut roots {
        let want_vis = if h.web_built > 0.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want_vis {
            *vis = want_vis;
        }
        let scale = Vec3::splat(h.web_built.clamp(0.0, 1.0) * (1.0 - 0.07 * hit));
        let rotation = Quat::from_rotation_z(0.035 * hit * (t * 90.0).sin());
        if tf.scale != scale || tf.rotation != rotation {
            tf.scale = scale;
            tf.rotation = rotation;
        }
    }
    for (mut tf, mut vis) in &mut wraps {
        let want_vis = if h.is_trapped() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want_vis {
            *vis = want_vis;
        }
        if h.is_trapped() {
            tf.scale = Vec3::splat(1.0 + 0.05 * (t * 5.0).sin() - 0.2 * hit);
            tf.rotation = Quat::from_rotation_z(0.08 * (t * 2.3).sin());
        }
    }

    // fio da aranha ate o cursor
    let hunter = state
        .hunter
        .and_then(|e| mobs.get(e).ok())
        .map(|m| m.sim.body_center);
    let line = hunter.and_then(|p| h.thread(p, cursor.local));
    for (mut tf, mut vis, mut material) in &mut threads {
        let want_vis = if line.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want_vis {
            *vis = want_vis;
        }
        if let Some((a, b)) = line {
            // fio quase rompendo treme e fica vermelho
            let weak = h.has_cursor() && h.thread_hp <= h.thread_max * 0.35;
            let width = if weak {
                1.6 + (t * 40.0).sin().abs()
            } else {
                2.0
            };
            *tf = strand(
                geo.local_to_world(a),
                geo.local_to_world(b),
                width,
                THREAD_Z,
            );
            let want_mat = if h.flash > 0.0 || weak {
                &assets.hot
            } else {
                &assets.silk
            };
            if material.0 != *want_mat {
                material.0 = want_mat.clone();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corner_web_fans_out_from_the_corner_inside_its_quadrant() {
        let strands = corner_web_strands(160.0);
        assert_eq!(strands.iter().filter(|s| s.2).count(), 7, "raios");
        assert!(strands.iter().filter(|s| !s.2).count() >= 60, "aneis");
        for (a, b, _) in &strands {
            for p in [a, b] {
                assert!(p.x >= -0.01 && p.y >= -0.01, "fora do quadrante: {p}");
                assert!(p.length() <= 160.0 * 1.06);
            }
        }
    }

    #[test]
    fn panel_line_follows_the_hunt() {
        let cfg = WebConfig::default(); // idle_secs = 20
        let off = WebConfig {
            enabled: false,
            ..cfg.clone()
        };
        let mut h = Heist::new(1, &cfg);
        assert_eq!(hud_line(&h, &off, true, true), "");
        assert!(hud_line(&h, &cfg, false, true).contains("indisponivel"));
        assert!(hud_line(&h, &cfg, true, false).contains("sem aranha"));
        // mouse mexendo: mostra a regra; parado: contagem regressiva
        assert_eq!(
            hud_line(&h, &cfg, true, true),
            "Aranha rouba o mouse se parar 20s"
        );
        h.still = 7.5;
        h.countdown = 12.5;
        assert_eq!(
            hud_line(&h, &cfg, true, true),
            "Mouse parado! Aranha vem em 0:13"
        );
        h.rest_left = 75.2;
        h.countdown = 75.2;
        assert_eq!(hud_line(&h, &cfg, true, true), "Aranha descansando: 1:16");
        h.rest_left = 0.0;
        h.web_built = 0.0;
        h.rebuild_left = 20.0;
        h.countdown = 30.0;
        assert_eq!(
            hud_line(&h, &cfg, true, true),
            "Sem teia: aranha refaz em 0:20"
        );
        h.phase = HeistPhase::Weaving;
        h.web_built = 0.4;
        assert_eq!(hud_line(&h, &cfg, true, true), "Aranha tecendo a teia: 40%");
        h.web_built = 1.0;
        h.phase = HeistPhase::Carrying;
        assert!(hud_line(&h, &cfg, true, true).starts_with("Fio no mouse!"));
        h.phase = HeistPhase::Trapped { elapsed: 4.0 };
        let line = hud_line(&h, &cfg, true, true);
        assert!(
            line.starts_with("Mouse na teia!") && line.contains("(26s)"),
            "{line}"
        );
    }
}
