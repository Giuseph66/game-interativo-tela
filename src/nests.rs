//! Ninhos e sacudida de janela no ECS (as regras, puras, ficam em [`crate::game::nests`]):
//!
//! * mob em repouso numa janela parada constroi um ninho; o ninho choca filhotes;
//! * clicar no ninho = golpe de espada; destruir rende XP extra;
//! * sacudir a janela derruba os mobs dela (dano de queda + atordoamento) e racha o ninho;
//! * elite (filhote de ninho velho) morde o cursor que fica parado por perto e rouba XP.

use bevy::input::ButtonState;
use bevy::input::mouse::MouseButtonInput;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::config::Config;
use crate::creature::behavior::SpiderState;
use crate::creature::effects::{EffectAssets, spawn_burst};
use crate::creature::glb::PendingModel;
use crate::creature::spider::Mob;
use crate::desktop::Desktop;
use crate::game::nests::NEST_FLASH_SECS;
use crate::game::{NestField, Progress};
use crate::hud::{FloatText, HitFx, HudRoot, health_color};
use crate::mobs::{MobCatalog, MobCommand, spawn_mob};
use crate::navigation::window_graph::SurfaceOwner;
use crate::overlay::{Activity, CursorState, HitCircle, HitTargets, OverlayGeometry, RunOptions};

/// Os ninhos ficam atras dos mobs (camera em +Z olhando para -Z).
const NEST_Z: f32 = -40.0;
const BAR_W: f32 = 64.0;
const BAR_H: f32 = 7.0;
// Formas do ninho no espaco do casulo: esfera principal de raio 0,5; +Y aponta para fora da
// superficie, que fica em y = SURFACE_Y.
const SURFACE_Y: f32 = -0.4;
/// Gomos de seda que tiram a cara de "bola": (centro, escala).
const LUMPS: [(Vec3, Vec3); 3] = [
    (Vec3::new(0.0, -0.24, -0.05), Vec3::new(1.28, 0.58, 0.9)), // base larga colada na borda
    (Vec3::new(0.12, 0.30, 0.02), Vec3::new(0.56, 0.56, 0.56)),
    (Vec3::new(-0.26, 0.10, 0.05), Vec3::new(0.50, 0.50, 0.50)),
];
/// Voltas de fio por cima do casulo (a, b).
const BANDS: [(Vec2, Vec2); 5] = [
    (Vec2::new(-0.43, 0.10), Vec2::new(0.40, 0.28)),
    (Vec2::new(-0.40, -0.12), Vec2::new(0.45, 0.02)),
    (Vec2::new(-0.30, 0.34), Vec2::new(0.34, -0.30)),
    (Vec2::new(-0.44, -0.27), Vec2::new(0.38, -0.20)),
    (Vec2::new(-0.12, 0.44), Vec2::new(0.30, 0.36)),
];
/// Fios que amarram o casulo na superficie (do casulo ate a borda), espelhados em X.
const ANCHORS: [(Vec2, Vec2); 4] = [
    (Vec2::new(0.16, 0.42), Vec2::new(1.75, SURFACE_Y)),
    (Vec2::new(0.32, 0.22), Vec2::new(1.25, SURFACE_Y)),
    (Vec2::new(0.42, 0.02), Vec2::new(0.88, SURFACE_Y)),
    (Vec2::new(0.05, 0.48), Vec2::new(0.55, 1.05)), // ponta solta para cima, presa na teia
];

