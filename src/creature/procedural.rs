//! Aranha provisoria feita de primitivas 3D do Bevy, com marcha procedural (IK de duas juntas).
//! Tipo de mob sempre disponivel; tambem e o substituto quando um GLB falha ao carregar.

use bevy::math::Vec3;
use bevy::prelude::*;

use super::animation::{
    BODY_HEIGHT, LEG_LOWER, LEG_UPPER, STRIDE, curled_foot, foot_position, leg_layout, leg_pose,
    smoothstep,
};
use super::behavior::SpiderState;
use super::spider::{Mob, PX_PER_UNIT, SpiderSim};
use crate::config::Config;
use bevy::mesh::CylinderMeshBuilder;

#[derive(Component)]
pub struct Abdomen;

/// Segmento de perna (cilindro entre duas articulacoes).
#[derive(Component)]
pub struct LegSegment {
    pub leg: usize,
    pub lower: bool,
}

/// Malhas e materiais compartilhados por todas as aranhas procedurais (criados uma vez).
#[derive(Resource)]
pub struct ProceduralAssets {
    sphere: Handle<Mesh>,
    cylinder: Handle<Mesh>,
    body_mat: Handle<StandardMaterial>,
    leg_mat: Handle<StandardMaterial>,
    eye_mat: Handle<StandardMaterial>,
    mark_mat: Handle<StandardMaterial>,
}

impl ProceduralAssets {
    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        let sphere = meshes.add(Sphere::new(0.5).mesh().uv(24, 14));
        let cylinder = meshes.add(CylinderMeshBuilder::new(0.5, 1.0, 8).build());

        let body_mat = materials.add(StandardMaterial {
            base_color: Color::srgb(0.07, 0.06, 0.065),
            perceptual_roughness: 0.38,
            metallic: 0.15,
            ..default()
        });
        let leg_mat = materials.add(StandardMaterial {
            base_color: Color::srgb(0.13, 0.105, 0.09),
            perceptual_roughness: 0.5,
            ..default()
        });
        let eye_mat = materials.add(StandardMaterial {
            base_color: Color::srgb(0.9, 0.1, 0.05),
            emissive: LinearRgba::new(6.0, 0.3, 0.1, 1.0),
            unlit: true,
            ..default()
        });
        let mark_mat = materials.add(StandardMaterial {
            base_color: Color::srgb(0.75, 0.08, 0.04),
            perceptual_roughness: 0.4,
            ..default()
        });
        Self {
            sphere,
            cylinder,
            body_mat,
            leg_mat,
            eye_mat,
            mark_mat,
        }
    }
}

/// Cria a aranha como filhos de `root`.
pub fn spawn_procedural(commands: &mut Commands, root: Entity, assets: &ProceduralAssets) {
    let ProceduralAssets {
        sphere,
        cylinder,
        body_mat,
        leg_mat,
        eye_mat,
        mark_mat,
    } = assets;

    let thorax = commands
        .spawn((
            Mesh3d(sphere.clone()),
            MeshMaterial3d(body_mat.clone()),
            Transform::from_xyz(0.18, BODY_HEIGHT + 0.02, 0.0)
                .with_scale(Vec3::new(0.62, 0.42, 0.52)),
        ))
        .id();
    let abdomen = commands
        .spawn((
            Abdomen,
            Mesh3d(sphere.clone()),
            MeshMaterial3d(body_mat.clone()),
            Transform::from_xyz(-0.40, BODY_HEIGHT + 0.07, 0.0)
                .with_scale(Vec3::new(0.95, 0.78, 0.78)),
        ))
        .id();
    // marca vermelha no dorso (efeito "viuva negra")
    let mark = commands
        .spawn((
            Mesh3d(sphere.clone()),
            MeshMaterial3d(mark_mat.clone()),
            Transform::from_xyz(-0.40, BODY_HEIGHT + 0.07 + 0.37, 0.0)
                .with_scale(Vec3::new(0.26, 0.07, 0.16)),
        ))
        .id();
    let mut children = vec![thorax, abdomen, mark];
    for z in [-0.1, 0.1] {
        children.push(
            commands
                .spawn((
                    Mesh3d(sphere.clone()),
                    MeshMaterial3d(eye_mat.clone()),
                    Transform::from_xyz(0.42, BODY_HEIGHT + 0.12, z).with_scale(Vec3::splat(0.09)),
                ))
                .id(),
        );
    }
    for leg in 0..8 {
        for lower in [false, true] {
            children.push(
                commands
                    .spawn((
                        LegSegment { leg, lower },
                        Mesh3d(cylinder.clone()),
                        MeshMaterial3d(leg_mat.clone()),
                        Transform::from_scale(Vec3::new(0.085, 1.0, 0.085)),
                    ))
                    .id(),
            );
        }
    }
    commands.entity(root).add_children(&children);
}

