//! Matematica pura da animacao: marcha procedural, IK de duas juntas, orientacao 2.5D e
//! resolucao de clipes (com fallback) para modelos GLB que nao tenham todas as animacoes.
//!
//! Espaco local da aranha: +X = frente, +Y = "cima" (afasta da superficie), +Z = lado (apos a
//! inclinacao, o dorso fica voltado para a camera). A superficie e o plano y = 0.

use bevy::math::{Mat3, Quat, Vec2, Vec3};

use super::behavior::SpiderState;

/// Fracao padrao do ciclo em que o pe esta apoiado no chao.
pub const STANCE_FRACTION: f32 = 0.6;

// ----------------------------------------------------------------------------------------------
// Clipes
// ----------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Clip {
    Idle,
    Walk,
    Run,
    Climb,
    Hit,
    Death,
}

impl Clip {
    pub const ALL: [Clip; 6] = [
        Clip::Idle,
        Clip::Walk,
        Clip::Run,
        Clip::Climb,
        Clip::Hit,
        Clip::Death,
    ];

    /// Clipes aceitaveis, do mais desejado para o menos, quando o modelo nao tem o ideal.
    pub fn fallback_chain(self) -> &'static [Clip] {
        match self {
            Clip::Idle => &[Clip::Idle, Clip::Walk],
            Clip::Walk => &[Clip::Walk, Clip::Run, Clip::Idle],
            Clip::Run => &[Clip::Run, Clip::Walk, Clip::Idle],
            Clip::Climb => &[Clip::Climb, Clip::Walk, Clip::Run, Clip::Idle],
            Clip::Hit => &[Clip::Hit, Clip::Idle],
            Clip::Death => &[Clip::Death, Clip::Hit, Clip::Idle],
        }
    }

    /// Clipes que devem tocar uma unica vez.
    pub fn is_one_shot(self) -> bool {
        matches!(self, Clip::Hit | Clip::Death)
    }
}

/// Clipe desejado para cada estado do comportamento.
pub fn clip_for_state(state: SpiderState) -> Clip {
    match state {
        SpiderState::Idle | SpiderState::Attacking | SpiderState::Respawning => Clip::Idle,
        SpiderState::Walking => Clip::Walk,
        SpiderState::Climbing => Clip::Climb,
        SpiderState::Fleeing => Clip::Run,
        SpiderState::Hit => Clip::Hit,
        SpiderState::Dead => Clip::Death,
    }
}

/// Primeiro clipe da cadeia de fallback que o modelo possui.
pub fn resolve_clip(wanted: Clip, has: impl Fn(Clip) -> bool) -> Option<Clip> {
    wanted.fallback_chain().iter().copied().find(|&c| has(c))
}

/// Classifica o nome de uma animacao do GLB ("Armature|Walk", "Spider walking fast", "dead"...).
/// Compara por **prefixo de palavra**, entao "walking" -> Walk, "running" -> Run e "white" nao
/// vira Hit. A ordem da tabela define a precedencia ("walking fast" -> Run).
pub fn classify_clip_name(name: &str) -> Option<Clip> {
    let lower = name.to_ascii_lowercase();
    let tail = lower.rsplit(['|', '/']).next().unwrap_or(&lower);
    let words: Vec<&str> = tail
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    const TABLE: [(&str, Clip); 17] = [
        ("idle", Clip::Idle),
        ("climb", Clip::Climb),
        ("death", Clip::Death),
        ("dead", Clip::Death),
        ("dying", Clip::Death),
        ("die", Clip::Death),
        ("hurt", Clip::Hit),
        ("damage", Clip::Hit),
        ("hit", Clip::Hit),
        ("run", Clip::Run),
        ("sprint", Clip::Run),
        ("fast", Clip::Run),
        ("walk", Clip::Walk),
        ("crawl", Clip::Walk),
        ("scuttle", Clip::Walk),
        ("move", Clip::Walk),
        ("stand", Clip::Idle),
    ];
    TABLE
        .iter()
        .find(|(key, _)| words.iter().any(|w| w.starts_with(key)))
        .map(|&(_, c)| c)
}

