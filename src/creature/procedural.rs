//! Bichos feitos de primitivas 3D (esferas, cones e segmentos afunilados) a partir de um
//! [`BodyPlan`], com animacao 100% procedural:
//!
//! * **pernas**: IK de duas juntas + tarso apoiado; a ponta do pe segue `step_offset`, com a fase
//!   avancando pela distancia percorrida (sem patinar). Andar -> correr troca a defasagem das
//!   pernas (o rato passa a galopar, o gafanhoto pula com as patas de tras juntas);
//! * **corpo**: sobe-e-desce, flexao da coluna, balanco da traseira, respiracao;
//! * **cabeca**: olha em volta e fareja quando parado;
//! * **cadeias** (antenas, cauda, bigodes, palpos, cercos): ondas que se propagam junta a junta.
//!
//! Hierarquia de cada mob: raiz (`Mob`) -> pivo do corpo -> {pivo da cabeca, pivo da traseira};
//! as partes rigidas ficam nos pivos. Pernas e cadeias sao filhas diretas da raiz e recebem a
//! pose calculada no espaco da raiz a cada quadro.

use bevy::math::{Affine3A, Vec3};
use bevy::mesh::{ConeMeshBuilder, ConicalFrustumMeshBuilder};
use bevy::prelude::*;

use super::animation::{smoothstep, step_offset, two_bone_ik};
use super::behavior::SpiderState;
use super::species::{BodyPlan, ChainKind, Group, Paint, Shape, Species};
use super::spider::{Mob, PX_PER_UNIT};
use crate::config::Config;

/// Afinamento dos segmentos de perna/cadeia (raio da ponta / raio da base).
const TAPER: f32 = 0.72;

/// Malhas e materiais compartilhados (criados uma vez; um plano + paleta por especie).
#[derive(Resource)]
pub struct ProceduralAssets {
    sphere: Handle<Mesh>,
    cone: Handle<Mesh>,
    limb: Handle<Mesh>,
    kinds: Vec<KindAssets>,
}

struct KindAssets {
    plan: BodyPlan,
    mats: Vec<Handle<StandardMaterial>>,
}

fn material(p: &Paint) -> StandardMaterial {
    let [r, g, b] = p.rgb;
    StandardMaterial {
        base_color: Color::srgb(r, g, b),
        perceptual_roughness: p.rough,
        metallic: p.metal,
        emissive: if p.glow > 0.0 {
            LinearRgba::new(r * p.glow, g * p.glow, b * p.glow, 1.0)
        } else {
            LinearRgba::BLACK
        },
        unlit: p.glow > 0.0,
        ..default()
    }
}

impl ProceduralAssets {
    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Self {
        let kinds = Species::ALL
            .iter()
            .map(|s| {
                let plan = s.plan();
                let mats = plan
                    .palette
                    .iter()
                    .map(|p| materials.add(material(p)))
                    .collect();
                KindAssets { plan, mats }
            })
            .collect();
        Self {
            sphere: meshes.add(Sphere::new(0.5).mesh().uv(20, 12)),
            cone: meshes.add(ConeMeshBuilder::new(0.5, 1.0, 14)),
            limb: meshes.add(ConicalFrustumMeshBuilder::new(0.5 * TAPER, 0.5, 1.0, 10)),
            kinds,
        }
    }

    pub fn plan(&self, species: Species) -> &BodyPlan {
        &self.kinds[species.index()].plan
    }
}

/// Entidades de uma perna: femur, tibia, tarso, juntas (joelho, tornozelo) e patinha.
struct LegRig {
    segs: [Entity; 3],
    joints: [Entity; 2],
    paw: Option<Entity>,
}

/// Esqueleto procedural de um mob (na entidade raiz).
#[derive(Component)]
pub struct ProcRig {
    species: Species,
    body: Entity,
    head: Entity,
    rear: Entity,
    legs: Vec<LegRig>,
    chains: Vec<Vec<Entity>>,
    /// Defasagem propria (para os mobs nao se moverem em sincronia).
    seed: f32,
}