/// Teia em leque atras do casulo: raios saindo da base e arcos entre eles.
fn nest_web_strands() -> Vec<(Vec2, Vec2)> {
    const SPOKES: [f32; 7] = [8.0, 32.0, 60.0, 90.0, 120.0, 148.0, 172.0];
    let base = Vec2::new(0.0, SURFACE_Y);
    let tip = |i: usize, r: f32| base + Vec2::from_angle(SPOKES[i].to_radians()) * r;
    let reach = |i: usize| [1.55, 1.3, 1.45, 1.6, 1.4, 1.35, 1.5][i];
    let mut v = Vec::new();
    for i in 0..SPOKES.len() {
        v.push((base, tip(i, reach(i))));
    }
    for ring in [0.45, 0.72, 0.95] {
        for i in 0..SPOKES.len() - 1 {
            let (a, b) = (tip(i, reach(i) * ring), tip(i + 1, reach(i + 1) * ring));
            let mid = base + ((a + b) * 0.5 - base) * 0.9; // barriga
            v.push((a, mid));
            v.push((mid, b));
        }
    }
    v
}

/// Faixa fina de `a` ate `b` no plano `z` (quadrado unitario esticado).
fn strand(a: Vec2, b: Vec2, width: f32, z: f32) -> Transform {
    let d = b - a;
    Transform {
        translation: ((a + b) * 0.5).extend(z),
        rotation: Quat::from_rotation_z(d.y.atan2(d.x)),
        scale: Vec3::new(d.length().max(0.001), width, 1.0),
    }
}

const COLOR_NEST: Color = Color::srgb(0.93, 0.90, 0.80);
const COLOR_WARN: Color = Color::srgb(1.0, 0.35, 0.30);
const COLOR_SHAKE: Color = Color::srgb(0.55, 0.85, 1.0);

#[derive(Resource)]
pub struct NestAssets {
    mesh: Handle<Mesh>,
    silk: Handle<StandardMaterial>,
    elite: Handle<StandardMaterial>,
    flash: Handle<StandardMaterial>,
    quad: Handle<Mesh>,
    /// Seda mais escura (gomos na sombra e voltas de fio).
    shade: Handle<StandardMaterial>,
    crease: Handle<StandardMaterial>,
    thread: Handle<StandardMaterial>,
    thread_faint: Handle<StandardMaterial>,
}

/// Casulo desenhado para o ninho `id`.
#[derive(Component)]
pub struct NestVisual {
    id: u32,
}

/// Barra de vida do ninho `id` (interface do overlay).
#[derive(Component)]
pub struct NestBar {
    id: u32,
}

#[derive(Component)]
pub struct NestBarFill;

/// Mob recem-chocado: a simulacao dele ainda precisa saber onde nasce e se e elite.
#[derive(Component)]
pub struct Hatchling {
    at: Vec2,
    elite: bool,
}

pub fn setup_nests(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let matte = |c: Color| StandardMaterial {
        base_color: c,
        perceptual_roughness: 0.95,
        ..default()
    };
    let flat = |c: Color| StandardMaterial {
        base_color: c,
        unlit: true,
        cull_mode: None,
        ..default()
    };
    commands.insert_resource(NestAssets {
        mesh: meshes.add(Sphere::new(0.5).mesh().uv(20, 12)),
        silk: materials.add(matte(Color::srgb(0.90, 0.88, 0.82))),
        elite: materials.add(matte(Color::srgb(0.30, 0.09, 0.11))),
        flash: materials.add(StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            ..default()
        }),
        quad: meshes.add(Rectangle::new(1.0, 1.0)),
        shade: materials.add(matte(Color::srgb(0.78, 0.76, 0.70))),
        crease: materials.add(flat(Color::srgb(0.60, 0.58, 0.53))),
        thread: materials.add(flat(Color::srgb(0.92, 0.94, 1.0))),
        thread_faint: materials.add(flat(Color::srgb(0.70, 0.73, 0.82))),
    });
}

/// Entrega ao filhote os dados do ninho antes do primeiro passo de simulacao dele; "Remover
/// todos" do painel tambem limpa os ninhos.
pub fn apply_hatchlings(
    mut commands: Commands,
    mut newborn: Query<(Entity, &mut Mob, &Hatchling)>,
    mut cmds: MessageReader<MobCommand>,
    mut field: ResMut<NestField>,
) {
    for (entity, mut mob, h) in &mut newborn {
        mob.sim.spawn_at = Some(h.at);
        mob.sim.brood = true;
        mob.sim.elite = h.elite;
        commands.entity(entity).remove::<Hatchling>();
    }
    if cmds.read().any(|c| *c == MobCommand::RemoveAll) && !field.nests.is_empty() {
        field.clear();
        info!("ninhos removidos");
    }
}

