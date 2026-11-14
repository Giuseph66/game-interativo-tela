//! Catalogo de bichos procedurais: cada [`Species`] tem um [`BodyPlan`] (dados puros) com
//! partes rigidas do corpo, pernas com IK de duas juntas (+ tarso), cadeias flexiveis (antenas,
//! cauda, bigodes, palpos), paleta de materiais e um [`Profile`] com tamanho, marcha e hitbox.
//!
//! Espaco local: +X = frente, +Y = cima (afasta da superficie, chao em y = 0), +Z = lado
//! esquerdo. As coordenadas das partes sao absolutas no espaco do corpo; o grupo (corpo, cabeca,
//! traseira) so define em torno de qual pivo a parte se move.

use bevy::math::{EulerRot, Quat, Vec2, Vec3};

use super::animation::LegRest;

/// Maximo de circulos clicaveis por bicho.
pub const MAX_HITS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Species {
    Viuva,
    AranhaLobo,
    Formiga,
    Barata,
    Gafanhoto,
    Rato,
}

impl Species {
    pub const ALL: [Species; 6] = [
        Species::Viuva,
        Species::AranhaLobo,
        Species::Formiga,
        Species::Barata,
        Species::Gafanhoto,
        Species::Rato,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// Nome exibido no painel.
    pub fn label(self) -> &'static str {
        match self {
            Species::Viuva => "Aranha",
            Species::AranhaLobo => "Aranha-lobo",
            Species::Formiga => "Formiga",
            Species::Barata => "Barata",
            Species::Gafanhoto => "Gafanhoto",
            Species::Rato => "Rato",
        }
    }

    /// Outros nomes aceitos em `--spawn` / `mobs.initial_kind` (sem acento, minusculos).
    pub fn aliases(self) -> &'static [&'static str] {
        match self {
            Species::Viuva => &["aranha", "spider", "viuva", "viuva-negra", "procedural"],
            Species::AranhaLobo => &["aranha-lobo", "aranha_lobo", "lobo", "wolf", "wolf_spider"],
            Species::Formiga => &["formiga", "ant"],
            Species::Barata => &["barata", "cockroach", "roach"],
            Species::Gafanhoto => &["gafanhoto", "grasshopper"],
            Species::Rato => &["rato", "rat", "mouse"],
        }
    }

    pub fn from_name(name: &str) -> Option<Species> {
        let n = name.trim().to_ascii_lowercase();
        Species::ALL
            .into_iter()
            .find(|s| s.label().to_ascii_lowercase() == n || s.aliases().contains(&n.as_str()))
    }

    /// Tamanho, marcha e hitbox (sem alocar; copiado para a simulacao de cada mob).
    pub fn profile(self) -> Profile {
        use std::f32::consts::{FRAC_PI_2, PI};
        let hits = |h: &[(f32, f32, f32)]| {
            let mut out = [Vec3::ZERO; MAX_HITS];
            for (o, &(x, y, r)) in out.iter_mut().zip(h) {
                *o = Vec3::new(x, y, r);
            }
            out
        };
        match self {
            Species::Viuva => Profile {
                size: 1.0,
                stride: 0.7,
                run_stride: 0.15,
                run_lo: 4.0,
                run_hi: 8.0,
                stance: 0.6,
                speed_mul: 1.0,
                death_roll: PI,
                hits: hits(&[(-0.1, 0.55, 1.2)]),
            },
            Species::AranhaLobo => Profile {
                size: 1.1,
                stride: 0.7,
                run_stride: 0.25,
                run_lo: 4.0,
                run_hi: 8.0,
                stance: 0.6,
                speed_mul: 1.25,
                death_roll: PI,
                hits: hits(&[(-0.1, 0.48, 1.15)]),
            },
            Species::Formiga => Profile {
                size: 0.8,
                stride: 0.42,
                run_stride: 0.2,
                run_lo: 5.0,
                run_hi: 10.0,
                stance: 0.5,
                speed_mul: 0.9,
                death_roll: PI,
                hits: hits(&[(0.12, 0.36, 0.95)]),
            },
            Species::Barata => Profile {
                size: 1.0,
                stride: 0.5,
                run_stride: 0.35,
                run_lo: 4.0,
                run_hi: 10.0,
                stance: 0.5,
                speed_mul: 1.4,
                death_roll: PI,
                hits: hits(&[(0.4, 0.26, 0.6), (-0.4, 0.26, 0.62)]),
            },
            Species::Gafanhoto => Profile {
                size: 1.0,
                stride: 0.42,
                run_stride: 0.3,
                run_lo: 4.0,
                run_hi: 7.0,
                stance: 0.55,
                speed_mul: 0.8,
                death_roll: PI,
                hits: hits(&[(0.45, 0.45, 0.55), (-0.4, 0.42, 0.6)]),
            },
            Species::Rato => Profile {
                size: 1.5,
                stride: 0.36,
                run_stride: 0.9,
                run_lo: 2.8,
                run_hi: 5.0,
                stance: 0.6,
                speed_mul: 1.1,
                death_roll: FRAC_PI_2,
                hits: hits(&[(0.78, 0.48, 0.36), (0.12, 0.48, 0.45), (-0.5, 0.48, 0.42)]),
            },
        }
    }

    /// Plano completo do corpo (aloca; monte uma vez por especie).
    pub fn plan(self) -> BodyPlan {
        match self {
            Species::Viuva => viuva(),
            Species::AranhaLobo => aranha_lobo(),
            Species::Formiga => formiga(),
            Species::Barata => barata(),
            Species::Gafanhoto => gafanhoto(),
            Species::Rato => rato(),
        }
    }
}

/// Parametros que a simulacao precisa (marcha sem patinar + hitbox).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Profile {
    /// Multiplica `PX_PER_UNIT * spider.scale`.
    pub size: f32,
    /// Passo andando (unidades locais).
    pub stride: f32,
    /// Passo extra ao correr (fracao de `stride`).
    pub run_stride: f32,
    /// Velocidades (unid./s) em que a marcha passa de andar para correr.
    pub run_lo: f32,
    pub run_hi: f32,
    /// Fracao do ciclo com o pe apoiado.
    pub stance: f32,
    /// Multiplica as velocidades de `[spider]`.
    pub speed_mul: f32,
    /// Giro em torno da frente na morte (PI = de barriga para cima).
    pub death_roll: f32,
    /// Circulos clicaveis: (frente, cima, raio) em unidades locais; raio 0 = sem uso.
    pub hits: [Vec3; MAX_HITS],
}

impl Default for Profile {
    fn default() -> Self {
        Species::Viuva.profile()
    }
}

impl Profile {
    /// 0 = andando, 1 = correndo.
    pub fn run_blend(&self, speed_units: f32) -> f32 {
        let span = (self.run_hi - self.run_lo).max(1e-3);
        super::animation::smoothstep((speed_units - self.run_lo) / span)
    }