/// Penalidade de variantes que nao servem como clipe "principal" (andar de costas, virar...).
fn variant_penalty(name: &str) -> u32 {
    let n = name.to_ascii_lowercase();
    [
        "back", "turn", "left", "right", "strafe", "jump", "attack", "side",
    ]
    .iter()
    .filter(|k| n.contains(**k))
    .count() as u32
}

/// Escolhe, entre os nomes de animacao de um GLB, o clipe de cada tipo (indices em `names`).
///
/// * por tipo vence quem tem menos penalidade de variante e o nome mais curto
///   ("Spider walking" ganha de "Spider walk and turn left");
/// * se nada foi reconhecido como Walk/Run (modelos com um unico clipe generico, como
///   "Take 001" ou "Scene"), o primeiro clipe nao reconhecido vira Walk.
pub fn select_clips(names: &[&str]) -> std::collections::HashMap<Clip, usize> {
    use std::collections::HashMap;
    let mut best: HashMap<Clip, (u32, usize, usize)> = HashMap::new(); // (penalidade, len, indice)
    let mut unclassified: Vec<usize> = Vec::new();
    for (i, name) in names.iter().enumerate() {
        match classify_clip_name(name) {
            Some(c) => {
                let key = (variant_penalty(name), name.len(), i);
                if best.get(&c).is_none_or(|b| key < *b) {
                    best.insert(c, key);
                }
            }
            None => unclassified.push(i),
        }
    }
    let mut out: HashMap<Clip, usize> = best.into_iter().map(|(c, (_, _, i))| (c, i)).collect();
    if !out.contains_key(&Clip::Walk) && !out.contains_key(&Clip::Run) {
        // o mais curto e mais "generico"; empate pelo indice
        if let Some(&i) = unclassified.iter().min_by_key(|&&i| (names[i].len(), i)) {
            out.insert(Clip::Walk, i);
        }
    }
    out
}

// ----------------------------------------------------------------------------------------------
// Orientacao 2.5D
// ----------------------------------------------------------------------------------------------

/// Converte um vetor de tela (Y para baixo) para o mundo do jogo (Y para cima).
pub fn screen_to_world_dir(v: Vec2) -> Vec3 {
    Vec3::new(v.x, -v.y, 0.0)
}

/// Orientacao da aranha: frente = `heading`, cima = `normal` (ambos no plano da tela, mundo Y-up),
/// com o dorso inclinado `tilt` rad em direcao a camera (+Z) para dar sensacao tridimensional.
pub fn orientation(heading: Vec3, normal: Vec3, tilt: f32) -> Quat {
    let f = heading.try_normalize().unwrap_or(Vec3::X);
    // Gram-Schmidt: garante que "cima" seja perpendicular a "frente"
    let u = (normal - f * f.dot(normal))
        .try_normalize()
        .unwrap_or_else(|| Vec3::new(-f.y, f.x, 0.0));
    let side = f.cross(u); // +-Z
    let base = Quat::from_mat3(&Mat3::from_cols(f, u, side));
    // gira em torno da frente para que o dorso (u) aponte para a camera (+Z)
    let sign = if side.z >= 0.0 { 1.0 } else { -1.0 };
    Quat::from_axis_angle(f, tilt * sign) * base
}

/// Suavizacao exponencial independente de taxa de quadros.
pub fn smooth_factor(rate: f32, dt: f32) -> f32 {
    1.0 - (-rate * dt.max(0.0)).exp()
}

// ----------------------------------------------------------------------------------------------
// Pernas
// ----------------------------------------------------------------------------------------------