/// Clique (ou golpe simulado de `--auto-hit`) num ninho: golpe com a espada atual.
#[allow(clippy::too_many_arguments)]
pub fn nest_clicks(
    mut commands: Commands,
    mut clicks: MessageReader<MouseButtonInput>,
    overlay: Query<Entity, With<PrimaryWindow>>,
    cursor: Res<CursorState>,
    geo: Res<OverlayGeometry>,
    cfg: Res<Config>,
    opts: Res<RunOptions>,
    time: Res<Time<Real>>,
    effects: Option<Res<EffectAssets>>,
    mobs: Query<&Mob, Without<PendingModel>>,
    mut field: ResMut<NestField>,
    mut progress: ResMut<Progress>,
    mut fx: MessageWriter<HitFx>,
    mut auto_hits_done: Local<u32>,
) {
    let overlay = overlay.single().ok();
    let pressed = clicks.read().any(|ev| {
        ev.button == MouseButton::Left
            && ev.state == ButtonState::Pressed
            && Some(ev.window) == overlay
    });
    if !geo.ready || !cfg.nests.enabled {
        return;
    }
    let c = &cfg.nests;

    // gancho de teste (`--auto-hit N`): a meio caminho entre dois golpes simulados em mobs, um
    // golpe no primeiro ninho exposto
    let simulated = opts.auto_hit_secs.and_then(|every| {
        let due = time.elapsed_secs() >= every * (*auto_hits_done as f32 + 1.5);
        due.then(|| {
            *auto_hits_done += 1;
            field.nests.iter().find(|n| n.exposed).map(|n| n.id)
        })
        .flatten()
    });
    let target = simulated.or_else(|| {
        let p = cursor.local.filter(|_| pressed)?;
        // um mob sob o cursor tem prioridade: o golpe e dele (tratado em `read_click`)
        let on_mob = mobs
            .iter()
            .any(|m| m.sim.hittable() && p.distance(m.sim.body_center) <= m.sim.hit_radius);
        if on_mob { None } else { field.nest_at(p, c) }
    });
    let Some(id) = target else {
        return;
    };
    let sword = progress.sword;
    let Some(hit) = field.strike(id, &mut progress, &cfg.combat, c) else {
        return;
    };
    if let Some(new) = hit.strike.upgraded {
        info!("espada evoluiu: {} (XP {})", new.label(), progress.xp);
    }
    if hit.destroyed {
        info!(
            "ninho destruido! +{} XP (total {})",
            hit.strike.xp_gained, progress.xp
        );
        if let Some(effects) = &effects {
            let w = geo.local_to_world(hit.at);
            spawn_burst(
                &mut commands,
                effects,
                Vec3::new(w.x, w.y, 0.0),
                field.rng(),
            );
        }
    }
    fx.write(HitFx {
        at: hit.at,
        radius: hit.radius,
        sword,
        strike: hit.strike,
    });
}