    /// Passo para uma velocidade (cresce ao correr; a frequencia acompanha, sem patinar).
    pub fn stride_at(&self, speed_units: f32) -> f32 {
        self.stride * (1.0 + self.run_stride * self.run_blend(speed_units))
    }

    pub fn max_stride(&self) -> f32 {
        self.stride * (1.0 + self.run_stride)
    }

    /// Circulos em uso.
    pub fn hit_spots(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.hits.iter().copied().filter(|h| h.z > 0.0)
    }

    /// Circulo que envolve todos os de hit: ((frente, cima), raio).
    pub fn bounds(&self) -> (Vec2, f32) {
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        let mut up = 0.0;
        let mut n = 0.0;
        for h in self.hit_spots() {
            lo = lo.min(h.x - h.z);
            hi = hi.max(h.x + h.z);
            up += h.y;
            n += 1.0;
        }
        if n == 0.0 {
            return (Vec2::ZERO, 1.0);
        }
        let c = Vec2::new((lo + hi) * 0.5, up / n);
        let r = self
            .hit_spots()
            .map(|h| (h.x - c.x).abs() + h.z)
            .fold(0.0, f32::max);
        (c, r)
    }
}

// ----------------------------------------------------------------------------------------------
// Plano do corpo
// ----------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Body,
    Head,
    Rear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Esfera de raio 0.5 (escala = diametros).
    Sphere,
    /// Cone de raio 0.5 e altura 1, ponta em +Y.
    Cone,
}

#[derive(Debug, Clone, Copy)]
pub struct Part {
    pub group: Group,
    pub shape: Shape,
    pub paint: u8,
    pub pos: Vec3,
    pub rot: Quat,
    pub scale: Vec3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainKind {
    Antenna,
    Tail,
    Whisker,
    Palp,
    Cercus,
}

/// Cadeia flexivel de segmentos (antena, cauda...). Animada por ondas em cada junta.
#[derive(Debug, Clone)]
pub struct Chain {
    pub group: Group,
    pub kind: ChainKind,
    pub paint: u8,
    pub base: Vec3,
    /// Direcao de repouso de cada segmento (espaco do grupo, normalizada).
    pub dirs: Vec<Vec3>,
    pub lens: Vec<f32>,
    /// Raio na base e na ponta.
    pub w: (f32, f32),
    pub side: f32,
    /// Mantem os pontos acima do chao (caudas).
    pub ground: bool,
    /// Defasagem para nao balancar em sincronia com a cadeia irma.
    pub phase: f32,
}

impl Chain {
    pub fn segments(&self) -> usize {
        self.dirs.len()
    }

    /// Raio no ponto `i` (0..=segments).
    pub fn width_at(&self, i: usize) -> f32 {
        let n = self.segments().max(1) as f32;
        let t = (i as f32 / n).clamp(0.0, 1.0);
        self.w.0 + (self.w.1 - self.w.0) * t
    }
}

/// Material simples (PBR).
#[derive(Debug, Clone, Copy)]
pub struct Paint {
    pub rgb: [f32; 3],
    pub rough: f32,
    pub metal: f32,
    /// > 0: brilha (sem iluminacao).
    pub glow: f32,
}

const fn paint(rgb: [f32; 3], rough: f32, metal: f32) -> Paint {
    Paint {
        rgb,
        rough,
        metal,
        glow: 0.0,
    }
}

/// Estilo de movimento do corpo.
#[derive(Debug, Clone, Copy)]
pub struct Motion {
    /// Altura do passo.
    pub lift: f32,
    /// Sobe-e-desce do corpo andando.
    pub bob: f32,
    /// Balanco lateral da traseira andando (rad).
    pub sway: f32,
    /// Respiracao (escala Y da traseira).
    pub breath: f32,
    /// Olhar em volta parado (rad).
    pub look: f32,
    /// Farejar (cabeca rapida, rad).
    pub sniff: f32,
    /// Flexao da coluna ao correr (rad).
    pub flex: f32,
    /// Altura do pulo ao correr (gafanhoto).
    pub hop: f32,
    /// Pernas fecham sobre o corpo na morte.
    pub curl_on_death: bool,
}

/// Tudo para montar e animar um bicho.
#[derive(Debug, Clone)]
pub struct BodyPlan {
    pub species: Species,
    pub profile: Profile,
    pub body_height: f32,
    pub head_pivot: Vec3,
    pub rear_pivot: Vec3,
    pub parts: Vec<Part>,
    pub legs: Vec<LegRest>,
    pub chains: Vec<Chain>,
    pub palette: Vec<Paint>,
    pub motion: Motion,
}

// ----------------------------------------------------------------------------------------------
// Construtor
// ----------------------------------------------------------------------------------------------

fn mirror(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, -v.z)
}

/// Rotacao refletida pelo plano z = 0.
fn mirror_q(q: Quat) -> Quat {
    Quat::from_xyzw(-q.x, -q.y, q.z, q.w)
}

/// Rotacao a partir de (inclinacao do nariz, guinada, rolagem) em radianos.
fn rot(pitch: f32, yaw: f32, roll: f32) -> Quat {
    Quat::from_euler(EulerRot::YZX, yaw, pitch, roll)
}

/// Vetor do pe ate o tornozelo: `back` em direcao ao quadril (no plano do chao) e `up` para cima.
fn tarsus_toward_hip(hip: Vec3, foot: Vec3, back: f32, up: f32) -> Vec3 {
    let h = Vec3::new(hip.x - foot.x, 0.0, hip.z - foot.z).normalize_or_zero();
    h * back + Vec3::Y * up
}

/// Uma perna (lado +Z); [`Builder::legs`] cria o par espelhado.
#[derive(Clone, Copy)]
struct LegSpec {
    hip: Vec3,
    foot: Vec3,
    /// Fases andando (lado +Z, lado -Z) e correndo.
    phase: (f32, f32),
    phase_run: (f32, f32),
    l1: f32,
    l2: f32,
    pole: Vec3,
    tarsus: Vec3,
    widths: [f32; 3],
    paints: [u8; 3],
    paw: f32,
}

struct Builder {
    parts: Vec<Part>,
    legs: Vec<LegRest>,
    chains: Vec<Chain>,
}

impl Builder {
    fn new() -> Self {
        Self {
            parts: Vec::new(),
            legs: Vec::new(),
            chains: Vec::new(),
        }
    }

    /// Elipsoide com semi-eixos `r`.
    fn ell(&mut self, g: Group, paint: u8, pos: Vec3, r: Vec3) -> &mut Self {
        self.ell_rot(g, paint, pos, r, Quat::IDENTITY)
    }

    fn ell_rot(&mut self, g: Group, paint: u8, pos: Vec3, r: Vec3, rot: Quat) -> &mut Self {
        self.parts.push(Part {
            group: g,
            shape: Shape::Sphere,
            paint,
            pos,
            rot,
            scale: r * 2.0,
        });
        self
    }

