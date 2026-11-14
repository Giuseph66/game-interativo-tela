//! Painel flutuante: janela pequena, transparente, sem bordas e sempre no topo, com um botao por
//! tipo de mob. Arraste pelo titulo; "_" minimiza.
//!
//! O painel e uma janela comum (recebe cliques normalmente); o overlay continua em click-through.

use bevy::camera::RenderTarget;
use bevy::picking::events::PointerPress;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::Pressed;
use bevy::ui_widgets::{Activate, Button};
use bevy::window::{CompositeAlphaMode, WindowLevel, WindowPosition, WindowRef, WindowResolution};

use crate::creature::spider::Mob;
use crate::game::Score;
use crate::mobs::{MobCatalog, MobCommand};
use crate::overlay::OverlayGeometry;

/// Largura do painel em pixels logicos.
const PANEL_W: f32 = 270.0;
const BUTTON_H: f32 = 30.0;
const GAP: f32 = 6.0;

const BG: Color = Color::srgba(0.07, 0.07, 0.09, 0.86);
const BTN: Color = Color::srgba(0.18, 0.18, 0.22, 0.95);
const BTN_HOVER: Color = Color::srgba(0.28, 0.28, 0.34, 0.98);
const BTN_PRESS: Color = Color::srgba(0.35, 0.62, 0.35, 1.0);
const BTN_DANGER: Color = Color::srgba(0.45, 0.14, 0.14, 0.95);
const TEXT: Color = Color::srgb(0.92, 0.92, 0.95);
const MUTED: Color = Color::srgb(0.6, 0.6, 0.66);

#[derive(Component)]
pub struct PanelWindow;

#[derive(Component)]
pub struct StatusText;

/// Cor-base do botao (para voltar apos hover/press).
#[derive(Component)]
pub struct PanelButton(Color);

/// Altura logica necessaria para `kinds` botoes de spawn.
pub fn panel_height(kinds: usize) -> f32 {
    // titulo + status + rotulo + botoes de spawn + linha de acoes + sair + dica + margens
    let rows = kinds as f32 + 2.0;
    46.0 + 26.0 + 22.0 + rows * (BUTTON_H + GAP) + 26.0 + 24.0
}