/// O coracao da mecanica, uma vez por quadro, depois da simulacao dos mobs: sacudidas, pousos,
/// mordidas, construcao, eclosao, alvos clicaveis e ritmo do laco.
#[allow(clippy::too_many_arguments)]
pub fn nest_update(
    mut commands: Commands,
    cfg: Res<Config>,
    time: Res<Time<Real>>,
    geo: Res<OverlayGeometry>,
    desktop: Res<Desktop>,
    cursor: Res<CursorState>,
    asset_server: Res<AssetServer>,
    mut catalog: ResMut<MobCatalog>,
    mut field: ResMut<NestField>,
    mut progress: ResMut<Progress>,
    mut mobs: Query<(Entity, &mut Mob)>,
    mut texts: MessageWriter<FloatText>,
    mut targets: ResMut<HitTargets>,
    mut activity: ResMut<Activity>,
    mut seen_graph: Local<u64>,
) {
    if !geo.ready || desktop.version == 0 || !cfg.nests.enabled {
        return;
    }
    let c = &cfg.nests;
    let now = time.elapsed_secs();
    let dt = time.delta_secs().clamp(0.0, 1.0);
    let mut say = |at: Vec2, text: String, color: Color, size: f32| {
        texts.write(FloatText {
            at,
            text,
            color,
            size,
        });
    };

    if *seen_graph != desktop.version {
        *seen_graph = desktop.version;
        field.refresh_exposure(&desktop.graph);
    }

    // 1) janelas sacudidas: os mobs delas caem, os ninhos racham
    for window in field.take_shakes() {
        let mut thrown = 0;
        for (_, mut mob) in &mut mobs {
            if mob.sim.walker.owner == SurfaceOwner::Window(window) && mob.sim.shake_off() {
                thrown += 1;
            }
        }
        let cracked = field.apply_shake(window, c, cfg.combat.health_bar_secs);
        info!("janela sacudida: {thrown} mob(s) derrubado(s), {cracked} ninho(s) rachado(s)");
        if thrown + cracked > 0
            && let Some(w) = desktop.windows.iter().find(|w| w.id == window)
        {
            let top = Vec2::new(w.rect.center().x, w.rect.min.y - 14.0);
            say(top, "Sacudida!".into(), COLOR_SHAKE, 24.0);
        }
    }

    // 2) pousos, mordidas e filhotes mortos (nao reaparecem: quem repoe e o ninho)
    let mut alive = 0u32;
    for (entity, mut mob) in &mut mobs {
        if mob.sim.fall_hurt.is_some() {
            let dmg = mob.sim.fall_hurt.take().unwrap_or(0.0);
            let at = mob.sim.body_center - Vec2::new(0.0, mob.sim.hit_radius + 18.0);
            say(at, format!("-{dmg}"), COLOR_SHAKE, 18.0);
        }
        if mob.sim.pending_bite {
            mob.sim.pending_bite = false;
            let lost = progress.lose_xp(c.bite_xp, &cfg.combat);
            let at = mob.sim.body_center - Vec2::new(0.0, mob.sim.hit_radius + 18.0);
            say(at, format!("Mordida!  -{lost} XP"), COLOR_WARN, 20.0);
            info!("elite mordeu: -{lost} XP (total {})", progress.xp);
        }
        let gone = mob.sim.brain.state() == SpiderState::Dead && !mob.sim.visible;
        if mob.sim.brood && gone {
            commands.entity(entity).despawn();
            continue;
        }
        alive += 1;
    }

    // 3) construcao: mob em repouso numa janela que esta parada ha tempo suficiente
    for (_, mob) in &mobs {
        let sim = &mob.sim;
        let resting = sim.hittable()
            && sim.brain.state() == SpiderState::Idle
            && !sim.walker.is_falling()
            && sim.stun_left <= 0.0;
        let SurfaceOwner::Window(window) = sim.walker.owner else {
            continue;
        };
        // (nada de ninho nas barras do sistema)
        if !resting || window >= crate::platform::PANEL_ID_BASE {
            continue;
        }
        let (pos, normal) = (sim.walker.pos, sim.walker.normal);
        if field
            .try_build(window, pos, normal, mob.kind, now, dt, c)
            .is_some()
        {
            info!(
                "ninho construido na janela 0x{window:X} ({} no total)",
                field.nests.len()
            );
            say(
                pos - Vec2::new(0.0, 30.0),
                "Ninho!".into(),
                COLOR_NEST,
                20.0,
            );
        }
    }

    // 4) o tempo passa: o ninho cresce e choca
    let free = cfg.mobs.max.saturating_sub(alive);
    for hatch in field.tick(dt, c, free) {
        let Some(entity) = spawn_mob(&mut commands, &mut catalog, &asset_server, hatch.kind) else {
            continue;
        };
        commands.entity(entity).insert(Hatchling {
            at: hatch.at,
            elite: hatch.elite,
        });
        info!(
            "ninho {} chocou um {}",
            hatch.nest,
            if hatch.elite { "ELITE" } else { "filhote" }
        );
        if hatch.elite {
            say(
                hatch.at - Vec2::new(0.0, 34.0),
                "Elite!".into(),
                COLOR_WARN,
                22.0,
            );
        }
    }

    // 5) ninhos sao clicaveis (os circulos dos mobs ja foram publicados por `spider_think`)
    let mut any_exposed = false;
    let mut busy = desktop.window_moving(now);
    for n in field.nests.iter().filter(|n| n.exposed) {
        let (center, radius) = (n.center(c), n.radius(c));
        targets.circles.push(HitCircle { center, radius });
        any_exposed = true;
        busy |= n.flash > 0.0
            || !n.built(c)
            || n.hatch_in < 2.0
            || cursor
                .local
                .is_some_and(|p| p.distance(center) < radius * 5.0);
    }

    // 6) ritmo do laco: nunca mais lento do que os ninhos (e a deteccao de sacudida) precisam
    let need = if busy {
        Some(Activity::Fast)
    } else if any_exposed {
        Some(Activity::Calm)
    } else {
        None
    };
    let rank = |a: Activity| match a {
        Activity::Hidden => 0,
        Activity::Calm => 1,
        Activity::Fast => 2,
    };
    if let Some(need) = need
        && rank(need) > rank(*activity)
    {
        *activity = need;
    }
}