fn pivot_point(plan: &BodyPlan, g: Group) -> Vec3 {
    match g {
        Group::Body => Vec3::new(0.0, plan.body_height, 0.0),
        Group::Head => plan.head_pivot,
        Group::Rear => plan.rear_pivot,
    }
}

/// Monta o bicho `species` como filhos de `root`.
pub fn spawn_procedural(
    commands: &mut Commands,
    root: Entity,
    assets: &ProceduralAssets,
    species: Species,
) {
    let ka = &assets.kinds[species.index()];
    let plan = &ka.plan;
    let mat = |i: u8| ka.mats[(i as usize).min(ka.mats.len() - 1)].clone();
    let body_p = pivot_point(plan, Group::Body);

    let body = commands
        .spawn((Transform::from_translation(body_p), Visibility::default()))
        .id();
    let head = commands
        .spawn((
            Transform::from_translation(plan.head_pivot - body_p),
            Visibility::default(),
        ))
        .id();
    let rear = commands
        .spawn((
            Transform::from_translation(plan.rear_pivot - body_p),
            Visibility::default(),
        ))
        .id();
    commands.entity(body).add_children(&[head, rear]);

    for part in &plan.parts {
        let (pivot, at) = match part.group {
            Group::Body => (body, body_p),
            Group::Head => (head, plan.head_pivot),
            Group::Rear => (rear, plan.rear_pivot),
        };
        let mesh = match part.shape {
            Shape::Sphere => assets.sphere.clone(),
            Shape::Cone => assets.cone.clone(),
        };
        let e = commands
            .spawn((
                Mesh3d(mesh),
                MeshMaterial3d(mat(part.paint)),
                Transform {
                    translation: part.pos - at,
                    rotation: part.rot,
                    scale: part.scale,
                },
            ))
            .id();
        commands.entity(pivot).add_child(e);
    }

    let mut children = vec![body];
    let mut seg = |commands: &mut Commands, mesh: &Handle<Mesh>, paint: u8| {
        let e = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat(paint)),
                Transform::from_scale(Vec3::ZERO),
            ))
            .id();
        children.push(e);
        e
    };
    let legs = plan
        .legs
        .iter()
        .map(|l| LegRig {
            segs: [
                seg(commands, &assets.limb, l.paints[0]),
                seg(commands, &assets.limb, l.paints[1]),
                seg(commands, &assets.limb, l.paints[2]),
            ],
            joints: [
                seg(commands, &assets.sphere, l.paints[1]),
                seg(commands, &assets.sphere, l.paints[2]),
            ],
            paw: (l.paw > 0.0).then(|| seg(commands, &assets.sphere, l.paints[2])),
        })
        .collect();
    let chains = plan
        .chains
        .iter()
        .map(|c| {
            (0..c.segments())
                .map(|_| seg(commands, &assets.limb, c.paint))
                .collect()
        })
        .collect();
    commands
        .entity(root)
        .add_children(&children)
        .insert(ProcRig {
            species,
            body,
            head,
            rear,
            legs,
            chains,
            seed: (root.to_bits() % 997) as f32 * 0.37,
        });
}

/// Segmento afunilado de `a` (base, raio `w`) ate `b`.
fn segment(a: Vec3, b: Vec3, w: f32) -> Transform {
    let d = b - a;
    let len = d.length();
    if len < 1e-4 {
        return Transform::from_translation(a).with_scale(Vec3::ZERO);
    }
    Transform {
        translation: (a + b) * 0.5,
        rotation: Quat::from_rotation_arc(Vec3::Y, d / len),
        scale: Vec3::new(2.0 * w, len, 2.0 * w),
    }
}

fn ball(at: Vec3, r: f32) -> Transform {
    Transform::from_translation(at).with_scale(Vec3::splat(2.0 * r))
}