    /// Par espelhado (`pos.z` > 0 e o lado +Z).
    fn ell_pair(&mut self, g: Group, paint: u8, pos: Vec3, r: Vec3, rot: Quat) -> &mut Self {
        self.ell_rot(g, paint, pos, r, rot);
        self.ell_rot(g, paint, mirror(pos), r, mirror_q(rot))
    }

    /// Cone da base ate a ponta.
    fn cone(&mut self, g: Group, paint: u8, base: Vec3, tip: Vec3, radius: f32) -> &mut Self {
        let d = tip - base;
        let len = d.length().max(1e-4);
        self.parts.push(Part {
            group: g,
            shape: Shape::Cone,
            paint,
            pos: (base + tip) * 0.5,
            rot: Quat::from_rotation_arc(Vec3::Y, d / len),
            scale: Vec3::new(radius * 2.0, len, radius * 2.0),
        });
        self
    }

    fn cone_pair(&mut self, g: Group, paint: u8, base: Vec3, tip: Vec3, radius: f32) -> &mut Self {
        self.cone(g, paint, base, tip, radius);
        self.cone(g, paint, mirror(base), mirror(tip), radius)
    }

    fn legs(&mut self, s: LegSpec) -> &mut Self {
        for side in [1.0f32, -1.0] {
            let m = |v: Vec3| if side > 0.0 { v } else { mirror(v) };
            let hip = m(s.hip);
            let reach = s.l1 + s.l2;
            let curl = Vec3::new(
                hip.x * 1.3,
                hip.y + 0.32 * reach,
                side * (hip.z.abs() + 0.12 * reach),
            );
            self.legs.push(LegRest {
                hip,
                foot: m(s.foot),
                phase_offset: if side > 0.0 { s.phase.0 } else { s.phase.1 },
                phase_run: if side > 0.0 {
                    s.phase_run.0
                } else {
                    s.phase_run.1
                },
                side,
                l1: s.l1,
                l2: s.l2,
                pole: m(s.pole),
                tarsus: m(s.tarsus),
                curl,
                widths: s.widths,
                paints: s.paints,
                paw: s.paw,
            });
        }
        self
    }

    #[allow(clippy::too_many_arguments)]
    fn chain(
        &mut self,
        g: Group,
        kind: ChainKind,
        paint: u8,
        base: Vec3,
        dirs: Vec<Vec3>,
        lens: Vec<f32>,
        w: (f32, f32),
        ground: bool,
        phase: f32,
    ) -> &mut Self {
        let side = if base.z > 1e-4 {
            1.0
        } else if base.z < -1e-4 {
            -1.0
        } else {
            0.0
        };
        self.chains.push(Chain {
            group: g,
            kind,
            paint,
            base,
            dirs: dirs.into_iter().map(|d| d.normalize_or(Vec3::X)).collect(),
            lens,
            w,
            side,
            ground,
            phase,
        });
        self
    }

    /// Par espelhado de cadeias (lado +Z como modelo).
    #[allow(clippy::too_many_arguments)]
    fn chain_pair(
        &mut self,
        g: Group,
        kind: ChainKind,
        paint: u8,
        base: Vec3,
        dirs: Vec<Vec3>,
        lens: Vec<f32>,
        w: (f32, f32),
    ) -> &mut Self {
        let mirrored: Vec<Vec3> = dirs.iter().copied().map(mirror).collect();
        self.chain(g, kind, paint, base, dirs, lens.clone(), w, false, 0.0);
        self.chain(g, kind, paint, mirror(base), mirrored, lens, w, false, 1.7)
    }

    #[allow(clippy::too_many_arguments)]
    fn finish(
        self,
        species: Species,
        body_height: f32,
        head_pivot: Vec3,
        rear_pivot: Vec3,
        palette: Vec<Paint>,
        motion: Motion,
    ) -> BodyPlan {
        BodyPlan {
            species,
            profile: species.profile(),
            body_height,
            head_pivot,
            rear_pivot,
            parts: self.parts,
            legs: self.legs,
            chains: self.chains,
            palette,
            motion,
        }
    }
}

/// Curva: `n` direcoes, cada uma girada `turn` em relacao a anterior.
fn curve(start: Vec3, turn: Quat, n: usize) -> Vec<Vec3> {
    let mut d = start.normalize_or(Vec3::X);
    (0..n)
        .map(|_| {
            let cur = d;
            d = turn * d;
            cur
        })
        .collect()
}

/// Defasagens de marcha de oito pernas em tetrapodes alternados (pares 0..3, frente para tras).
fn tetrapod(pair: usize) -> (f32, f32) {
    if pair.is_multiple_of(2) {
        (0.0, 0.5)
    } else {
        (0.5, 0.0)
    }
}

/// Tripe alternado de insetos: E1, D2, E3 juntos; D1, E2, D3 juntos.
fn tripod(pair: usize) -> (f32, f32) {
    tetrapod(pair)
}

// ----------------------------------------------------------------------------------------------
// Aranhas
// ----------------------------------------------------------------------------------------------

struct SpiderLegs {
    h: f32,
    hip_x: [f32; 4],
    hip_z: f32,
    foot_x: [f32; 4],
    foot_z: [f32; 4],
    l1: [f32; 4],
    l2: [f32; 4],
    widths: [f32; 3],
    paints: [u8; 3],
}

fn spider_legs(b: &mut Builder, s: SpiderLegs) {
    for pair in 0..4 {
        let hip = Vec3::new(s.hip_x[pair], s.h, s.hip_z);
        let foot = Vec3::new(s.foot_x[pair], 0.0, s.foot_z[pair]);
        let ph = tetrapod(pair);
        b.legs(LegSpec {
            hip,
            foot,
            phase: ph,
            phase_run: ph,
            l1: s.l1[pair],
            l2: s.l2[pair],
            pole: Vec3::new(0.0, 1.0, 0.35),
            tarsus: tarsus_toward_hip(hip, foot, 0.1, 0.1),
            widths: s.widths,
            paints: s.paints,
            paw: 0.0,
        });
    }
}

