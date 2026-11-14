//! Modelos `.glb` reais (malha, materiais, esqueleto e animacoes incorporadas), um por mob.
//!
//! O carregador e tolerante com modelos de terceiros (Sketchfab, Meshy, Tripo...):
//!
//! * **Escala/posicao automaticas** (`spider.glb_auto_fit`): depois de instanciado, o modelo e
//!   medido (pelos ossos, se tiver esqueleto; senao pela AABB) e reescalado para ~3 unidades de
//!   envergadura, centralizado em X/Z e com a base em `y = 0`. A escala do tipo multiplica.
//! * **Orientacao**: o modelo deve olhar para +X; se olhar para outro lado, `glb_yaw_deg` no preset.
//! * **Animacoes** por nome (`idle`, `walk`, `run`, `climb`, `hit`, `death`, em qualquer
//!   prefixo/caixa); variantes ("walking backward", "turn left") sao evitadas e um unico clipe
//!   generico ("Take 001") vira `walk`. Clipes ausentes usam o fallback de
//!   [`Clip::fallback_chain`]; quando o fallback e outro tipo de movimento, o clipe fica
//!   **pausado** em vez de andar no lugar.
//!
//! O grafo de animacao e montado uma vez por TIPO e compartilhado pelas instancias.
//! Se o arquivo falhar ao carregar, o tipo passa a usar a aranha procedural.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use bevy::asset::LoadState;
use bevy::camera::primitives::Aabb;
use bevy::gltf::Gltf;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;

use super::animation::{Clip, clip_for_state, resolve_clip, select_clips};
use super::procedural::{ProceduralAssets, spawn_procedural};
use super::spider::Mob;
use crate::config::Config;
use crate::mobs::{MobCatalog, MobSource};

const CROSSFADE: Duration = Duration::from_millis(180);
/// Envergadura (unidades locais) para a qual o auto-fit reescala o modelo.
pub const FIT_SPAN: f32 = 3.0;

/// Dados de animacao de um tipo GLB (compartilhados).
#[derive(Debug, Clone)]
pub struct GlbKindData {
    graph: Handle<AnimationGraph>,
    scene: Handle<WorldAsset>,
    clips: HashMap<Clip, AnimationNodeIndex>,
    /// Clipes que existem de fato no modelo (os demais usam fallback).
    exact: HashSet<Clip>,
}

/// Marca um mob cujo modelo ainda nao foi anexado.
#[derive(Component)]
pub struct PendingModel;

/// Estado do modelo GLB de um mob.
#[derive(Component)]
pub struct GlbInstance {
    kind: usize,
    holder: Entity,
    scene: Entity,
    ready: bool,
    fitted: bool,
    fit_frames: u32,
    player: Option<Entity>,
    current: Option<Clip>,
}

/// Liga a raiz da cena instanciada ao seu mob.
#[derive(Component)]
pub struct GlbSceneOf(Entity);

fn build_kind_data(
    gltf: &Gltf,
    graphs: &mut Assets<AnimationGraph>,
    label: &str,
) -> Option<GlbKindData> {
    let scene = gltf
        .default_scene
        .clone()
        .or_else(|| gltf.scenes.first().cloned())?;

    let mut names: Vec<&Box<str>> = gltf.named_animations.keys().collect();
    names.sort(); // ordem estavel
    let name_strs: Vec<&str> = names.iter().map(|n| n.as_ref()).collect();
    let chosen = select_clips(&name_strs);

    let mut graph = AnimationGraph::new();
    let mut clips = HashMap::new();
    let mut exact = HashSet::new();
    let mut summary: Vec<(Clip, String)> = Vec::new();
    let mut ordered: Vec<(&Clip, &usize)> = chosen.iter().collect();
    ordered.sort_by_key(|(c, _)| **c as u8);
    for (clip, &idx) in ordered {
        let name = names[idx];
        let node = graph.add_clip(gltf.named_animations[name].clone(), 1.0, graph.root);
        clips.insert(*clip, node);
        exact.insert(*clip);
        summary.push((*clip, name.to_string()));
    }
    info!(
        "GLB \"{label}\" carregado: {} animacoes {:?}; usadas: {summary:?}",
        names.len(),
        name_strs
    );
    for c in Clip::ALL {
        if !exact.contains(&c) {
            let fb = resolve_clip(c, |x| clips.contains_key(&x));
            debug!("\"{label}\": clipe {c:?} ausente; fallback: {fb:?}");
        }
    }
    Some(GlbKindData {
        graph: graphs.add(graph),
        scene,
        clips,
        exact,
    })
}