/// Ruido suave em [-1, 1] (soma de senoides incomensuraveis).
fn wander(t: f32, seed: f32) -> f32 {
    ((t * 0.71 + seed).sin() * 0.6 + (t * 1.83 + seed * 1.7).sin() * 0.3 + (t * 0.23).sin() * 0.1)
        .clamp(-1.0, 1.0)
}

/// Variaveis de animacao de um mob neste quadro.
struct Drive {
    t: f32,
    seed: f32,
    gait: f32,
    /// 0..1: parado -> andando; correndo.
    moving: f32,
    run: f32,
    stride: f32,
    lift: f32,
    /// 0..1: progresso da morte.
    dead: f32,
    twitch: f32,
    fleeing: bool,
    /// 1 - dead; parado (0..1) e vivo; "farejando agora" (0..1).
    alive: f32,
    idle: f32,
    sniff_gate: f32,
}

fn drive(plan: &BodyPlan, mob: &Mob, cfg: &Config, now: f32, seed: f32) -> Drive {
    let sim = &mob.sim;
    let pr = &sim.profile;
    let units_per_px = 1.0 / (PX_PER_UNIT * cfg.spider.scale * pr.size);
    let v = sim.speed_px * units_per_px;
    let stride = pr.gait_stride(v);
    let moving = (stride / pr.stride.max(1e-3)).clamp(0.0, 1.0);
    let state = sim.brain.state();
    let st = sim.brain.state_time();
    let dead = if state == SpiderState::Dead {
        smoothstep(st / cfg.behavior.death_anim_secs.max(0.1))
    } else {
        0.0
    };
    let mut twitch = if state == SpiderState::Hit {
        (st * 70.0).sin() * (1.0 - st / 0.4).max(0.0)
    } else {
        0.0
    };
    if sim.hurt_flash > 0.0 {
        twitch += (now * 80.0).sin() * 0.7;
    }
    let alive = 1.0 - dead;
    Drive {
        t: now,
        seed,
        gait: sim.gait_phase,
        alive,
        idle: (1.0 - moving) * alive,
        sniff_gate: smoothstep((now * 0.5 + seed).sin() * 2.0),
        moving,
        run: pr.run_blend(v) * moving,
        stride,
        lift: plan.motion.lift * (0.4 + 0.6 * moving),
        dead,
        twitch,
        fleeing: state == SpiderState::Fleeing,
    }
}

/// Rotacao inicial de uma cadeia (antenas varrem para tras ao correr e caem na morte).
fn chain_root(kind: ChainKind, side: f32, d: &Drive) -> Quat {
    match kind {
        ChainKind::Antenna => {
            Quat::from_rotation_y(-side * (0.18 * d.moving + 0.2 * d.run))
                * Quat::from_rotation_z(-0.25 * d.run - 0.6 * d.dead)
        }
        ChainKind::Tail => Quat::from_rotation_z(0.18 * d.run),
        _ => Quat::IDENTITY,
    }
}

/// (guinada, arfagem) da junta `k` de `n` de uma cadeia: ondas que se propagam pela cadeia.
fn chain_joint(kind: ChainKind, k: f32, n: f32, ph: f32, d: &Drive, gait: f32) -> (f32, f32) {
    let t = d.t;
    match kind {
        ChainKind::Antenna => {
            let amp = (0.05 + 0.09 * d.idle + 0.04 * d.moving) * d.alive;
            let speed = if d.fleeing { 7.0 } else { 2.4 };
            (
                amp * (t * speed + ph + k * 0.55).sin(),
                amp * 0.6 * (t * speed * 0.7 + ph * 1.3 + k * 0.4).sin(),
            )
        }
        ChainKind::Tail => {
            let amp = (0.05 + 0.06 * d.moving + 0.05 * d.run) * (1.0 - 0.8 * d.dead);
            let speed = 2.0 + 5.0 * d.moving;
            (amp * (t * speed - k * 0.55 + ph).sin() * (0.4 + k / n), 0.0)
        }
        ChainKind::Whisker => {
            let amp = 0.04 + 0.2 * d.sniff_gate * d.idle;
            (amp * (t * 17.0 + ph).sin() * d.alive, 0.0)
        }
        ChainKind::Palp if k == 0.0 => {
            let w = 0.25 * (gait + ph).sin() * d.moving
                + 0.12 * (t * 3.1 + ph).sin() * (1.0 - d.moving);
            (0.0, w * d.alive)
        }
        ChainKind::Palp => (0.0, 0.0),
        ChainKind::Cercus => (0.05 * (t * 3.0 + ph).sin() * d.alive, 0.0),
    }
}