/// Repouso de uma perna (espaco local do corpo; o chao e y = 0).
#[derive(Debug, Clone, Copy)]
pub struct LegRest {
    /// Articulacao do quadril, no corpo.
    pub hip: Vec3,
    /// Posicao de repouso da ponta do pe (no chao).
    pub foot: Vec3,
    /// Defasagem do ciclo andando e correndo (voltas).
    pub phase_offset: f32,
    pub phase_run: f32,
    /// +1 / -1: lado do corpo (eixo Z).
    pub side: f32,
    /// Comprimento do femur e da tibia.
    pub l1: f32,
    pub l2: f32,
    /// Para onde o joelho dobra.
    pub pole: Vec3,
    /// Vetor da ponta do pe ate o tornozelo (tarso apoiado); zero = sem tarso.
    pub tarsus: Vec3,
    /// Ponta do pe na pose de morte (pernas encolhidas).
    pub curl: Vec3,
    /// Raio de femur, tibia e tarso.
    pub widths: [f32; 3],
    /// Tinta de cada segmento.
    pub paints: [u8; 3],
    /// Raio da "patinha" na ponta (0 = nenhuma).
    pub paw: f32,
}

/// Deslocamento (frente, cima) da ponta do pe para `phase` (qualquer real; usa a parte
/// fracionaria). `stride` escala a amplitude (0 = parado no repouso), `lift` e a altura do passo e
/// `stance` a fracao do ciclo com o pe no chao.
pub fn step_offset(phase: f32, stride: f32, lift: f32, stance: f32) -> Vec2 {
    let stance = stance.clamp(0.05, 0.95);
    let p = phase.rem_euclid(1.0);
    if p < stance {
        // apoio: o pe anda para tras em relacao ao corpo, em linha reta
        let u = p / stance;
        Vec2::new(0.5 * stride - u * stride, 0.0)
    } else {
        // balanco: sobe e volta para a frente (smoothstep + arco)
        let u = (p - stance) / (1.0 - stance);
        let s = u * u * (3.0 - 2.0 * u);
        Vec2::new(
            -0.5 * stride + s * stride,
            lift * (std::f32::consts::PI * u).sin(),
        )
    }
}

/// IK de duas juntas. Devolve (joelho, pe_efetivo). Se o alvo estiver fora do alcance, o pe e
/// puxado para dentro (a perna nunca se alonga). `pole` indica para onde o joelho dobra.
pub fn two_bone_ik(hip: Vec3, target: Vec3, l1: f32, l2: f32, pole: Vec3) -> (Vec3, Vec3) {
    let to = target - hip;
    let max_reach = (l1 + l2) * 0.999;
    let min_reach = (l1 - l2).abs() + 1e-3;
    let dist = to.length().clamp(min_reach.max(1e-3), max_reach);
    let dir = to.try_normalize().unwrap_or(Vec3::NEG_Y);
    let foot = hip + dir * dist;

    let a = (l1 * l1 - l2 * l2 + dist * dist) / (2.0 * dist);
    let h = (l1 * l1 - a * a).max(0.0).sqrt();
    let perp = (pole - dir * pole.dot(dir))
        .try_normalize()
        .unwrap_or_else(|| dir.any_orthonormal_vector());
    (hip + dir * a + perp * h, foot)
}

/// Frequencia do ciclo de marcha (Hz) para uma velocidade em unidades locais por segundo,
/// de modo que o pe apoiado fique parado no chao (sem "patinar").
pub fn gait_frequency(speed_units: f32, stride: f32, stance: f32) -> f32 {
    if stride <= 1e-4 {
        return 0.0;
    }
    stance * speed_units / stride
}

// ----------------------------------------------------------------------------------------------
// Easing
// ----------------------------------------------------------------------------------------------

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// "Pulinho": ultrapassa 1.0 e volta (usado no reaparecimento).
pub fn ease_out_back(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    const C1: f32 = 1.70158;
    const C3: f32 = C1 + 1.0;
    1.0 + C3 * (t - 1.0).powi(3) + C1 * (t - 1.0).powi(2)
}