fn viuva() -> BodyPlan {
    use Group::*;
    let h = 0.55;
    let mut b = Builder::new();
    // cefalotorax + abdomen globoso preto brilhante
    b.ell(Body, 0, Vec3::new(0.18, h + 0.02, 0.0), Vec3::new(0.31, 0.2, 0.26))
        .ell(Rear, 0, Vec3::new(-0.42, h + 0.1, 0.0), Vec3::new(0.48, 0.4, 0.4))
        // ampulheta vermelha (vista de cima/traseira) + pontinhos
        .ell(Rear, 3, Vec3::new(-0.42, h + 0.48, 0.0), Vec3::new(0.12, 0.035, 0.07))
        .ell(Rear, 3, Vec3::new(-0.6, h + 0.43, 0.0), Vec3::new(0.08, 0.035, 0.05))
        .ell(Rear, 3, Vec3::new(-0.24, h + 0.43, 0.0), Vec3::new(0.07, 0.035, 0.045))
        // fiandeiras
        .cone(Rear, 4, Vec3::new(-0.86, h + 0.06, 0.0), Vec3::new(-0.97, h + 0.02, 0.0), 0.05)
        // olhos (2 grandes + 2 pequenos) e queliceras
        .ell_pair(Head, 2, Vec3::new(0.45, h + 0.13, 0.06), Vec3::splat(0.045), Quat::IDENTITY)
        .ell_pair(Head, 2, Vec3::new(0.4, h + 0.17, 0.1), Vec3::splat(0.028), Quat::IDENTITY)
        .cone_pair(
            Head,
            4,
            Vec3::new(0.44, h - 0.02, 0.05),
            Vec3::new(0.55, h - 0.17, 0.035),
            0.045,
        );
    // palpos (pernas curtas na frente)
    b.chain_pair(
        Head,
        ChainKind::Palp,
        1,
        Vec3::new(0.42, h - 0.03, 0.09),
        vec![Vec3::new(1.0, -0.2, 0.35), Vec3::new(0.8, -0.9, 0.1)],
        vec![0.15, 0.13],
        (0.035, 0.025),
    );
    spider_legs(
        &mut b,
        SpiderLegs {
            h,
            hip_x: [0.3, 0.11, -0.1, -0.3],
            hip_z: 0.14,
            foot_x: [0.75, 0.4, -0.3, -0.72],
            foot_z: [0.7, 0.9, 0.9, 0.72],
            l1: [0.62, 0.55, 0.55, 0.62],
            l2: [0.78, 0.7, 0.7, 0.78],
            widths: [0.045, 0.032, 0.02],
            paints: [1, 1, 1],
        },
    );
    b.finish(
        Species::Viuva,
        h,
        Vec3::new(0.3, h, 0.0),
        Vec3::new(-0.02, h + 0.04, 0.0),
        vec![
            paint([0.035, 0.03, 0.035], 0.22, 0.15),  // 0 corpo preto brilhante
            paint([0.06, 0.05, 0.05], 0.35, 0.1),     // 1 pernas
            Paint {
                rgb: [0.9, 0.1, 0.05],
                rough: 0.5,
                metal: 0.0,
                glow: 6.0,
            }, // 2 olhos
            paint([0.82, 0.05, 0.03], 0.35, 0.0),     // 3 ampulheta
            paint([0.14, 0.06, 0.04], 0.4, 0.0),      // 4 queliceras
        ],
        Motion {
            lift: 0.4,
            bob: 0.025,
            sway: 0.05,
            breath: 0.035,
            look: 0.0,
            sniff: 0.0,
            flex: 0.0,
            hop: 0.0,
            curl_on_death: true,
        },
    )
}

fn aranha_lobo() -> BodyPlan {
    use Group::*;
    let h = 0.48;
    let mut b = Builder::new();
    // cefalotorax marrom com faixa clara central
    b.ell_rot(
        Body,
        0,
        Vec3::new(0.12, h + 0.04, 0.0),
        Vec3::new(0.36, 0.2, 0.27),
        rot(0.08, 0.0, 0.0),
    )
    .ell(Body, 2, Vec3::new(0.12, h + 0.2, 0.0), Vec3::new(0.3, 0.05, 0.07))
    .ell_pair(Body, 1, Vec3::new(0.1, h + 0.17, 0.15), Vec3::new(0.26, 0.05, 0.05), Quat::IDENTITY)
    // cabeca elevada com os olhos grandes
    .ell(Head, 0, Vec3::new(0.42, h + 0.1, 0.0), Vec3::new(0.16, 0.15, 0.17))
    .ell_pair(Head, 3, Vec3::new(0.48, h + 0.23, 0.065), Vec3::splat(0.05), Quat::IDENTITY)
    .ell_pair(Head, 3, Vec3::new(0.56, h + 0.15, 0.05), Vec3::splat(0.032), Quat::IDENTITY)
    .ell_pair(Head, 3, Vec3::new(0.58, h + 0.07, 0.03), Vec3::splat(0.02), Quat::IDENTITY)
    .ell_pair(Head, 3, Vec3::new(0.57, h + 0.07, 0.085), Vec3::splat(0.02), Quat::IDENTITY)
    .cone_pair(
        Head,
        1,
        Vec3::new(0.55, h + 0.0, 0.05),
        Vec3::new(0.62, h - 0.15, 0.04),
        0.055,
    )
    // abdomen oval com marca cardiaca escura e divisas claras
    .ell(Rear, 0, Vec3::new(-0.45, h + 0.06, 0.0), Vec3::new(0.42, 0.29, 0.3))
    .ell(Rear, 1, Vec3::new(-0.36, h + 0.3, 0.0), Vec3::new(0.24, 0.05, 0.08))
    .ell_pair(Rear, 2, Vec3::new(-0.62, h + 0.26, 0.08), Vec3::new(0.1, 0.04, 0.06), rot(0.0, 0.5, 0.0))
    .ell_pair(Rear, 2, Vec3::new(-0.74, h + 0.2, 0.06), Vec3::new(0.07, 0.035, 0.05), rot(0.0, 0.5, 0.0))
    .cone(Rear, 1, Vec3::new(-0.84, h + 0.04, 0.0), Vec3::new(-0.94, h + 0.0, 0.0), 0.05);
    b.chain_pair(
        Head,
        ChainKind::Palp,
        4,
        Vec3::new(0.5, h - 0.02, 0.1),
        vec![Vec3::new(1.0, 0.1, 0.35), Vec3::new(0.9, -0.8, 0.1)],
        vec![0.16, 0.14],
        (0.045, 0.035),
    );
    spider_legs(
        &mut b,
        SpiderLegs {
            h,
            hip_x: [0.3, 0.16, 0.02, -0.14],
            hip_z: 0.16,
            foot_x: [0.82, 0.42, -0.28, -0.82],
            foot_z: [0.66, 0.84, 0.84, 0.7],
            l1: [0.55, 0.5, 0.5, 0.6],
            l2: [0.7, 0.62, 0.62, 0.75],
            widths: [0.06, 0.045, 0.03],
            paints: [4, 5, 4],
        },
    );
    b.finish(
        Species::AranhaLobo,
        h,
        Vec3::new(0.3, h + 0.04, 0.0),
        Vec3::new(-0.1, h + 0.04, 0.0),
        vec![
            paint([0.3, 0.2, 0.12], 0.8, 0.0),   // 0 marrom
            paint([0.12, 0.08, 0.05], 0.8, 0.0), // 1 marrom escuro
            paint([0.6, 0.48, 0.32], 0.85, 0.0), // 2 faixa clara
            paint([0.01, 0.01, 0.012], 0.05, 0.5), // 3 olhos pretos brilhantes
            paint([0.34, 0.24, 0.15], 0.75, 0.0), // 4 perna clara
            paint([0.15, 0.1, 0.06], 0.75, 0.0), // 5 perna escura (anelada)
        ],
        Motion {
            lift: 0.38,
            bob: 0.02,
            sway: 0.04,
            breath: 0.03,
            look: 0.0,
            sniff: 0.0,
            flex: 0.0,
            hop: 0.0,
            curl_on_death: true,
        },
    )
}

