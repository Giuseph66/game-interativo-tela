//! Interface de combate desenhada no overlay (Bevy UI):
//!
//! * cursor vira a **espada atual** quando passa sobre um mob;
//! * **barra de vida** sobre o mob atingido (some apos `combat.health_bar_secs`);
//! * **numeros de dano** que sobem e somem (com "+XP" no abate);
//! * faixa "Espada X desbloqueada!" ao evoluir.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow, SystemCursorIcon};

use crate::config::Config;
use crate::creature::spider::{Mob, OverlayCamera};
use crate::game::{Progress, Strike, Sword};
use crate::overlay::{Activity, CursorState, HitTargets, OverlayGeometry};

const BAR_W: f32 = 56.0;
const BAR_H: f32 = 7.0;
const POPUP_SECS: f32 = 0.9;
const BANNER_SECS: f32 = 2.8;

/// Um golpe do jogador acabou de acontecer (para efeitos visuais).
#[derive(Message, Debug, Clone, Copy)]
pub struct HitFx {
    /// Centro do mob em px locais do overlay.
    pub at: Vec2,
    pub radius: f32,
    /// Espada usada no golpe.
    pub sword: Sword,
    pub strike: Strike,
}

/// Texto flutuante avulso (ninho construido, sacudida, dano de queda, mordida).
#[derive(Message, Debug, Clone)]
pub struct FloatText {
    /// Ponto de partida em px locais do overlay.
    pub at: Vec2,
    pub text: String,
    pub color: Color,
    pub size: f32,
}

#[derive(Resource)]
pub struct SwordCursors(HashMap<Sword, Handle<Image>>);

/// Raiz da UI do overlay (tela inteira, sem fundo).
#[derive(Component)]
pub struct HudRoot;

#[derive(Component)]
pub struct HealthBarFill;

#[derive(Component)]
pub struct Popup {
    age: f32,
    origin: Vec2,
    color: Color,
}

#[derive(Component)]
pub struct Banner {
    age: f32,
}

pub fn sword_color(s: Sword) -> Color {
    match s {
        Sword::Wood => Color::srgb(0.85, 0.62, 0.32),
        Sword::Iron => Color::srgb(0.92, 0.92, 0.95),
        Sword::Gold => Color::srgb(1.0, 0.86, 0.25),
        Sword::Diamond => Color::srgb(0.35, 0.95, 0.88),
    }
}

/// Cor da barra: verde (cheia) -> amarelo -> vermelho (quase morto).
pub fn health_color(frac: f32) -> Color {
    let f = frac.clamp(0.0, 1.0);
    if f > 0.5 {
        Color::srgb(1.0 - (f - 0.5) * 2.0 * 0.75, 0.85, 0.2)
    } else {
        Color::srgb(0.95, 0.15 + f * 2.0 * 0.7, 0.15)
    }
}

pub fn setup_hud(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    opts: Res<crate::overlay::RunOptions>,
    cameras: Query<Entity, With<OverlayCamera>>,
) {
    if opts.poc {
        return;
    }
    let cursors = Sword::ALL
        .iter()
        .map(|s| (*s, asset_server.load(s.cursor_path())))
        .collect();
    commands.insert_resource(SwordCursors(cursors));
    let Ok(camera) = cameras.single() else {
        return;
    };
    commands.spawn((
        HudRoot,
        UiTargetCamera(camera),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
    ));
}