pub fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::{FRAC_PI_2, PI};

    // --- clipes ---

    #[test]
    fn clip_names_are_classified() {
        assert_eq!(classify_clip_name("Armature|Walk"), Some(Clip::Walk));
        assert_eq!(classify_clip_name("spider_idle_01"), Some(Clip::Idle));
        assert_eq!(classify_clip_name("RUN"), Some(Clip::Run));
        assert_eq!(classify_clip_name("Climb_Wall"), Some(Clip::Climb));
        assert_eq!(classify_clip_name("Death"), Some(Clip::Death));
        assert_eq!(classify_clip_name("dead"), Some(Clip::Death));
        assert_eq!(classify_clip_name("Hit_React"), Some(Clip::Hit));
        assert_eq!(classify_clip_name("Spider running"), Some(Clip::Run));
        assert_eq!(classify_clip_name("Spider walking fast"), Some(Clip::Run));
        assert_eq!(classify_clip_name("Take 001"), None);
        assert_eq!(classify_clip_name("Scene"), None);
        assert_eq!(
            classify_clip_name("whiteout"),
            None,
            "prefixo de palavra, nao substring"
        );
    }

    fn names_of(
        v: &[&str],
        sel: &std::collections::HashMap<Clip, usize>,
        c: Clip,
    ) -> Option<String> {
        sel.get(&c).map(|&i| v[i].to_string())
    }

    #[test]
    fn wolf_spider_clips_pick_the_plain_walk_not_the_turns() {
        // nomes reais de wolf_spider_rigged_-_rabidosa_rabida.glb
        let v = [
            "Wolf Spider Armature|Spider walking",
            "Wolf Spider Armature|Spider walking fast",
            "Wolf Spider Armature|Spider running",
            "Wolf Spider Armature|Spider walking backword",
            "Wolf Spider Armature|Spider walk and turn left",
            "Wolf Spider Armature|Spider walk and turn right",
            "Wolf Spider Armature|Wolf Spider ArmatureAction",
        ];
        let sel = select_clips(&v);
        assert_eq!(
            names_of(&v, &sel, Clip::Walk).as_deref(),
            Some("Wolf Spider Armature|Spider walking")
        );
        assert_eq!(
            names_of(&v, &sel, Clip::Run).as_deref(),
            Some("Wolf Spider Armature|Spider running")
        );
        assert!(!sel.contains_key(&Clip::Idle) && !sel.contains_key(&Clip::Death));
    }

    #[test]
    fn rat_clips_map_dead_to_death_and_ignore_attack() {
        let v = ["idle", "attack", "dead", "walk"];
        let sel = select_clips(&v);
        assert_eq!(names_of(&v, &sel, Clip::Idle).as_deref(), Some("idle"));
        assert_eq!(names_of(&v, &sel, Clip::Death).as_deref(), Some("dead"));
        assert_eq!(names_of(&v, &sel, Clip::Walk).as_deref(), Some("walk"));
        assert!(!sel.contains_key(&Clip::Hit));
    }

    #[test]
    fn single_generic_clip_becomes_walk() {
        for only in ["Take 001", "Scene", "Modelo Cucaracha"] {
            let v = [only];
            let sel = select_clips(&v);
            assert_eq!(sel.get(&Clip::Walk), Some(&0), "{only}");
            assert_eq!(sel.len(), 1);
        }
        assert!(select_clips(&[]).is_empty());
    }

    #[test]
    fn fallback_chains_end_in_something_available() {
        // modelo so com "walk": tudo cai em walk
        let only_walk = |c: Clip| c == Clip::Walk;
        for c in Clip::ALL {
            let r = resolve_clip(c, only_walk);
            if c == Clip::Death || c == Clip::Hit {
                assert_eq!(r, None, "{c:?} sem hit/idle nao tem fallback");
            } else {
                assert_eq!(r, Some(Clip::Walk), "{c:?}");
            }
        }
        // modelo completo: cada um resolve para si mesmo
        for c in Clip::ALL {
            assert_eq!(resolve_clip(c, |_| true), Some(c));
        }
        // modelo so com idle: tudo vira idle, morte vira idle
        for c in Clip::ALL {
            assert_eq!(
                resolve_clip(c, |x| x == Clip::Idle),
                Some(Clip::Idle),
                "{c:?}"
            );
        }
        // modelo sem nenhuma animacao
        assert_eq!(resolve_clip(Clip::Walk, |_| false), None);
    }

    #[test]
    fn every_state_maps_to_a_clip() {
        use SpiderState::*;
        for s in [
            Idle, Walking, Climbing, Fleeing, Attacking, Hit, Dead, Respawning,
        ] {
            let _ = clip_for_state(s);
        }
        assert_eq!(clip_for_state(Dead), Clip::Death);
        assert_eq!(clip_for_state(Fleeing), Clip::Run);
        assert_eq!(clip_for_state(Climbing), Clip::Climb);
    }

    // --- orientacao ---

    #[test]
    fn orientation_on_floor_going_right_faces_camera_with_tilt() {
        let tilt = 0.6;
        let q = orientation(Vec3::X, Vec3::Y, tilt);
        let f = q * Vec3::X;
        let up = q * Vec3::Y;
        assert!((f - Vec3::X).length() < 1e-5, "frente mantida: {f}");
        assert!(up.z > 0.0, "dorso para a camera: {up}");
        assert!((up.z - tilt.sin()).abs() < 1e-5);
        assert!((up.y - tilt.cos()).abs() < 1e-5);
    }

    #[test]
    fn orientation_turning_around_keeps_back_toward_camera() {
        let tilt = 0.7;
        for (f, u) in [
            (Vec3::X, Vec3::Y),
            (Vec3::NEG_X, Vec3::Y),     // andando para a esquerda no chao
            (Vec3::X, Vec3::NEG_Y),     // pendurada no teto
            (Vec3::Y, Vec3::X),         // escalando parede (normal para a direita)
            (Vec3::NEG_Y, Vec3::NEG_X), // descendo parede esquerda
        ] {
            let q = orientation(f, u, tilt);
            assert!(
                ((q * Vec3::X) - f).length() < 1e-4,
                "frente {f} -> {}",
                q * Vec3::X
            );
            let up = q * Vec3::Y;
            assert!(up.z > 0.5, "f={f} u={u} up={up}");
            // componente no plano continua sendo a normal da superficie
            let planar = Vec3::new(up.x, up.y, 0.0).normalize();
            assert!((planar - u).length() < 1e-4);
        }
    }

    #[test]
    fn orientation_handles_degenerate_input() {
        let q = orientation(Vec3::ZERO, Vec3::ZERO, 0.5);
        assert!(q.is_finite() && q.is_normalized());
        // normal paralela a frente (link diagonal): ainda gera rotacao valida
        let q = orientation(Vec3::X, Vec3::X, 0.5);
        assert!(q.is_finite() && q.is_normalized());
    }

    #[test]
    fn screen_to_world_flips_y() {
        assert_eq!(
            screen_to_world_dir(Vec2::new(1.0, -1.0)),
            Vec3::new(1.0, 1.0, 0.0)
        );
    }

    #[test]
    fn smooth_factor_is_rate_independent_and_bounded() {
        let one = smooth_factor(10.0, 0.1);
        let two = 1.0 - (1.0 - smooth_factor(10.0, 0.05)).powi(2);
        assert!((one - two).abs() < 1e-5);
        assert_eq!(smooth_factor(10.0, 0.0), 0.0);
        assert!(smooth_factor(10.0, 100.0) <= 1.0);
        assert!(smooth_factor(10.0, -1.0) == 0.0);
    }

    // --- pernas ---

    const STRIDE: f32 = 0.7;

    #[test]
    fn feet_touch_the_ground_in_stance_and_lift_in_swing() {
        for i in 0..60 {
            let p = i as f32 / 60.0;
            let o = step_offset(p, STRIDE, 0.3, STANCE_FRACTION);
            if p < STANCE_FRACTION {
                assert_eq!(o.y, 0.0, "apoio no chao (p={p})");
            } else {
                assert!(o.y >= 0.0 && o.y <= 0.3 + 1e-5);
            }
        }
        // meio do balanco: altura maxima
        let mid = step_offset(STANCE_FRACTION + 0.2, STRIDE, 0.3, STANCE_FRACTION);
        assert!((mid.y - 0.3).abs() < 1e-3);
    }

    #[test]
    fn stance_foot_slides_backward_at_body_speed() {
        for stance in [0.5, 0.6] {
            let a = step_offset(0.0, STRIDE, 0.3, stance);
            let b = step_offset(stance - 1e-4, STRIDE, 0.3, stance);
            assert!(((a.x - b.x) - STRIDE).abs() < 1e-2);
            // a frequencia faz o corpo avancar STRIDE no mesmo tempo
            let v = 4.0; // unidades/s
            let freq = gait_frequency(v, STRIDE, stance);
            let stance_time = stance / freq;
            assert!((v * stance_time - STRIDE).abs() < 1e-5);
        }
        assert_eq!(gait_frequency(3.0, 0.0, 0.5), 0.0, "passo nulo nao divide por zero");
    }

    #[test]
    fn swing_is_continuous_with_stance() {
        let s = STANCE_FRACTION;
        let f = |p: f32| step_offset(p, STRIDE, 0.3, s);
        assert!((f(s - 1e-5) - f(s + 1e-5)).length() < 1e-3);
        assert!((f(1.0 - 1e-5) - f(1e-5)).length() < 1e-3);
    }

    #[test]
    fn zero_stride_keeps_feet_at_rest() {
        for p in [0.0, 0.3, 0.7, 0.95] {
            assert!(step_offset(p, 0.0, 0.0, 0.6).length() < 1e-6);
        }
    }

    #[test]
    fn ik_preserves_segment_lengths_when_reachable() {
        let (l1, l2) = (0.6, 0.85);
        let hip = Vec3::new(0.0, 0.55, 0.14);
        for target in [
            Vec3::new(0.5, 0.0, 1.0),
            Vec3::new(-0.7, 0.0, 1.2),
            Vec3::new(0.2, 0.3, 0.8),
        ] {
            let (knee, foot) = two_bone_ik(hip, target, l1, l2, Vec3::Y);
            assert!((hip.distance(knee) - l1).abs() < 1e-4);
            assert!((knee.distance(foot) - l2).abs() < 1e-4);
            assert!(foot.distance(target) < 1e-4, "alvo alcancavel e atingido");
            assert!(knee.y >= hip.y.min(foot.y), "joelho dobra para cima");
        }
    }

    #[test]
    fn ik_clamps_unreachable_targets_without_nan() {
        let (l1, l2) = (0.6, 0.85);
        let hip = Vec3::ZERO;
        let (knee, foot) = two_bone_ik(hip, Vec3::new(10.0, 0.0, 0.0), l1, l2, Vec3::Y);
        assert!(knee.is_finite() && foot.is_finite());
        assert!(foot.length() <= l1 + l2);
        // alvo exatamente no quadril e pole degenerada
        let (k, f) = two_bone_ik(hip, hip, l1, l2, Vec3::ZERO);
        assert!(k.is_finite() && f.is_finite());
        // pole paralela a direcao
        let (k, f) = two_bone_ik(hip, Vec3::new(0.0, -1.2, 0.0), l1, l2, Vec3::NEG_Y);
        assert!(k.is_finite() && f.is_finite());
        let _ = (FRAC_PI_2, PI);
    }

    #[test]
    fn easing_endpoints_and_overshoot() {
        for f in [ease_out_cubic, ease_out_back, smoothstep] {
            assert!(f(0.0).abs() < 1e-6);
            assert!((f(1.0) - 1.0).abs() < 1e-5);
            assert!(f(-5.0).abs() < 1e-6 || f(-5.0) == f(0.0));
            assert!((f(7.0) - 1.0).abs() < 1e-5);
        }
        assert!((0.0..1.0).contains(&ease_out_cubic(0.5)));
        assert!(ease_out_back(0.7) > 1.0, "deve ultrapassar 1.0");
    }
}