// ----------------------------------------------------------------------------------------------
// Insetos
// ----------------------------------------------------------------------------------------------

/// Pernas de inseto: 3 pares (frente, meio, tras) em tripe.
#[allow(clippy::too_many_arguments)]
fn insect_legs(
    b: &mut Builder,
    hips: [Vec3; 3],
    feet: [Vec3; 3],
    l1: [f32; 3],
    l2: [f32; 3],
    poles: [Vec3; 3],
    tarsus: [(f32, f32); 3],
    widths: [[f32; 3]; 3],
    paints: [[u8; 3]; 3],
) {
    for pair in 0..3 {
        let ph = tripod(pair);
        b.legs(LegSpec {
            hip: hips[pair],
            foot: feet[pair],
            phase: ph,
            phase_run: ph,
            l1: l1[pair],
            l2: l2[pair],
            pole: poles[pair],
            tarsus: tarsus_toward_hip(hips[pair], feet[pair], tarsus[pair].0, tarsus[pair].1),
            widths: widths[pair],
            paints: paints[pair],
            paw: 0.0,
        });
    }
}

fn formiga() -> BodyPlan {
    use Group::*;
    let h = 0.36;
    let mut b = Builder::new();
    // cabeca (pivo no pescoco), olhos, mandibulas
    b.ell_rot(
        Head,
        0,
        Vec3::new(0.64, h + 0.06, 0.0),
        Vec3::new(0.18, 0.15, 0.17),
        rot(-0.2, 0.0, 0.0),
    )
    .ell_pair(Head, 3, Vec3::new(0.67, h + 0.11, 0.14), Vec3::new(0.05, 0.045, 0.035), Quat::IDENTITY)
    .cone_pair(
        Head,
        4,
        Vec3::new(0.78, h - 0.02, 0.08),
        Vec3::new(0.9, h - 0.04, 0.015),
        0.035,
    )
    // mesossoma em tres gomos
    .ell(Body, 0, Vec3::new(0.38, h + 0.05, 0.0), Vec3::new(0.13, 0.11, 0.11))
    .ell(Body, 0, Vec3::new(0.22, h + 0.03, 0.0), Vec3::new(0.12, 0.09, 0.09))
    .ell(Body, 0, Vec3::new(0.08, h + 0.05, 0.0), Vec3::new(0.09, 0.09, 0.08))
    // peciolo e gaster
    .ell(Rear, 0, Vec3::new(-0.05, h + 0.07, 0.0), Vec3::new(0.045, 0.075, 0.045))
    .ell_rot(
        Rear,
        1,
        Vec3::new(-0.36, h + 0.07, 0.0),
        Vec3::new(0.27, 0.2, 0.21),
        rot(-0.12, 0.0, 0.0),
    )
    .ell_rot(
        Rear,
        1,
        Vec3::new(-0.3, h + 0.2, 0.0),
        Vec3::new(0.17, 0.06, 0.15),
        rot(-0.12, 0.0, 0.0),
    );
    // antenas cotoveladas: escapo para cima/fora, funiculo para a frente
    let mut dirs = vec![Vec3::new(0.25, 0.75, 0.6)];
    dirs.extend(curve(
        Vec3::new(1.0, 0.1, 0.35),
        Quat::from_rotation_z(-0.12),
        5,
    ));
    b.chain_pair(
        Head,
        ChainKind::Antenna,
        2,
        Vec3::new(0.74, h + 0.14, 0.05),
        dirs,
        vec![0.26, 0.08, 0.08, 0.08, 0.08, 0.1],
        (0.022, 0.03),
    );
    let hy = h - 0.04;
    insect_legs(
        &mut b,
        [
            Vec3::new(0.34, hy, 0.07),
            Vec3::new(0.2, hy, 0.07),
            Vec3::new(0.07, hy, 0.07),
        ],
        [
            Vec3::new(0.66, 0.0, 0.4),
            Vec3::new(0.16, 0.0, 0.5),
            Vec3::new(-0.36, 0.0, 0.46),
        ],
        [0.38, 0.33, 0.42],
        [0.4, 0.37, 0.47],
        [
            Vec3::new(0.3, 1.0, 0.5),
            Vec3::new(0.0, 1.0, 0.6),
            Vec3::new(-0.3, 1.0, 0.5),
        ],
        [(0.08, 0.06); 3],
        [[0.035, 0.024, 0.016]; 3],
        [[2, 2, 2]; 3],
    );
    b.finish(
        Species::Formiga,
        h,
        Vec3::new(0.5, h + 0.04, 0.0),
        Vec3::new(0.0, h + 0.05, 0.0),
        vec![
            paint([0.42, 0.11, 0.04], 0.3, 0.05),  // 0 cabeca/torax ruivo
            paint([0.11, 0.04, 0.03], 0.22, 0.1),  // 1 gaster escuro
            paint([0.32, 0.09, 0.04], 0.4, 0.0),   // 2 pernas/antenas
            paint([0.01, 0.01, 0.01], 0.08, 0.4),  // 3 olhos
            paint([0.16, 0.06, 0.03], 0.35, 0.0),  // 4 mandibulas
        ],
        Motion {
            lift: 0.16,
            bob: 0.012,
            sway: 0.06,
            breath: 0.02,
            look: 0.25,
            sniff: 0.0,
            flex: 0.0,
            hop: 0.0,
            curl_on_death: true,
        },
    )
}