/// Cursor de espada sobre mobs; cursor normal fora deles.
#[allow(clippy::too_many_arguments)]
pub fn update_sword_cursor(
    mut commands: Commands,
    geo: Res<OverlayGeometry>,
    cursor: Res<CursorState>,
    targets: Res<HitTargets>,
    progress: Res<Progress>,
    cursors: Option<Res<SwordCursors>>,
    window: Query<Entity, With<PrimaryWindow>>,
    mut current: Local<Option<Option<Sword>>>,
) {
    let (Some(cursors), Ok(window)) = (cursors, window.single()) else {
        return;
    };
    let hovering = geo.ready && cursor.local.is_some_and(|c| targets.contains(c, 1.0));
    let want = hovering.then_some(progress.sword);
    if *current == Some(want) {
        return;
    }
    *current = Some(want);
    let icon = match want.and_then(|s| cursors.0.get(&s)) {
        Some(handle) => CursorIcon::Custom(CustomCursor::Image(CustomCursorImage {
            handle: handle.clone(),
            texture_atlas: None,
            flip_x: false,
            flip_y: false,
            rect: None,
            // ponta da lamina (as imagens foram espelhadas para a ponta ficar no canto superior esquerdo)
            hotspot: (1, 1),
        })),
        None => CursorIcon::System(SystemCursorIcon::Default),
    };
    commands.entity(window).insert(icon);
}

/// Numeros de dano e faixa de evolucao.
pub fn spawn_hit_fx(
    mut commands: Commands,
    mut fx: MessageReader<HitFx>,
    progress: Res<Progress>,
    cursors: Option<Res<SwordCursors>>,
    root: Query<Entity, With<HudRoot>>,
) {
    let Ok(root) = root.single() else {
        fx.clear();
        return;
    };
    for hit in fx.read() {
        let s = &hit.strike;
        let label = if s.killed {
            format!("-{}  +{} XP", s.damage, s.xp_gained)
        } else {
            format!("-{}", s.damage)
        };
        let color = sword_color(hit.sword);
        let origin = hit.at - Vec2::new(0.0, hit.radius + 18.0);
        let popup = commands
            .spawn((
                Popup {
                    age: 0.0,
                    origin,
                    color,
                },
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(if s.killed { 22.0 } else { 18.0 }),
                    ..default()
                },
                TextColor(color),
                TextShadow::default(),
            ))
            .id();
        commands.entity(root).add_child(popup);

        if let Some(new) = s.upgraded {
            let mut children = Vec::new();
            if let Some(img) = cursors.as_ref().and_then(|c| c.0.get(&new)) {
                children.push(
                    commands
                        .spawn((
                            ImageNode::new(img.clone()),
                            Node {
                                width: px(40),
                                height: px(40),
                                ..default()
                            },
                        ))
                        .id(),
                );
            }
            children.push(
                commands
                    .spawn((
                        Text::new(format!(
                            "Espada de {} desbloqueada!  (XP {})",
                            new.label(),
                            progress.xp
                        )),
                        TextFont {
                            font_size: FontSize::Px(26.0),
                            ..default()
                        },
                        TextColor(sword_color(new)),
                        TextShadow::default(),
                    ))
                    .id(),
            );
            let banner = commands
                .spawn((
                    Banner { age: 0.0 },
                    Node {
                        position_type: PositionType::Absolute,
                        top: percent(12),
                        width: percent(100),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        column_gap: px(12),
                        ..default()
                    },
                ))
                .add_children(&children)
                .id();
            commands.entity(root).add_child(banner);
        }
    }
}

/// Textos flutuantes avulsos: sobem e somem como os numeros de dano.
pub fn spawn_float_text(
    mut commands: Commands,
    mut texts: MessageReader<FloatText>,
    root: Query<Entity, With<HudRoot>>,
) {
    let Ok(root) = root.single() else {
        texts.clear();
        return;
    };
    for t in texts.read() {
        let popup = commands
            .spawn((
                Popup {
                    age: 0.0,
                    origin: t.at,
                    color: t.color,
                },
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                Text::new(t.text.clone()),
                TextFont {
                    font_size: FontSize::Px(t.size),
                    ..default()
                },
                TextColor(t.color),
                TextShadow::default(),
            ))
            .id();
        commands.entity(root).add_child(popup);
    }
}