pub fn spawn_panel(
    mut commands: Commands,
    catalog: Res<MobCatalog>,
    opts: Res<crate::overlay::RunOptions>,
    cfg: Res<crate::config::Config>,
) {
    if opts.poc || !cfg.mobs.panel || opts.no_panel {
        return;
    }
    let h = panel_height(catalog.kinds.len());
    let window = commands
        .spawn((
            PanelWindow,
            Window {
                title: "SpiderDesk - mobs".into(),
                transparent: true,
                decorations: false,
                resizable: false,
                window_level: WindowLevel::AlwaysOnTop,
                composite_alpha_mode: CompositeAlphaMode::PreMultiplied,
                // aparece depois de posicionado/escalado (ver `fit_panel`)
                visible: false,
                resolution: WindowResolution::new(PANEL_W as u32, h as u32),
                position: WindowPosition::At(IVec2::new(40, 40)),
                ..default()
            },
        ))
        .id();
    let camera = commands
        .spawn((
            Camera2d,
            Camera {
                clear_color: ClearColorConfig::Custom(Color::NONE),
                ..default()
            },
            RenderTarget::Window(WindowRef::Entity(window)),
        ))
        .id();

    let root = commands
        .spawn((
            UiTargetCamera(camera),
            Node {
                width: percent(100),
                height: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(10)),
                row_gap: px(GAP),
                border_radius: BorderRadius::all(px(12)),
                ..default()
            },
            BackgroundColor(BG),
        ))
        .id();

    // titulo (area de arrastar) + minimizar
    let title_row = commands
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(6),
            ..default()
        })
        .id();
    let drag = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                height: px(28),
                align_items: AlignItems::Center,
                ..default()
            },
            children![(text("SpiderDesk  ::  mobs", 15.0, TEXT))],
        ))
        .observe(
            |_press: On<PointerPress>, mut windows: Query<&mut Window, With<PanelWindow>>| {
                if let Ok(mut w) = windows.single_mut() {
                    w.start_drag_move();
                }
            },
        )
        .id();
    let minimize = button(&mut commands, "_", BTN, 30.0, PanelAction::Minimize);
    commands.entity(title_row).add_children(&[drag, minimize]);

    let status = commands
        .spawn((
            StatusText,
            text("Mobs: 0   Abates: 0   Pontos: 0", 13.0, MUTED),
        ))
        .id();
    let label = commands
        .spawn(text("Clique para spawnar:", 13.0, MUTED))
        .id();
    let mut children = vec![title_row, status, label];

    for (i, kind) in catalog.kinds.iter().enumerate() {
        let b = button(
            &mut commands,
            &format!("+  {}", kind.label),
            BTN,
            0.0,
            PanelAction::Mob(MobCommand::Spawn(i)),
        );
        children.push(b);
    }

    // acoes em massa (dois botoes lado a lado)
    let row = commands
        .spawn(Node {
            flex_direction: FlexDirection::Row,
            column_gap: px(GAP),
            ..default()
        })
        .id();
    let kill = button(
        &mut commands,
        "Matar todos",
        BTN,
        -1.0,
        PanelAction::Mob(MobCommand::KillAll),
    );
    let remove = button(
        &mut commands,
        "Remover todos",
        BTN,
        -1.0,
        PanelAction::Mob(MobCommand::RemoveAll),
    );
    commands.entity(row).add_children(&[kill, remove]);
    children.push(row);

    children.push(button(
        &mut commands,
        "Sair do SpiderDesk",
        BTN_DANGER,
        0.0,
        PanelAction::Mob(MobCommand::Quit),
    ));
    children.push(
        commands
            .spawn(text("Emergencia: Ctrl+Alt+Shift+Q", 12.0, MUTED))
            .id(),
    );

    commands.entity(root).add_children(&children);
}

fn text(s: &str, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

/// O que um botao do painel faz.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelAction {
    Mob(MobCommand),
    Minimize,
}

