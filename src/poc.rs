//! Modo prova de conceito (Etapa 1): um cubo 3D colorido atravessa a tela, sobre os outros
//! aplicativos. Serve para validar, no Windows real, tres coisas isoladas da aranha:
//! (1) transparencia verdadeira, (2) click-through seletivo e (3) encerramento de emergencia.
//! Clicar no cubo troca sua cor; qualquer outro clique deve chegar ao aplicativo de baixo.
//!
//! Execucao: `cargo run -- --poc`

use bevy::prelude::*;

use crate::overlay::{CursorState, HitCircle, HitTargets, OverlayGeometry};

const CUBE_SIZE: f32 = 90.0;

#[derive(Component)]
pub struct PocCube;

#[derive(Resource, Default)]
pub struct PocState {
    pub clicks: u32,
    pub t: f32,
}

pub fn setup_poc(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    commands.insert_resource(PocState::default());
    commands.spawn((
        PocCube,
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.95, 0.35, 0.1),
            perceptual_roughness: 0.4,
            ..default()
        })),
        Transform::from_scale(Vec3::splat(CUBE_SIZE)),
        Visibility::Hidden,
    ));
}

/// Movimento simples + hit region + clique.
#[allow(clippy::too_many_arguments)]
pub fn update_poc(
    time: Res<Time>,
    geo: Res<OverlayGeometry>,
    cursor: Res<CursorState>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut state: ResMut<PocState>,
    mut targets: ResMut<HitTargets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    cube: Single<
        (
            &mut Transform,
            &mut Visibility,
            &MeshMaterial3d<StandardMaterial>,
        ),
        With<PocCube>,
    >,
) {
    if !geo.ready {
        return;
    }
    let (mut tf, mut vis, mat) = cube.into_inner();
    state.t += time.delta_secs();

    // vai e volta na faixa a 60% da altura, com leve onda vertical
    let w = geo.size.x as f32;
    let h = geo.size.y as f32;
    let margin = CUBE_SIZE;
    let phase = (state.t * 0.12).fract();
    let tri = 1.0 - (2.0 * phase - 1.0).abs(); // 0..1..0
    let x = margin + tri * (w - 2.0 * margin);
    let y = h * 0.6 + (state.t * 1.3).sin() * 40.0;
    let world = geo.local_to_world(Vec2::new(x, y));
    tf.translation = Vec3::new(world.x, world.y, 0.0);
    tf.rotation = Quat::from_euler(EulerRot::XYZ, state.t * 0.9, state.t * 1.3, state.t * 0.4);
    *vis = Visibility::Inherited;

    let center = Vec2::new(x, y);
    let radius = CUBE_SIZE * 0.75;
    targets.circles.clear();
    targets.circles.push(HitCircle { center, radius });

    if buttons.just_pressed(MouseButton::Left)
        && cursor.local.is_some_and(|c| c.distance(center) <= radius)
    {
        state.clicks += 1;
        info!("PoC: clique no cubo #{}", state.clicks);
        if let Some(mut m) = materials.get_mut(&mat.0) {
            let hue = (state.clicks as f32 * 67.0) % 360.0;
            m.base_color = Color::hsl(hue, 0.8, 0.55);
        }
    }
}