fn barata() -> BodyPlan {
    use Group::*;
    let h = 0.26;
    let mut b = Builder::new();
    // abdomen por baixo das asas
    b.ell(Body, 3, Vec3::new(-0.22, h - 0.02, 0.0), Vec3::new(0.62, 0.09, 0.3))
        // tegminas (asas) brilhantes, levemente sobrepostas
        .ell_pair(
            Body,
            0,
            Vec3::new(-0.3, h + 0.07, 0.1),
            Vec3::new(0.72, 0.06, 0.2),
            rot(-0.03, 0.05, -0.08),
        )
        // pronoto: escudo claro com o centro escuro
        .ell(Body, 1, Vec3::new(0.42, h + 0.07, 0.0), Vec3::new(0.29, 0.06, 0.32))
        .ell(Body, 2, Vec3::new(0.42, h + 0.1, 0.0), Vec3::new(0.18, 0.045, 0.19))
        // cabeca sob o pronoto, olhos
        .ell_rot(
            Head,
            2,
            Vec3::new(0.7, h - 0.01, 0.0),
            Vec3::new(0.12, 0.1, 0.14),
            rot(-0.4, 0.0, 0.0),
        )
        .ell_pair(Head, 5, Vec3::new(0.72, h + 0.03, 0.11), Vec3::new(0.04, 0.05, 0.03), Quat::IDENTITY)
        // ponta do abdomen
        .ell(Rear, 3, Vec3::new(-0.8, h - 0.01, 0.0), Vec3::new(0.12, 0.06, 0.14));
    // antenas longas que curvam para tras e para fora
    b.chain_pair(
        Head,
        ChainKind::Antenna,
        4,
        Vec3::new(0.8, h + 0.02, 0.06),
        curve(
            Vec3::new(1.0, 0.18, 0.3),
            Quat::from_rotation_y(-0.09),
            12,
        ),
        vec![0.13; 12],
        (0.02, 0.008),
    );
    // cercos
    b.chain_pair(
        Rear,
        ChainKind::Cercus,
        3,
        Vec3::new(-0.88, h, 0.06),
        vec![Vec3::new(-1.0, 0.15, 0.4), Vec3::new(-1.0, 0.1, 0.5)],
        vec![0.08, 0.07],
        (0.022, 0.01),
    );
    let hy = h - 0.05;
    insect_legs(
        &mut b,
        [
            Vec3::new(0.36, hy, 0.1),
            Vec3::new(0.14, hy, 0.12),
            Vec3::new(-0.04, hy, 0.12),
        ],
        [
            Vec3::new(0.72, 0.0, 0.48),
            Vec3::new(0.18, 0.0, 0.7),
            Vec3::new(-0.6, 0.0, 0.7),
        ],
        [0.32, 0.38, 0.48],
        [0.44, 0.48, 0.62],
        [
            Vec3::new(0.2, 0.7, 1.0),
            Vec3::new(0.0, 0.7, 1.0),
            Vec3::new(-0.2, 0.7, 1.0),
        ],
        [(0.1, 0.05); 3],
        [[0.045, 0.03, 0.018], [0.05, 0.032, 0.018], [0.055, 0.035, 0.02]],
        [[4, 4, 4]; 3],
    );
    b.finish(
        Species::Barata,
        h,
        Vec3::new(0.62, h, 0.0),
        Vec3::new(-0.6, h, 0.0),
        vec![
            paint([0.38, 0.15, 0.05], 0.2, 0.15), // 0 asas castanho-avermelhadas
            paint([0.72, 0.54, 0.28], 0.35, 0.05), // 1 borda clara do pronoto
            paint([0.26, 0.1, 0.04], 0.25, 0.1),  // 2 centro escuro / cabeca
            paint([0.2, 0.09, 0.04], 0.4, 0.0),   // 3 corpo
            paint([0.36, 0.18, 0.08], 0.4, 0.0),  // 4 pernas/antenas
            paint([0.01, 0.01, 0.01], 0.1, 0.4),  // 5 olhos
        ],
        Motion {
            lift: 0.14,
            bob: 0.008,
            sway: 0.0,
            breath: 0.0,
            look: 0.12,
            sniff: 0.0,
            flex: 0.0,
            hop: 0.0,
            curl_on_death: true,
        },
    )
}

fn gafanhoto() -> BodyPlan {
    use Group::*;
    let h = 0.4;
    let mut b = Builder::new();
    // cabeca alta ("cara de cavalo"), olhos compostos, boca
    b.ell_rot(
        Head,
        0,
        Vec3::new(0.78, h + 0.1, 0.0),
        Vec3::new(0.16, 0.21, 0.16),
        rot(-0.35, 0.0, 0.0),
    )
    .ell_pair(Head, 3, Vec3::new(0.83, h + 0.2, 0.12), Vec3::new(0.075, 0.095, 0.055), Quat::IDENTITY)
    .ell(Head, 4, Vec3::new(0.87, h - 0.07, 0.0), Vec3::new(0.06, 0.05, 0.07))
    // pronoto em sela com listras laterais
    .ell(Body, 0, Vec3::new(0.5, h + 0.12, 0.0), Vec3::new(0.22, 0.19, 0.18))
    .ell_pair(Body, 4, Vec3::new(0.5, h + 0.13, 0.16), Vec3::new(0.18, 0.035, 0.03), Quat::IDENTITY)
    .ell(Body, 0, Vec3::new(0.26, h + 0.05, 0.0), Vec3::new(0.2, 0.16, 0.15))
    // abdomen comprido
    .ell_rot(
        Rear,
        1,
        Vec3::new(-0.32, h + 0.02, 0.0),
        Vec3::new(0.6, 0.14, 0.14),
        rot(0.05, 0.0, 0.0),
    )
    // asas (tegminas) ao longo do dorso, passando da ponta do abdomen
    .ell_pair(
        Body,
        2,
        Vec3::new(-0.3, h + 0.17, 0.06),
        Vec3::new(0.76, 0.045, 0.1),
        rot(-0.07, 0.0, -0.25),
    );
    b.chain_pair(
        Head,
        ChainKind::Antenna,
        0,
        Vec3::new(0.88, h + 0.27, 0.05),
        curve(
            Vec3::new(1.0, 0.55, 0.35),
            Quat::from_rotation_z(-0.06),
            7,
        ),
        vec![0.09; 7],
        (0.02, 0.012),
    );
    let hy = h - 0.08;
    // pernas da frente e do meio (pequenas)
    for (pair, (hip, foot)) in [
        (Vec3::new(0.48, hy, 0.08), Vec3::new(0.78, 0.0, 0.32)),
        (Vec3::new(0.3, hy, 0.1), Vec3::new(0.26, 0.0, 0.42)),
    ]
    .into_iter()
    .enumerate()
    {
        let ph = tripod(pair);
        b.legs(LegSpec {
            hip,
            foot,
            phase: ph,
            phase_run: ph,
            l1: 0.3,
            l2: 0.34,
            pole: Vec3::new(0.1, 1.0, 0.6),
            tarsus: tarsus_toward_hip(hip, foot, 0.06, 0.04),
            widths: [0.04, 0.028, 0.02],
            paints: [0, 0, 4],
            paw: 0.0,
        });
    }
    // pernas saltadoras: femur grosso para tras/para cima, tibia fina descendo para a frente
    let hip = Vec3::new(0.15, h - 0.02, 0.15);
    let foot = Vec3::new(-0.2, 0.0, 0.36);
    let ph = tripod(2);
    b.legs(LegSpec {
        hip,
        foot,
        phase: ph,
        phase_run: (0.0, 0.0),
        l1: 0.72,
        l2: 0.7,
        pole: Vec3::new(-1.0, 1.2, 0.3),
        tarsus: Vec3::new(0.12, 0.04, 0.0),
        widths: [0.11, 0.035, 0.022],
        paints: [0, 4, 4],
        paw: 0.0,
    });
    b.finish(
        Species::Gafanhoto,
        h,
        Vec3::new(0.62, h + 0.1, 0.0),
        Vec3::new(0.2, h + 0.04, 0.0),
        vec![
            paint([0.33, 0.52, 0.13], 0.55, 0.0), // 0 verde
            paint([0.5, 0.64, 0.22], 0.6, 0.0),   // 1 verde claro (abdomen)
            paint([0.4, 0.42, 0.18], 0.45, 0.0),  // 2 asas
            paint([0.28, 0.2, 0.1], 0.15, 0.2),   // 3 olhos
            paint([0.16, 0.24, 0.07], 0.6, 0.0),  // 4 detalhes escuros
        ],
        Motion {
            lift: 0.14,
            bob: 0.01,
            sway: 0.02,
            breath: 0.03,
            look: 0.18,
            sniff: 0.0,
            flex: 0.0,
            hop: 0.55,
            curl_on_death: true,
        },
    )
}

