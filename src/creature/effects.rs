//! Efeito visual da morte: pequena explosao de particulas (esferas que encolhem).
//! Sem alocacoes por frame: entidades sao criadas na explosao e removidas ao fim da vida.

use bevy::prelude::*;

use crate::rng::Rng;

const PARTICLES: usize = 16;
const GRAVITY: f32 = -1100.0;

#[derive(Resource)]
pub struct EffectAssets {
    mesh: Handle<Mesh>,
    materials: [Handle<StandardMaterial>; 3],
}

/// Quantas particulas existem agora (usado para manter o laco rapido enquanto animam).
#[derive(Resource, Default)]
pub struct EffectsActive(pub usize);

#[derive(Component)]
pub struct Particle {
    vel: Vec3,
    life: f32,
    max_life: f32,
    size: f32,
}

impl EffectAssets {
    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        let unlit = |c: Color| StandardMaterial {
            base_color: c,
            unlit: true,
            ..default()
        };
        Self {
            mesh: meshes.add(Sphere::new(0.5).mesh().uv(10, 6)),
            materials: [
                materials.add(unlit(Color::srgb(0.45, 0.85, 0.15))),
                materials.add(unlit(Color::srgb(0.20, 0.55, 0.10))),
                materials.add(unlit(Color::srgb(0.10, 0.09, 0.09))),
            ],
        }
    }
}

/// Dispara a explosao em `at` (coordenadas de mundo, px).
pub fn spawn_burst(commands: &mut Commands, assets: &EffectAssets, at: Vec3, rng: &mut Rng) {
    for i in 0..PARTICLES {
        let angle = rng.range(0.0, std::f32::consts::TAU);
        let speed = rng.range(140.0, 420.0);
        let size = rng.range(3.0, 8.0);
        let life = rng.range(0.45, 0.85);
        commands.spawn((
            Particle {
                vel: Vec3::new(
                    angle.cos() * speed,
                    angle.sin() * speed + 120.0,
                    rng.range(-60.0, 120.0),
                ),
                life,
                max_life: life,
                size,
            },
            Mesh3d(assets.mesh.clone()),
            MeshMaterial3d(assets.materials[i % assets.materials.len()].clone()),
            Transform::from_translation(at + Vec3::new(0.0, 0.0, 40.0))
                .with_scale(Vec3::splat(size)),
        ));
    }
}

pub fn update_particles(
    mut commands: Commands,
    time: Res<Time>,
    mut active: ResMut<EffectsActive>,
    mut particles: Query<(Entity, &mut Transform, &mut Particle)>,
) {
    let dt = time.delta_secs().min(0.1);
    let mut alive = 0;
    for (e, mut tf, mut p) in &mut particles {
        p.life -= dt;
        if p.life <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        alive += 1;
        p.vel.y += GRAVITY * dt;
        p.vel *= 1.0 - (2.0 * dt).min(0.9); // arrasto
        tf.translation += p.vel * dt;
        // encolhe ate sumir
        tf.scale = Vec3::splat(p.size * (p.life / p.max_life).clamp(0.0, 1.0));
    }
    if active.0 != alive {
        active.0 = alive;
    }
}