/// Posiciona todas as partes moveis de cada bicho procedural.
pub fn animate_procedural(
    cfg: Res<Config>,
    time: Res<Time>,
    assets: Res<ProceduralAssets>,
    mobs: Query<(&Mob, &ProcRig)>,
    mut tfs: Query<&mut Transform, Without<Mob>>,
) {
    use std::f32::consts::TAU;
    let now = time.elapsed_secs();
    for (mob, rig) in &mobs {
        if !mob.sim.visible {
            continue;
        }
        let plan = assets.plan(rig.species);
        let m = &plan.motion;
        let d = drive(plan, mob, &cfg, now, rig.seed);
        let mut set = |e: Entity, tf: Transform| {
            if let Ok(mut t) = tfs.get_mut(e) {
                *t = tf;
            }
        };
        let (alive, idle, sniff_gate) = (d.alive, d.idle, d.sniff_gate);
        let g = d.gait * TAU;

        // ---- corpo ----
        let body_p = pivot_point(plan, Group::Body);
        let quad = plan.legs.len() == 4;
        let bob = if quad {
            m.bob * d.moving * g.sin() * (1.0 + d.run)
        } else {
            m.bob * d.moving * (2.0 * g).sin()
        };
        let hop = m.hop * d.run * g.sin().max(0.0).powf(0.7);
        let breathe = (d.t * 2.2 + d.seed).sin();
        let body_pos = body_p + Vec3::Y * (bob + hop);
        let body_rot =
            Quat::from_rotation_z(m.flex * d.run * g.sin() + 0.25 * m.hop * d.run * g.cos())
                * Quat::from_rotation_x(0.08 * d.twitch);
        let body_aff = Affine3A::from_rotation_translation(body_rot, body_pos);
        set(
            rig.body,
            Transform::from_translation(body_pos).with_rotation(body_rot),
        );

        // ---- cabeca ----
        let look = m.look * idle * wander(d.t, d.seed);
        let sniff = m.sniff * idle * sniff_gate * (d.t * 19.0).sin();
        let head_rot = Quat::from_rotation_y(look)
            * Quat::from_rotation_z(
                sniff + 0.5 * m.flex * d.run * (g + 0.6).sin() - 0.6 * d.dead * m.look,
            );
        let head_off = plan.head_pivot - body_p;
        set(
            rig.head,
            Transform::from_translation(head_off).with_rotation(head_rot),
        );
        let head_aff = body_aff * Affine3A::from_rotation_translation(head_rot, head_off);

        // ---- traseira ----
        let sway = m.sway * d.moving * g.sin();
        let rear_rot =
            Quat::from_rotation_y(sway) * Quat::from_rotation_z(-1.3 * m.flex * d.run * g.sin());
        let rear_off = plan.rear_pivot - body_p;
        let breath = 1.0 + m.breath * breathe * alive;
        set(
            rig.rear,
            Transform {
                translation: rear_off,
                rotation: rear_rot,
                scale: Vec3::new(1.0, breath, 1.0 + 0.5 * (breath - 1.0)),
            },
        );
        let rear_aff = body_aff * Affine3A::from_rotation_translation(rear_rot, rear_off);

        // ---- pernas ----
        for (i, (leg, lr)) in plan.legs.iter().zip(&rig.legs).enumerate() {
            let offset = leg.phase_offset + (leg.phase_run - leg.phase_offset) * d.run;
            let o = step_offset(d.gait + offset, d.stride, d.lift, plan.profile.stance);
            let mut toe = leg.foot + Vec3::new(o.x, o.y, 0.0);
            // parado: pequenos ajustes de apoio
            toe.x += (d.t * 1.7 + i as f32 + d.seed).sin() * 0.012 * idle;
            toe.z += 0.05 * d.twitch * leg.side;
            if m.curl_on_death {
                toe = toe.lerp(leg.curl, d.dead);
            } else {
                // de lado: patas soltas, um pouco esticadas
                toe = toe.lerp(leg.foot + Vec3::new(0.0, 0.12, 0.08 * leg.side), d.dead);
            }
            let hip = body_aff.transform_point3(leg.hip - body_p);
            let pole = body_rot * leg.pole;
            let (knee, ankle) = two_bone_ik(hip, toe + leg.tarsus, leg.l1, leg.l2, pole);
            let toe = ankle - leg.tarsus;
            let [w0, w1, w2] = leg.widths;
            set(lr.segs[0], segment(hip, knee, w0));
            set(lr.segs[1], segment(knee, ankle, w1));
            set(lr.segs[2], segment(ankle, toe, w2));
            set(lr.joints[0], ball(knee, (w0 * TAPER).max(w1) * 0.98));
            set(lr.joints[1], ball(ankle, (w1 * TAPER).max(w2) * 0.98));
            if let Some(paw) = lr.paw {
                set(
                    paw,
                    Transform {
                        translation: toe + Vec3::Y * leg.paw * 0.4,
                        rotation: body_rot,
                        scale: Vec3::new(leg.paw * 3.2, leg.paw * 1.1, leg.paw * 2.2),
                    },
                );
            }
        }

        // ---- cadeias ----
        for (chain, ents) in plan.chains.iter().zip(&rig.chains) {
            let (aff, at) = match chain.group {
                Group::Body => (body_aff, body_p),
                Group::Head => (head_aff, plan.head_pivot),
                Group::Rear => (rear_aff, plan.rear_pivot),
            };
            let rot = Quat::from_mat3a(&aff.matrix3);
            let ph = d.seed + chain.phase;
            let n = chain.segments().max(1) as f32;
            let mut q = chain_root(chain.kind, chain.side, &d);
            // cadeias do lado -Z balancam espelhadas
            let mirror = if chain.side < 0.0 { -1.0 } else { 1.0 };
            let mut p = aff.transform_point3(chain.base - at);
            for (i, (&e, (&dir, &len))) in ents
                .iter()
                .zip(chain.dirs.iter().zip(&chain.lens))
                .enumerate()
            {
                let (yaw, pitch) = chain_joint(chain.kind, i as f32, n, ph, &d, g);
                q = q * Quat::from_rotation_y(yaw * mirror) * Quat::from_rotation_z(pitch);
                let mut next = p + rot * (q * dir) * len;
                let w_end = chain.width_at(i + 1);
                if chain.ground && next.y < w_end {
                    next.y = w_end;
                }
                set(e, segment(p, next, chain.width_at(i)));
                p = next;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_spans_both_points() {
        let a = Vec3::new(0.1, 0.5, 0.2);
        let b = Vec3::new(0.7, 0.0, 0.9);
        let tf = segment(a, b, 0.05);
        let start = tf.transform_point(Vec3::new(0.0, -0.5, 0.0));
        let end = tf.transform_point(Vec3::new(0.0, 0.5, 0.0));
        assert!(start.distance(a) < 1e-4 && end.distance(b) < 1e-4);
        assert_eq!(segment(a, a, 0.05).scale, Vec3::ZERO);
    }

    #[test]
    fn wander_is_bounded() {
        for i in 0..500 {
            let w = wander(i as f32 * 0.37, 3.0);
            assert!((-1.0..=1.0).contains(&w));
        }
    }
}