/// Pose de uma perna: (quadril, joelho, pe).
type LegPoses = [(Vec3, Vec3, Vec3); 8];

/// Pose das 8 pernas (quadril, joelho, pe) de um mob, a partir da simulacao.
fn leg_poses(sim: &SpiderSim, cfg: &Config, now: f32) -> [(Vec3, Vec3, Vec3); 8] {
    let rest = leg_layout();
    let units_per_px = 1.0 / (PX_PER_UNIT * cfg.spider.scale);
    let speed_units = sim.speed_px * units_per_px;
    // amplitude do passo cresce com a velocidade; parada = pes plantados
    let moving = smoothstep(speed_units / 1.2);
    let stride = STRIDE * moving;
    let lift = 0.18 + 0.22 * moving;

    let state = sim.brain.state();
    let t = sim.brain.state_time();
    let curl = if state == SpiderState::Dead {
        smoothstep(t / cfg.behavior.death_anim_secs.max(0.1))
    } else {
        0.0
    };
    let twitch = if state == SpiderState::Hit {
        (t * 70.0).sin() * 0.08
    } else {
        0.0
    };

    let mut poses = [(Vec3::ZERO, Vec3::ZERO, Vec3::ZERO); 8];
    for (i, leg) in rest.iter().enumerate() {
        let mut foot = foot_position(leg, sim.gait_phase, stride, lift);
        // pequeno "respirar" nas pernas quando parada
        foot.x += (now * 1.7 + i as f32).sin() * 0.012 * (1.0 - moving);
        foot.z += twitch * leg.side;
        if curl > 0.0 {
            foot = foot.lerp(curled_foot(leg), curl);
        }
        let (knee, foot) = leg_pose(leg, foot);
        poses[i] = (leg.hip, knee, foot);
    }
    poses
}

/// Posiciona cada segmento de perna entre suas articulacoes e anima a respiracao, por mob.
pub fn animate_procedural(
    cfg: Res<Config>,
    time: Res<Time>,
    mobs: Query<&Mob>,
    mut legs: Query<(&LegSegment, &ChildOf, &mut Transform), Without<Abdomen>>,
    mut abdomens: Query<(&ChildOf, &mut Transform), With<Abdomen>>,
    mut cache: Local<Vec<(Entity, LegPoses)>>,
) {
    let now = time.elapsed_secs();
    // uma pose por mob por quadro (reaproveita a alocacao entre quadros)
    cache.clear();
    for (seg, parent, mut tf) in &mut legs {
        let root = parent.parent();
        let poses = match cache.iter().find(|(e, _)| *e == root) {
            Some((_, p)) => *p,
            None => {
                let Ok(mob) = mobs.get(root) else { continue };
                let p = leg_poses(&mob.sim, &cfg, now);
                cache.push((root, p));
                p
            }
        };
        let (hip, knee, foot) = poses[seg.leg.min(7)];
        let (a, b) = if seg.lower { (knee, foot) } else { (hip, knee) };
        let d = b - a;
        let len = d.length().max(1e-4);
        tf.translation = (a + b) * 0.5;
        tf.rotation = Quat::from_rotation_arc(Vec3::Y, d / len);
        tf.scale = Vec3::new(0.085, len, 0.085);
    }
    let _ = (LEG_UPPER, LEG_LOWER);

    for (parent, mut ab) in &mut abdomens {
        // fase diferente por mob para nao "respirarem" em sincronia
        let phase = (parent.parent().to_bits() % 97) as f32;
        let breath = 1.0 + 0.035 * (now * 2.2 + phase).sin();
        ab.scale = Vec3::new(0.95, 0.78 * breath, 0.78);
    }
}