/// Botao com texto. `width`: > 0 fixa, 0 = largura total, < 0 = divide a linha (flex).
fn button(
    commands: &mut Commands,
    label: &str,
    color: Color,
    width: f32,
    action: PanelAction,
) -> Entity {
    commands
        .spawn((
            Button,
            Hovered::default(),
            PanelButton(color),
            action,
            Node {
                width: if width > 0.0 {
                    px(width)
                } else if width == 0.0 {
                    percent(100)
                } else {
                    auto()
                },
                flex_grow: if width < 0.0 { 1.0 } else { 0.0 },
                height: px(BUTTON_H),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(color),
            children![text(label, 14.0, TEXT)],
        ))
        .observe(on_panel_activate)
        .id()
}

/// Um unico observer para todos os botoes: le a `PanelAction` do botao ativado.
fn on_panel_activate(
    activate: On<Activate>,
    actions: Query<&PanelAction>,
    mut cmds: MessageWriter<MobCommand>,
    mut windows: Query<&mut Window, With<PanelWindow>>,
) {
    match actions.get(activate.entity) {
        Ok(PanelAction::Mob(c)) => {
            cmds.write(*c);
        }
        Ok(PanelAction::Minimize) => {
            if let Ok(mut w) = windows.single_mut() {
                w.set_minimized(true);
            }
        }
        Err(_) => {}
    }
}

/// `--panel-selftest`: dispara `Activate` (o mesmo evento de um clique real) nos botoes do painel:
/// spawna um de cada tipo, depois "Matar todos" e por fim "Remover todos".
pub fn panel_selftest(
    mut commands: Commands,
    opts: Res<crate::overlay::RunOptions>,
    time: Res<Time<Real>>,
    geo: Res<OverlayGeometry>,
    buttons: Query<(Entity, &PanelAction)>,
    mut step: Local<u32>,
    mut t0: Local<Option<f32>>,
) {
    if !opts.panel_selftest || !geo.ready {
        return;
    }
    let now = time.elapsed_secs();
    let start = *t0.get_or_insert(now);
    let spawns: Vec<Entity> = {
        let mut v: Vec<(usize, Entity)> = buttons
            .iter()
            .filter_map(|(e, a)| match a {
                PanelAction::Mob(MobCommand::Spawn(i)) => Some((*i, e)),
                _ => None,
            })
            .collect();
        v.sort();
        v.into_iter().map(|(_, e)| e).collect()
    };
    let find = |want: MobCommand| {
        buttons
            .iter()
            .find(|(_, a)| **a == PanelAction::Mob(want))
            .map(|(e, _)| e)
    };
    // agenda: um spawn a cada 0.4 s; matar todos em +4 s; remover todos em +8 s
    let n = spawns.len() as u32;
    let target = if *step < n {
        (now - start >= 1.0 + 0.4 * *step as f32).then(|| spawns[*step as usize])
    } else if *step == n {
        (now - start >= 1.0 + 0.4 * n as f32 + 4.0)
            .then(|| find(MobCommand::KillAll))
            .flatten()
    } else if *step == n + 1 {
        (now - start >= 1.0 + 0.4 * n as f32 + 8.0)
            .then(|| find(MobCommand::RemoveAll))
            .flatten()
    } else {
        None
    };
    if let Some(e) = target {
        info!(
            "--panel-selftest: Activate no botao {:?}",
            buttons.get(e).map(|(_, a)| *a).ok()
        );
        commands.trigger(Activate { entity: e });
        *step += 1;
    }
}

/// Depois que o overlay conhece o monitor: escala o painel pelo DPI, posiciona no canto superior
/// direito e mostra.
pub fn fit_panel(
    geo: Res<OverlayGeometry>,
    catalog: Res<MobCatalog>,
    mut windows: Query<&mut Window, With<PanelWindow>>,
    mut done: Local<bool>,
) {
    if *done || !geo.ready {
        return;
    }
    let Ok(mut w) = windows.single_mut() else {
        return;
    };
    let scale = geo.monitor_scale.max(0.5);
    let size = Vec2::new(PANEL_W, panel_height(catalog.kinds.len())) * scale;
    w.resolution
        .set_physical_resolution(size.x.round() as u32, size.y.round() as u32);
    let margin = (24.0 * scale) as i32;
    w.position = WindowPosition::At(IVec2::new(
        geo.origin.x + geo.size.x as i32 - size.x as i32 - margin,
        geo.origin.y + margin + (40.0 * scale) as i32,
    ));
    w.visible = true;
    *done = true;
}

pub fn update_status(
    score: Res<Score>,
    mobs: Query<&Mob>,
    mut texts: Query<&mut Text, With<StatusText>>,
    mut last: Local<(usize, u32, u32)>,
) {
    let n = mobs.iter().count();
    let now = (n, score.kills, score.points);
    if *last == now {
        return;
    }
    *last = now;
    for mut t in &mut texts {
        **t = format!(
            "Mobs: {n}   Abates: {}   Pontos: {}",
            score.kills, score.points
        );
    }
}

/// Realce visual de hover/press (poucos botoes: varre todos e so escreve quando muda).
pub fn button_colors(
    mut buttons: Query<(&Hovered, Has<Pressed>, &PanelButton, &mut BackgroundColor)>,
) {
    for (hovered, pressed, base, mut bg) in &mut buttons {
        let want = if pressed {
            BTN_PRESS
        } else if hovered.get() {
            BTN_HOVER
        } else {
            base.0
        };
        if bg.0 != want {
            bg.0 = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_grows_with_number_of_kinds() {
        assert!(panel_height(6) > panel_height(1));
        assert!(panel_height(1) > 200.0);
        let per = panel_height(2) - panel_height(1);
        assert!((per - (BUTTON_H + GAP)).abs() < 1e-3);
    }
}