/// Anexa o modelo aos mobs recem-criados: procedural na hora; GLB quando o arquivo terminar de
/// carregar (ou procedural, se falhar).
#[allow(clippy::too_many_arguments)]
pub fn spawn_pending_models(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    procedural: Res<ProceduralAssets>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut catalog: ResMut<MobCatalog>,
    pending: Query<(Entity, &Mob), With<PendingModel>>,
) {
    for (entity, mob) in &pending {
        let Some(kind) = catalog.kinds.get_mut(mob.kind) else {
            spawn_procedural(&mut commands, entity, &procedural);
            commands.entity(entity).remove::<PendingModel>();
            continue;
        };
        let use_procedural = match (&kind.source, &kind.handle) {
            (MobSource::Procedural, _) => true,
            _ if kind.failed => true,
            (MobSource::Glb(_), None) => true,
            (MobSource::Glb(_), Some(handle)) => match asset_server.load_state(handle) {
                LoadState::Failed(err) => {
                    error!(
                        "falha ao carregar \"{}\" ({err}); usando a aranha procedural",
                        kind.label
                    );
                    kind.failed = true;
                    true
                }
                _ if !asset_server.is_loaded_with_dependencies(handle) => continue, // carregando
                _ => {
                    if kind.glb.is_none() {
                        let built = gltfs
                            .get(handle)
                            .and_then(|g| build_kind_data(g, &mut graphs, &kind.label));
                        if built.is_none() {
                            error!("\"{}\" nao tem cena; usando a aranha procedural", kind.label);
                            kind.failed = true;
                        }
                        kind.glb = built;
                    }
                    kind.glb.is_none()
                }
            },
        };

        commands.entity(entity).remove::<PendingModel>();
        if use_procedural {
            spawn_procedural(&mut commands, entity, &procedural);
            continue;
        }
        let Some(data) = &kind.glb else { continue };

        // `holder` recebe escala/posicao/rotacao finais; oculto ate o auto-fit medir o modelo
        let scene = commands
            .spawn((
                WorldAssetRoot(data.scene.clone()),
                Transform::IDENTITY,
                GlbSceneOf(entity),
            ))
            .observe(on_glb_ready)
            .id();
        let holder = commands
            .spawn((
                Transform::from_rotation(Quat::from_rotation_y(kind.yaw_deg.to_radians())),
                Visibility::Hidden,
            ))
            .add_child(scene)
            .id();
        commands
            .entity(entity)
            .add_child(holder)
            .insert(GlbInstance {
                kind: mob.kind,
                holder,
                scene,
                ready: false,
                fitted: false,
                fit_frames: 0,
                player: None,
                current: None,
            });
    }
}

/// A cena de um mob foi instanciada: liga o `AnimationGraph` ao `AnimationPlayer` dela.
fn on_glb_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    catalog: Res<MobCatalog>,
    scenes: Query<&GlbSceneOf>,
    children: Query<&Children>,
    mut players: Query<&mut AnimationPlayer>,
    mut instances: Query<&mut GlbInstance>,
) {
    let scene = ready.entity;
    let Ok(GlbSceneOf(mob)) = scenes.get(scene) else {
        return;
    };
    let Ok(mut inst) = instances.get_mut(*mob) else {
        return;
    };
    inst.ready = true;
    let Some(data) = catalog.kinds.get(inst.kind).and_then(|k| k.glb.as_ref()) else {
        return;
    };
    let Some(player_entity) = children
        .iter_descendants(scene)
        .find(|e| players.contains(*e))
    else {
        debug!("GLB sem AnimationPlayer: sera exibido sem animacao");
        return;
    };
    let Ok(mut player) = players.get_mut(player_entity) else {
        return;
    };
    let mut transitions = AnimationTransitions::new();
    let start = resolve_clip(Clip::Walk, |c| data.clips.contains_key(&c))
        .or_else(|| resolve_clip(Clip::Idle, |c| data.clips.contains_key(&c)))
        .and_then(|c| data.clips.get(&c).copied());
    if let Some(node) = start {
        transitions.play(&mut player, node, Duration::ZERO).repeat();
    }
    commands
        .entity(player_entity)
        .insert(AnimationGraphHandle(data.graph.clone()))
        .insert(transitions);
    inst.player = Some(player_entity);
}