/// Anima/remove numeros de dano e faixas; mantem o laco rapido enquanto houver algo animando.
#[allow(clippy::too_many_arguments)]
pub fn animate_fx(
    mut commands: Commands,
    time: Res<Time<Real>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut popups: Query<(Entity, &mut Popup, &mut Node, &mut TextColor), Without<Banner>>,
    mut banners: Query<(Entity, &mut Banner)>,
    mut activity: ResMut<Activity>,
) {
    let dt = time.delta_secs().min(0.1);
    let scale = windows
        .single()
        .map(|w| w.scale_factor())
        .unwrap_or(1.0)
        .max(0.1);
    let mut busy = false;
    for (e, mut p, mut node, mut color) in &mut popups {
        p.age += dt;
        if p.age >= POPUP_SECS {
            commands.entity(e).despawn();
            continue;
        }
        busy = true;
        let k = p.age / POPUP_SECS;
        let pos = p.origin - Vec2::new(14.0, 40.0 * k);
        node.left = px(pos.x / scale);
        node.top = px(pos.y / scale);
        color.0 = p.color.with_alpha(1.0 - k * k);
    }
    for (e, mut b) in &mut banners {
        b.age += dt;
        if b.age >= BANNER_SECS {
            commands.entity(e).despawn();
        } else {
            busy = true;
        }
    }
    if busy && *activity != Activity::Fast {
        *activity = Activity::Fast;
    }
}

/// Barra de vida sobre cada mob atingido recentemente.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn update_health_bars(
    mut commands: Commands,
    cfg: Res<Config>,
    windows: Query<&Window, With<PrimaryWindow>>,
    root: Query<Entity, With<HudRoot>>,
    mobs: Query<(Entity, &Mob)>,
    mut bars_q: Query<(&mut Node, &Children), Without<HealthBarFill>>,
    mut fills: Query<(&mut Node, &mut BackgroundColor), With<HealthBarFill>>,
    mut bars: Local<HashMap<Entity, Entity>>,
) {
    let Ok(root) = root.single() else {
        return;
    };
    let scale = windows
        .single()
        .map(|w| w.scale_factor())
        .unwrap_or(1.0)
        .max(0.1);

    // remove barras de mobs que sumiram
    bars.retain(|mob, bar| {
        let alive = mobs.contains(*mob);
        if !alive {
            commands.entity(*bar).despawn();
        }
        alive
    });

    for (entity, mob) in &mobs {
        let sim = &mob.sim;
        let show = sim.hittable()
            && sim
                .last_hit
                .is_some_and(|t| sim.game_time - t <= cfg.combat.health_bar_secs);
        let bar = match bars.get(&entity) {
            Some(b) => *b,
            None if show => {
                let fill = commands
                    .spawn((
                        HealthBarFill,
                        Node {
                            width: percent(100),
                            height: percent(100),
                            border_radius: BorderRadius::all(px(2)),
                            ..default()
                        },
                        BackgroundColor(health_color(1.0)),
                    ))
                    .id();
                let bar = commands
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
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
                bars.insert(entity, bar);
                continue; // posiciona no proximo quadro
            }
            None => continue,
        };
        let Ok((mut node, children)) = bars_q.get_mut(bar) else {
            continue;
        };
        if !show {
            if node.display != Display::None {
                node.display = Display::None;
            }
            continue;
        }
        node.display = Display::Flex;
        let top_left =
            sim.body_center - Vec2::new(BAR_W * scale * 0.5, sim.hit_radius + 10.0 * scale);
        node.left = px(top_left.x / scale);
        node.top = px(top_left.y / scale);
        let frac = (sim.hp / sim.max_hp.max(0.001)).clamp(0.0, 1.0);
        if let Some(&fill) = children.first()
            && let Ok((mut fnode, mut bg)) = fills.get_mut(fill)
        {
            fnode.width = percent(frac * 100.0);
            bg.0 = health_color(frac);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_color_goes_green_to_red() {
        let full = health_color(1.0).to_srgba();
        let low = health_color(0.05).to_srgba();
        assert!(full.green > full.red, "cheia = verde");
        assert!(low.red > low.green, "quase morto = vermelho");
        // nao explode fora da faixa
        let _ = health_color(-3.0);
        let _ = health_color(9.0);
    }
}