/// Mantem um casulo 3D por ninho: cresce na construcao, pulsa antes de chocar, pisca ao levar
/// golpe e muda de cor quando vira ninho de elite.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn sync_nest_visuals(
    mut commands: Commands,
    cfg: Res<Config>,
    geo: Res<OverlayGeometry>,
    time: Res<Time<Real>>,
    field: Res<NestField>,
    assets: Option<Res<NestAssets>>,
    mut visuals: Query<(
        Entity,
        &NestVisual,
        &mut Transform,
        &mut Visibility,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
    mut known: Local<Vec<u32>>,
) {
    let Some(assets) = assets else { return };
    if !geo.ready {
        return;
    }
    let c = &cfg.nests;
    let t = time.elapsed_secs();
    known.clear();
    for (entity, visual, mut tf, mut vis, mut material) in &mut visuals {
        let Some(n) = field.get(visual.id) else {
            commands.entity(entity).despawn();
            continue;
        };
        known.push(n.id);

        let wobble = if n.built(c) && n.hatch_in < 2.0 {
            0.09 * (t * 20.0).sin() // prestes a chocar
        } else {
            0.03 * (t * 2.5 + n.id as f32).sin()
        };
        let flinch = 1.0 - 0.22 * (n.flash / NEST_FLASH_SECS).clamp(0.0, 1.0);
        let w = geo.local_to_world(n.center(c));
        tf.translation = Vec3::new(w.x, w.y, NEST_Z);
        // +Y do casulo = normal da superficie (Y de mundo e o inverso do de tela)
        tf.rotation =
            Quat::from_rotation_z((-n.normal.y).atan2(n.normal.x) - std::f32::consts::FRAC_PI_2);
        tf.scale = Vec3::splat(n.radius(c) * 2.0 * (1.0 + wobble) * flinch);

        let want_vis = if n.exposed {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want_vis {
            *vis = want_vis;
        }
        let want_mat = if n.flash > 0.0 {
            &assets.flash
        } else if n.is_elite(c) {
            &assets.elite
        } else {
            &assets.silk
        };
        if material.0 != *want_mat {
            material.0 = want_mat.clone();
        }
    }

    for n in field.nests.iter().filter(|n| !known.contains(&n.id)) {
        // nasce oculto e sem tamanho; o proximo quadro posiciona
        commands
            .spawn((
                NestVisual { id: n.id },
                Mesh3d(assets.mesh.clone()),
                MeshMaterial3d(assets.silk.clone()),
                Transform::from_scale(Vec3::ZERO),
                Visibility::Hidden,
            ))
            .with_children(|parent| {
                for (at, scale) in LUMPS {
                    parent.spawn((
                        Mesh3d(assets.mesh.clone()),
                        MeshMaterial3d(assets.shade.clone()),
                        Transform::from_translation(at).with_scale(scale),
                    ));
                }
                let mut line = |a: Vec2, b: Vec2, w: f32, z: f32, m: &Handle<StandardMaterial>| {
                    parent.spawn((
                        Mesh3d(assets.quad.clone()),
                        MeshMaterial3d(m.clone()),
                        strand(a, b, w, z),
                    ));
                };
                for (a, b) in BANDS {
                    line(a, b, 0.035, 0.56, &assets.crease);
                }
                for (a, b) in ANCHORS {
                    for flip in [Vec2::ONE, Vec2::new(-1.0, 1.0)] {
                        line(a * flip, b * flip, 0.04, -0.35, &assets.thread);
                    }
                }
                for (a, b) in nest_web_strands() {
                    line(a, b, 0.03, -0.6, &assets.thread_faint);
                }
            });
    }
}

/// Barra de vida sobre cada ninho golpeado/rachado ha pouco.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_nest_bars(
    mut commands: Commands,
    cfg: Res<Config>,
    field: Res<NestField>,
    windows: Query<&Window, With<PrimaryWindow>>,
    root: Query<Entity, With<HudRoot>>,
    mut bars: Query<(Entity, &NestBar, &mut Node, &Children), Without<NestBarFill>>,
    mut fills: Query<(&mut Node, &mut BackgroundColor), With<NestBarFill>>,
    mut known: Local<Vec<u32>>,
) {
    let Ok(root) = root.single() else {
        return;
    };
    let c = &cfg.nests;
    let scale = windows
        .single()
        .map(|w| w.scale_factor())
        .unwrap_or(1.0)
        .max(0.1);
    let top_left = |n: &crate::game::Nest| {
        n.center(c) - Vec2::new(BAR_W * scale * 0.5, n.radius(c) + 12.0 * scale)
    };

    known.clear();
    for (entity, bar, mut node, children) in &mut bars {
        let Some(n) = field.get(bar.id).filter(|n| n.exposed && n.bar_left > 0.0) else {
            commands.entity(entity).despawn();
            continue;
        };
        known.push(n.id);
        let at = top_left(n);
        node.left = px(at.x / scale);
        node.top = px(at.y / scale);
        let frac = (n.hp / n.max_hp.max(0.001)).clamp(0.0, 1.0);
        if let Some(&fill) = children.first()
            && let Ok((mut fnode, mut bg)) = fills.get_mut(fill)
        {
            fnode.width = percent(frac * 100.0);
            bg.0 = health_color(frac);
        }
    }

    for n in field
        .nests
        .iter()
        .filter(|n| n.exposed && n.bar_left > 0.0 && !known.contains(&n.id))
    {
        let frac = (n.hp / n.max_hp.max(0.001)).clamp(0.0, 1.0);
        let at = top_left(n);
        let fill = commands
            .spawn((
                NestBarFill,
                Node {
                    width: percent(frac * 100.0),
                    height: percent(100),
                    border_radius: BorderRadius::all(px(2)),
                    ..default()
                },
                BackgroundColor(health_color(frac)),
            ))
            .id();
        let bar = commands
            .spawn((
                NestBar { id: n.id },
                Node {
                    position_type: PositionType::Absolute,
                    left: px(at.x / scale),
                    top: px(at.y / scale),
                    width: px(BAR_W),
                    height: px(BAR_H),
                    padding: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.05, 0.05, 0.85)),
            ))
            .add_child(fill)
            .id();
        commands.entity(root).add_child(bar);
    }
}