/// Mede a caixa envolvente de cada modelo recem-instanciado e o reescala/centraliza (auto-fit).
#[allow(clippy::too_many_arguments)]
pub fn fit_glb_model(
    cfg: Res<Config>,
    catalog: Res<MobCatalog>,
    mut instances: Query<&mut GlbInstance>,
    children: Query<&Children>,
    aabbs: Query<(&Aabb, &GlobalTransform)>,
    skins: Query<&SkinnedMesh>,
    transforms: Query<&GlobalTransform>,
    mut holders: Query<(&mut Transform, &mut Visibility), Without<Mob>>,
) {
    for mut inst in &mut instances {
        if !inst.ready || inst.fitted {
            continue;
        }
        // espera a propagacao de transformacoes e o calculo de AABBs
        inst.fit_frames += 1;
        if inst.fit_frames < 4 {
            continue;
        }
        let Ok(scene_gt) = transforms.get(inst.scene) else {
            continue;
        };
        let inv = scene_gt.affine().inverse();

        // Malhas com esqueleto: a AABB em pose de bind nao diz onde o modelo e realmente
        // desenhado (FBX->glTF escala so a cadeia de ossos); mede-se pelos ossos.
        let (mut jmin, mut jmax) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        let mut joints = 0;
        let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        let mut count = 0;
        for e in children.iter_descendants(inst.scene) {
            if let Ok(skin) = skins.get(e) {
                for &j in &skin.joints {
                    if let Ok(gt) = transforms.get(j) {
                        let p = inv.transform_point3(gt.translation());
                        jmin = jmin.min(p);
                        jmax = jmax.max(p);
                        joints += 1;
                    }
                }
            }
            let Ok((aabb, gt)) = aabbs.get(e) else {
                continue;
            };
            let (c, h) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
            for corner in 0..8u8 {
                let sign = Vec3::new(
                    if corner & 1 == 0 { -1.0 } else { 1.0 },
                    if corner & 2 == 0 { -1.0 } else { 1.0 },
                    if corner & 4 == 0 { -1.0 } else { 1.0 },
                );
                let local = inv.transform_point3(gt.affine().transform_point3(c + h * sign));
                min = min.min(local);
                max = max.max(local);
            }
            count += 1;
        }
        let use_joints = joints >= 4 && (jmax - jmin).max_element() > 1e-6;
        if use_joints {
            (min, max) = (jmin, jmax);
        }

        let Ok((mut tf, mut vis)) = holders.get_mut(inst.holder) else {
            continue;
        };
        let (yaw_deg, kind_scale, label) = catalog
            .kinds
            .get(inst.kind)
            .map(|k| (k.yaw_deg, k.scale, k.label.as_str()))
            .unwrap_or((0.0, 1.0, "?"));
        let yaw = Quat::from_rotation_y(yaw_deg.to_radians());
        let extent = max - min;
        let span = extent.x.max(extent.z);
        if count == 0 || !span.is_finite() || span <= 1e-6 {
            if inst.fit_frames > 180 {
                warn!("auto-fit: nao foi possivel medir \"{label}\"; usando escala 1");
                inst.fitted = true;
                *vis = Visibility::Inherited;
            }
            continue;
        }

        let fit = if cfg.spider.glb_auto_fit {
            FIT_SPAN / span
        } else {
            1.0
        };
        let s = fit * kind_scale * cfg.spider.glb_scale;
        let center = Vec3::new((min.x + max.x) * 0.5, min.y, (min.z + max.z) * 0.5);
        tf.scale = Vec3::splat(s);
        tf.rotation = yaw;
        tf.translation = -(yaw * (center * s));
        *vis = Visibility::Inherited;
        inst.fitted = true;
        debug!(
            "auto-fit \"{label}\": {count} malhas, {joints} ossos (por {}), caixa {:.2}x{:.2}x{:.2}, escala {s:.4}, yaw {yaw_deg:.0}",
            if use_joints { "ossos" } else { "AABB" },
            extent.x,
            extent.y,
            extent.z
        );
    }
}

/// Escolhe o clipe de cada mob pelo estado (e pela velocidade real) e troca com cross-fade.
pub fn drive_glb_animation(
    cfg: Res<Config>,
    catalog: Res<MobCatalog>,
    mut mobs: Query<(&Mob, &mut GlbInstance)>,
    mut players: Query<(&mut AnimationPlayer, &mut AnimationTransitions)>,
) {
    for (mob, mut inst) in &mut mobs {
        let (Some(player_entity), Some(data)) = (
            inst.player,
            catalog.kinds.get(inst.kind).and_then(|k| k.glb.as_ref()),
        ) else {
            continue;
        };
        if data.clips.is_empty() {
            continue;
        }
        let Ok((mut player, mut transitions)) = players.get_mut(player_entity) else {
            continue;
        };
        let sim = &mob.sim;
        let mut wanted = clip_for_state(sim.brain.state());
        // parada momentanea (micro-pausa) durante a caminhada: mostra o repouso
        if matches!(wanted, Clip::Walk | Clip::Run | Clip::Climb) && sim.speed_px < 3.0 {
            wanted = Clip::Idle;
        }
        let Some(clip) = resolve_clip(wanted, |c| data.clips.contains_key(&c)) else {
            continue;
        };
        let Some(&node) = data.clips.get(&clip) else {
            continue;
        };
        // sem clipe proprio para repouso/golpe/morte: congela o fallback em vez de "andar no lugar"
        let frozen =
            matches!(wanted, Clip::Idle | Clip::Hit | Clip::Death) && !data.exact.contains(&wanted);

        if inst.current != Some(clip) {
            let active = transitions.play(&mut player, node, CROSSFADE);
            if !clip.is_one_shot() {
                active.repeat();
            }
            inst.current = Some(clip);
        }
        let rate = if frozen {
            0.0
        } else if matches!(clip, Clip::Walk | Clip::Run | Clip::Climb) {
            // velocidade de reproducao proporcional a velocidade real
            let base = sim.base_speed(&cfg).max(1.0);
            (sim.speed_px / base).clamp(0.3, 2.4)
        } else {
            1.0
        };
        if let Some(anim) = player.animation_mut(node) {
            anim.set_speed(rate);
        }
    }
}