// ----------------------------------------------------------------------------------------------
// Rato
// ----------------------------------------------------------------------------------------------

fn rato() -> BodyPlan {
    use Group::*;
    let h = 0.42;
    let mut b = Builder::new();
    // tronco: peito, corpo, quadris, barriga clara
    b.ell(Body, 0, Vec3::new(-0.1, h + 0.07, 0.0), Vec3::new(0.55, 0.3, 0.3))
        .ell(Body, 0, Vec3::new(-0.42, h + 0.06, 0.0), Vec3::new(0.36, 0.32, 0.33))
        .ell(Body, 0, Vec3::new(0.28, h + 0.03, 0.0), Vec3::new(0.3, 0.25, 0.25))
        .ell(Body, 1, Vec3::new(-0.05, h - 0.1, 0.0), Vec3::new(0.5, 0.17, 0.23))
        // cabeca: cranio, focinho, nariz, olhos, orelhas
        .ell(Head, 0, Vec3::new(0.66, h + 0.09, 0.0), Vec3::new(0.24, 0.19, 0.2))
        .ell_rot(
            Head,
            0,
            Vec3::new(0.88, h + 0.03, 0.0),
            Vec3::new(0.17, 0.12, 0.12),
            rot(-0.15, 0.0, 0.0),
        )
        .ell(Head, 2, Vec3::new(1.04, h + 0.02, 0.0), Vec3::splat(0.045))
        .ell_pair(Head, 3, Vec3::new(0.78, h + 0.17, 0.12), Vec3::splat(0.045), Quat::IDENTITY)
        .ell_pair(
            Head,
            2,
            Vec3::new(0.6, h + 0.28, 0.15),
            Vec3::new(0.05, 0.13, 0.12),
            rot(0.0, 0.3, 0.45),
        )
        .ell_pair(
            Head,
            5,
            Vec3::new(0.63, h + 0.28, 0.155),
            Vec3::new(0.03, 0.1, 0.09),
            rot(0.0, 0.3, 0.45),
        );
    // bigodes (3 por lado)
    for (i, dy) in [0.12f32, 0.0, -0.12].into_iter().enumerate() {
        let base = Vec3::new(0.98, h + 0.03, 0.07);
        let d = Vec3::new(0.45, dy, 1.0);
        b.chain(Head, ChainKind::Whisker, 4, base, vec![d, d], vec![0.16, 0.16], (0.012, 0.006), false, i as f32);
        b.chain(
            Head,
            ChainKind::Whisker,
            4,
            mirror(base),
            vec![mirror(d), mirror(d)],
            vec![0.16, 0.16],
            (0.012, 0.006),
            false,
            i as f32 + 1.3,
        );
    }
    // cauda longa e rosada, encostando no chao
    let mut tail = vec![Vec3::new(-1.0, -0.55, 0.0), Vec3::new(-1.0, -0.35, 0.0)];
    tail.extend(std::iter::repeat_n(Vec3::new(-1.0, -0.05, 0.0), 10));
    b.chain(
        Body,
        ChainKind::Tail,
        2,
        Vec3::new(-0.74, h - 0.02, 0.0),
        tail,
        vec![0.14; 12],
        (0.065, 0.02),
        true,
        0.0,
    );
    // patas da frente (cotovelo para tras) e de tras (joelho para a frente, pe plantigrado)
    // marcha andando em sequencia lateral; correndo em meio-galope (pares quase juntos)
    let front_hip = Vec3::new(0.3, h - 0.1, 0.16);
    let front_foot = Vec3::new(0.46, 0.0, 0.2);
    b.legs(LegSpec {
        hip: front_hip,
        foot: front_foot,
        phase: (0.25, 0.75),
        phase_run: (0.5, 0.6),
        l1: 0.22,
        l2: 0.22,
        pole: Vec3::new(-1.0, 0.0, 0.15),
        tarsus: Vec3::new(-0.07, 0.05, 0.0),
        widths: [0.075, 0.05, 0.035],
        paints: [0, 0, 2],
        paw: 0.045,
    });
    b.legs(LegSpec {
        hip: Vec3::new(-0.42, h - 0.06, 0.2),
        foot: Vec3::new(-0.3, 0.0, 0.24),
        phase: (0.0, 0.5),
        phase_run: (0.0, 0.1),
        l1: 0.27,
        l2: 0.26,
        pole: Vec3::new(1.0, 0.2, 0.0),
        tarsus: Vec3::new(-0.18, 0.05, 0.0),
        widths: [0.11, 0.055, 0.04],
        paints: [0, 0, 2],
        paw: 0.04,
    });
    b.finish(
        Species::Rato,
        h,
        Vec3::new(0.46, h + 0.07, 0.0),
        Vec3::new(-0.2, h + 0.06, 0.0),
        vec![
            paint([0.4, 0.36, 0.33], 0.95, 0.0),  // 0 pelo cinza-acastanhado
            paint([0.66, 0.61, 0.56], 0.95, 0.0), // 1 barriga
            paint([0.86, 0.58, 0.56], 0.6, 0.0),  // 2 pele rosada
            paint([0.01, 0.01, 0.01], 0.05, 0.5), // 3 olhos
            paint([0.9, 0.9, 0.86], 0.5, 0.0),    // 4 bigodes
            paint([0.78, 0.45, 0.45], 0.6, 0.0),  // 5 dentro da orelha
        ],
        Motion {
            lift: 0.13,
            bob: 0.02,
            sway: 0.0,
            breath: 0.04,
            look: 0.35,
            sniff: 0.06,
            flex: 0.14,
            hop: 0.0,
            curl_on_death: false,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::animation::{gait_frequency, step_offset};

    fn plans() -> Vec<BodyPlan> {
        Species::ALL.iter().map(|s| s.plan()).collect()
    }

    #[test]
    fn index_matches_order_and_names_resolve() {
        for (i, s) in Species::ALL.iter().enumerate() {
            assert_eq!(s.index(), i);
            assert_eq!(Species::from_name(s.label()), Some(*s));
            for a in s.aliases() {
                assert_eq!(Species::from_name(a), Some(*s), "alias {a}");
            }
        }
        assert_eq!(Species::from_name("RAT"), Some(Species::Rato));
        assert_eq!(Species::from_name("dragao"), None);
    }

    #[test]
    fn plan_profile_matches_species_profile() {
        for p in plans() {
            assert_eq!(p.profile, p.species.profile());
            assert!(!p.parts.is_empty() && !p.legs.is_empty());
        }
    }

    #[test]
    fn paints_and_parts_reference_valid_slots() {
        for p in plans() {
            let n = p.palette.len() as u8;
            for part in &p.parts {
                assert!(part.paint < n, "{:?}: tinta {}", p.species, part.paint);
                assert!(part.scale.min_element() > 0.0 && part.rot.is_normalized());
            }
            for l in &p.legs {
                assert!(l.paints.iter().all(|&c| c < n), "{:?}", p.species);
            }
            for c in &p.chains {
                assert!(c.paint < n);
                assert_eq!(c.dirs.len(), c.lens.len());
                assert!(c.dirs.iter().all(|d| d.is_normalized()));
            }
        }
    }

    /// O tornozelo (pe + tarso) tem de ficar ao alcance da perna em TODO o ciclo, no passo
    /// maximo (correndo); senao o IK encurta a perna e o pe escorrega.
    #[test]
    fn every_leg_reaches_its_foot_through_the_whole_stride() {
        for p in plans() {
            let stride = p.profile.max_stride();
            for (i, leg) in p.legs.iter().enumerate() {
                let min = (leg.l1 - leg.l2).abs() + 0.02;
                let max = (leg.l1 + leg.l2) * 0.985;
                for k in 0..40 {
                    let o = step_offset(k as f32 / 40.0, stride, p.motion.lift, p.profile.stance);
                    let toe = leg.foot + Vec3::new(o.x, o.y, 0.0);
                    let d = leg.hip.distance(toe + leg.tarsus);
                    assert!(
                        (min..=max).contains(&d),
                        "{:?} perna {i}: distancia {d:.3} fora de [{min:.3}, {max:.3}] (k={k})",
                        p.species
                    );
                }
            }
        }
    }

    #[test]
    fn neighbouring_legs_alternate() {
        for p in plans() {
            if p.species == Species::Rato {
                continue; // marcha de quadrupede, testada abaixo
            }
            // pernas vem em pares (lado +, lado -), da frente para tras
            for pair in p.legs.chunks(2) {
                assert_ne!(pair[0].phase_offset, pair[1].phase_offset, "{:?}", p.species);
            }
            let left: Vec<_> = p.legs.iter().filter(|l| l.side > 0.0).collect();
            for w in left.windows(2) {
                assert_ne!(w[0].phase_offset, w[1].phase_offset, "{:?}", p.species);
            }
        }
    }

    #[test]
    fn insects_walk_in_tripods() {
        for s in [Species::Formiga, Species::Barata, Species::Gafanhoto] {
            let p = s.plan();
            assert_eq!(p.legs.len(), 6);
            let a: Vec<_> = p.legs.iter().filter(|l| l.phase_offset == 0.0).collect();
            assert_eq!(a.len(), 3, "{s:?}: 3 pernas em cada tripe");
            // tripe = frente e tras de um lado + meio do outro
            let sides: f32 = a.iter().map(|l| l.side).sum();
            assert_eq!(sides.abs(), 1.0);
        }
    }

    #[test]
    fn rat_walks_in_lateral_sequence_and_bounds_when_running() {
        let p = Species::Rato.plan();
        assert_eq!(p.legs.len(), 4);
        let mut walk: Vec<f32> = p.legs.iter().map(|l| l.phase_offset).collect();
        walk.sort_by(f32::total_cmp);
        assert_eq!(walk, vec![0.0, 0.25, 0.5, 0.75], "4 tempos distintos");
        // correndo: patas do mesmo par quase juntas
        for pair in p.legs.chunks(2) {
            assert!((pair[0].phase_run - pair[1].phase_run).abs() <= 0.1);
        }
        // cotovelo para tras, joelho para a frente
        assert!(p.legs[0].pole.x < 0.0 && p.legs[2].pole.x > 0.0);
    }

    #[test]
    fn no_slip_for_every_species() {
        for s in Species::ALL {
            let pr = s.profile();
            for v in [0.5, 2.0, 6.0, 12.0] {
                let stride = pr.stride_at(v);
                let f = gait_frequency(v, stride, pr.stance);
                let stance_time = pr.stance / f;
                assert!((v * stance_time - stride).abs() < 1e-4, "{s:?} v={v}");
            }
            assert!(pr.stride_at(100.0) <= pr.max_stride() + 1e-6);
            assert!(pr.stride_at(0.0) >= pr.stride - 1e-6);
        }
    }

    #[test]
    fn hit_circles_cover_the_body_but_not_far_beyond() {
        for p in plans() {
            let (lo, hi) = p
                .parts
                .iter()
                .map(|q| (q.pos.x - q.scale.x * 0.5, q.pos.x + q.scale.x * 0.5))
                .fold((f32::MAX, f32::MIN), |(a, b), (c, d)| (a.min(c), b.max(d)));
            let spots: Vec<Vec3> = p.profile.hit_spots().collect();
            assert!(!spots.is_empty());
            for h in &spots {
                assert!(h.x >= lo && h.x <= hi, "{:?}: centro {h} fora do corpo", p.species);
                assert!(h.z <= (hi - lo), "{:?}: raio exagerado", p.species);
            }
            // a cobertura vai de ponta a ponta do corpo (com folga de 25%)
            let cov_lo = spots.iter().map(|h| h.x - h.z).fold(f32::MAX, f32::min);
            let cov_hi = spots.iter().map(|h| h.x + h.z).fold(f32::MIN, f32::max);
            let slack = 0.25 * (hi - lo);
            assert!(cov_lo <= lo + slack && cov_hi >= hi - slack, "{:?}", p.species);
            let (c, r) = p.profile.bounds();
            for h in &spots {
                assert!((h.x - c.x).abs() + h.z <= r + 1e-4);
            }
        }
    }

    #[test]
    fn mirror_helpers_reflect_across_z() {
        let q = rot(0.2, 0.4, 0.3);
        let v = Vec3::new(0.3, 0.5, 0.7);
        let a = mirror(q * v);
        let b = mirror_q(q) * mirror(v);
        assert!((a - b).length() < 1e-5);
    }
}
