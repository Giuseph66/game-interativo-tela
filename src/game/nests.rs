//! Ninhos e sacudida de janela (logica pura, sem ECS).
//!
//! * Uma janela **parada** por `nests.calm_secs` aceita ninho: um mob em repouso nela constroi um.
//! * O ninho cresce (`build_secs`), depois choca um filhote a cada `hatch_interval_secs`; ninho
//!   velho (`elite_after_secs`) choca **elite**.
//! * O jogador destroi o ninho com a espada (XP gordo) ou **sacode a janela** (arrasta de um lado
//!   para o outro): os mobs caem e o ninho racha.
//!
//! Tudo aqui recebe o tempo de fora (`now`/`dt`), entao roda igual em testes e no app.

use std::collections::HashMap;

use bevy::math::Vec2;
use bevy::prelude::Resource;

use super::combat::{Progress, Strike};
use crate::config::{CombatConfig, NestsConfig};
use crate::navigation::window_graph::{PathGraph, SurfaceOwner, remap_point};
use crate::platform::DesktopWindow;
use crate::rng::Rng;

/// Duracao do "tranco" visual de um golpe/rachadura.
pub const NEST_FLASH_SECS: f32 = 0.15;
/// Deslocamentos menores que isso (px) sao ruido, nao movimento.
const MOVE_NOISE_PX: f32 = 1.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Nest {
    pub id: u32,
    /// Janela dona da borda em que o ninho esta preso.
    pub window: u64,
    /// Ponto sobre a borda, em px locais do overlay.
    pub pos: Vec2,
    /// Normal da superficie (para fora da janela).
    pub normal: Vec2,
    /// Tipo de mob que construiu (os filhotes sao do mesmo tipo).
    pub kind: usize,
    pub hp: f32,
    pub max_hp: f32,
    /// Tempo de vida (s); so avanca com o ninho exposto.
    pub age: f32,
    /// Tempo ate o proximo filhote (so conta depois de construido).
    pub hatch_in: f32,
    /// A borda do ninho esta visivel (nao coberta por outra janela)?
    pub exposed: bool,
    /// Tempo restante do tranco visual.
    pub flash: f32,
    /// Tempo restante de exibicao da barra de vida.
    pub bar_left: f32,
}

impl Nest {
    /// Terminou de ser construido (ja pode chocar)?
    pub fn built(&self, c: &NestsConfig) -> bool {
        self.age >= c.build_secs
    }

    /// Ninho velho: passa a chocar elites.
    pub fn is_elite(&self, c: &NestsConfig) -> bool {
        self.age >= c.elite_after_secs
    }

    /// Fracao do tamanho final (cresce durante a construcao).
    pub fn growth(&self, c: &NestsConfig) -> f32 {
        (self.age / c.build_secs.max(0.01)).clamp(0.25, 1.0)
    }

    pub fn radius(&self, c: &NestsConfig) -> f32 {
        c.nest_radius_px * self.growth(c)
    }

    /// Centro do casulo (apoiado na borda, do lado de fora da janela).
    pub fn center(&self, c: &NestsConfig) -> Vec2 {
        self.pos + self.normal * self.radius(c) * 0.8
    }
}

/// Um filhote a nascer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hatch {
    pub nest: u32,
    pub at: Vec2,
    pub kind: usize,
    pub elite: bool,
}

/// Resultado de um golpe de espada num ninho.
#[derive(Debug, Clone, PartialEq)]
pub struct NestStrike {
    pub strike: Strike,
    pub at: Vec2,
    pub radius: f32,
    /// O ninho foi destruido por este golpe.
    pub destroyed: bool,
}

#[derive(Debug, Clone, Copy, Default)]
struct AxisTrack {
    sign: i8,
    /// Distancia percorrida no sentido atual.
    travel: f32,
}

/// Reconhece "sacudir": varias inversoes de sentido, cada uma apos um trecho minimo, dentro de
/// uma janela curta de tempo. Um arrasto comum (ida so) nunca dispara.
#[derive(Debug, Clone, Default)]
pub struct ShakeDetector {
    axes: [AxisTrack; 2],
    /// Instantes das inversoes validas recentes.
    reversals: Vec<f32>,
    last_feed: Option<f32>,
}

impl ShakeDetector {
    /// Alimenta um deslocamento da janela ocorrido ate `now`. `true` = sacudida reconhecida.
    pub fn feed(&mut self, delta: Vec2, now: f32, c: &NestsConfig) -> bool {
        // pausa longa: o que veio antes nao faz parte do mesmo gesto
        if self
            .last_feed
            .is_some_and(|t| now - t > c.shake_window_secs)
        {
            *self = Self::default();
        }
        self.last_feed = Some(now);

        for (axis, d) in self.axes.iter_mut().zip([delta.x, delta.y]) {
            if d.abs() < MOVE_NOISE_PX {
                continue;
            }
            let sign = if d > 0.0 { 1 } else { -1 };
            if axis.sign == sign {
                axis.travel += d.abs();
                continue;
            }
            if axis.sign != 0 && axis.travel >= c.shake_min_px {
                self.reversals.push(now);
            }
            axis.sign = sign;
            axis.travel = d.abs();
        }
        self.reversals.retain(|t| now - *t <= c.shake_window_secs);
        if (self.reversals.len() as u32) < c.shake_reversals.max(1) {
            return false;
        }
        *self = Self::default();
        true
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NestStats {
    pub built: u32,
    pub destroyed: u32,
    pub hatched: u32,
    pub shakes: u32,
}

/// Todos os ninhos + o que se sabe do movimento de cada janela.
#[derive(Debug, Resource)]
pub struct NestField {
    pub nests: Vec<Nest>,
    /// Instante do ultimo movimento/redimensionamento (ou da primeira vez que a janela foi vista).
    last_moved: HashMap<u64, f32>,
    detectors: HashMap<u64, ShakeDetector>,
    /// Instante da ultima sacudida aceita de cada janela (para o intervalo minimo).
    last_shake: HashMap<u64, f32>,
    /// Janelas sacudidas ainda nao tratadas pelo jogo.
    shakes: Vec<u64>,
    next_id: u32,
    rng: Rng,
    pub stats: NestStats,
}

impl Default for NestField {
    fn default() -> Self {
        Self::new(0)
    }
}

impl NestField {
    /// `seed == 0` = semente do relogio (como o resto do jogo).
    pub fn new(seed: u64) -> Self {
        Self {
            nests: Vec::new(),
            last_moved: HashMap::new(),
            detectors: HashMap::new(),
            last_shake: HashMap::new(),
            shakes: Vec::new(),
            next_id: 1,
            rng: Rng::new(seed),
            stats: NestStats::default(),
        }
    }

    pub fn get(&self, id: u32) -> Option<&Nest> {
        self.nests.iter().find(|n| n.id == id)
    }

    pub fn clear(&mut self) {
        self.nests.clear();
    }

    pub fn rng(&mut self) -> &mut Rng {
        &mut self.rng
    }

    /// Compara a lista de janelas antiga com a nova (chamado a cada reconstrucao do grafo):
    /// zera a calma das que se moveram, leva os ninhos junto, detecta sacudidas e descarta o que
    /// pertencia a janelas que sumiram. Devolve `true` se alguma janela se moveu.
    pub fn observe_windows(
        &mut self,
        old: &[DesktopWindow],
        new: &[DesktopWindow],
        now: f32,
        c: &NestsConfig,
    ) -> bool {
        let mut moved_any = false;
        for n in new {
            let Some(o) = old.iter().find(|o| o.id == n.id) else {
                self.last_moved.entry(n.id).or_insert(now);
                continue;
            };
            if o.rect.approx_eq(&n.rect, MOVE_NOISE_PX) {
                continue; // so mudou a ordem Z (ou nada)
            }
            moved_any = true;
            self.last_moved.insert(n.id, now);
            for nest in self.nests.iter_mut().filter(|x| x.window == n.id) {
                nest.pos = remap_point(nest.pos, &o.rect, &n.rect);
            }
            // redimensionar nao e sacudir
            let same_size = (o.rect.width() - n.rect.width()).abs() <= 2.0
                && (o.rect.height() - n.rect.height()).abs() <= 2.0;
            let shaken = same_size
                && self
                    .detectors
                    .entry(n.id)
                    .or_default()
                    .feed(n.rect.min - o.rect.min, now, c);
            // sacudir sem parar conta uma vez a cada `shake_cooldown_secs`
            let rested = self
                .last_shake
                .get(&n.id)
                .is_none_or(|t| now - t >= c.shake_cooldown_secs);
            if shaken && rested {
                self.last_shake.insert(n.id, now);
                self.shakes.push(n.id);
                self.stats.shakes += 1;
            }
        }
        let alive = |id: &u64| new.iter().any(|w| w.id == *id);
        self.nests.retain(|x| alive(&x.window));
        self.last_moved.retain(|id, _| alive(id));
        self.detectors.retain(|id, _| alive(id));
        self.last_shake.retain(|id, _| alive(id));
        moved_any
    }

    /// Ha quanto tempo (s) a janela esta parada. `None` = janela desconhecida.
    pub fn calm_for(&self, window: u64, now: f32) -> Option<f32> {
        self.last_moved.get(&window).map(|t| (now - t).max(0.0))
    }

    /// Janelas sacudidas desde a ultima chamada.
    pub fn take_shakes(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.shakes)
    }

    /// Um ninho so "existe" para o jogador enquanto a borda dele esta visivel.
    pub fn refresh_exposure(&mut self, graph: &PathGraph) {
        for n in &mut self.nests {
            n.exposed = graph
                .nearest_point(n.pos, 3.0)
                .is_some_and(|p| graph.edge(p.edge).owner == SurfaceOwner::Window(n.window));
        }
    }

    /// Da para construir um ninho em `pos`, na janela `window`, agora?
    pub fn can_build(&self, window: u64, pos: Vec2, now: f32, c: &NestsConfig) -> bool {
        c.enabled
            && self.calm_for(window, now).is_some_and(|t| t >= c.calm_secs)
            && (self.nests.len() as u32) < c.max_total
            && (self.nests.iter().filter(|n| n.window == window).count() as u32) < c.max_per_window
            && self
                .nests
                .iter()
                .all(|n| n.pos.distance(pos) >= c.nest_radius_px * 3.0)
    }

    /// Cria o ninho sem conferir as regras (veja [`can_build`](Self::can_build)).
    pub fn build(
        &mut self,
        window: u64,
        pos: Vec2,
        normal: Vec2,
        kind: usize,
        c: &NestsConfig,
    ) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.nests.push(Nest {
            id,
            window,
            pos,
            normal: normal.normalize_or(Vec2::NEG_Y),
            kind,
            hp: c.nest_hp,
            max_hp: c.nest_hp,
            age: 0.0,
            hatch_in: c.hatch_interval_secs,
            exposed: true,
            flash: 0.0,
            bar_left: 0.0,
        });
        self.stats.built += 1;
        id
    }

    /// Um mob em repouso tenta construir: respeita as regras e a chance por segundo.
    #[allow(clippy::too_many_arguments)]
    pub fn try_build(
        &mut self,
        window: u64,
        pos: Vec2,
        normal: Vec2,
        kind: usize,
        now: f32,
        dt: f32,
        c: &NestsConfig,
    ) -> Option<u32> {
        if !self.can_build(window, pos, now, c) {
            return None;
        }
        let p = (c.build_chance_per_sec * dt).clamp(0.0, 1.0);
        self.rng
            .chance(p)
            .then(|| self.build(window, pos, normal, kind, c))
    }

    /// Avanca o tempo. Devolve os filhotes que nascem agora; `free_slots` = quantos mobs ainda
    /// cabem (`mobs.max`). Sem vaga, o ninho segura o filhote ate abrir uma.
    pub fn tick(&mut self, dt: f32, c: &NestsConfig, mut free_slots: u32) -> Vec<Hatch> {
        let dt = dt.clamp(0.0, 1.0);
        let mut out = Vec::new();
        for n in &mut self.nests {
            n.flash = (n.flash - dt).max(0.0);
            n.bar_left = (n.bar_left - dt).max(0.0);
            if !n.exposed {
                continue; // coberto por outra janela: pausado
            }
            let was_built = n.built(c);
            n.age += dt;
            if !was_built {
                continue;
            }
            n.hatch_in = (n.hatch_in - dt).max(0.0);
            if n.hatch_in > 0.0 || free_slots == 0 {
                continue;
            }
            free_slots -= 1;
            n.hatch_in = c.hatch_interval_secs;
            out.push(Hatch {
                nest: n.id,
                at: n.pos,
                kind: n.kind,
                elite: n.is_elite(c),
            });
        }
        self.stats.hatched += out.len() as u32;
        out
    }

    /// Ninho exposto sob o ponto `p` (o mais proximo, se houver sobreposicao).
    pub fn nest_at(&self, p: Vec2, c: &NestsConfig) -> Option<u32> {
        self.nests
            .iter()
            .filter(|n| n.exposed && p.distance(n.center(c)) <= n.radius(c))
            .min_by(|a, b| p.distance(a.center(c)).total_cmp(&p.distance(b.center(c))))
            .map(|n| n.id)
    }

    /// Golpe de espada no ninho `id`. Destruir rende `xp_per_nest` alem do XP normal de abate.
    pub fn strike(
        &mut self,
        id: u32,
        progress: &mut Progress,
        combat: &CombatConfig,
        c: &NestsConfig,
    ) -> Option<NestStrike> {
        let i = self.nests.iter().position(|n| n.id == id)?;
        let sword = progress.sword;
        let nest = &mut self.nests[i];
        let (at, radius) = (nest.center(c), nest.radius(c));
        let mut strike = progress.strike(&mut nest.hp, combat);
        nest.flash = NEST_FLASH_SECS;
        nest.bar_left = combat.health_bar_secs;
        if strike.killed {
            let bonus = (c.xp_per_nest as f32 * sword.xp_multiplier(combat)).round() as u32;
            let upgraded = progress.add_xp(bonus, combat);
            strike.xp_gained += bonus;
            strike.upgraded = upgraded.or(strike.upgraded);
            self.nests.remove(i);
            self.stats.destroyed += 1;
        }
        Some(NestStrike {
            destroyed: strike.killed,
            strike,
            at,
            radius,
        })
    }

    /// A janela foi sacudida: os ninhos dela racham (perdem vida, mas nunca morrem so com isso:
    /// o golpe final e da espada). Devolve quantos racharam.
    pub fn apply_shake(&mut self, window: u64, c: &NestsConfig, bar_secs: f32) -> u32 {
        let mut cracked = 0;
        for n in self.nests.iter_mut().filter(|n| n.window == window) {
            let after = (n.hp - c.shake_nest_damage).max(1.0).min(n.hp);
            if after < n.hp {
                n.hp = after;
                n.flash = NEST_FLASH_SECS;
                n.bar_left = bar_secs;
                cracked += 1;
            }
        }
        cracked
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Sword;
    use crate::navigation::geometry::Rect;
    use crate::navigation::window_graph::BuildParams;
    use crate::platform::WindowState;

    fn cfg() -> NestsConfig {
        NestsConfig::default()
    }

    fn win(id: u64, x: f32, y: f32, w: f32, h: f32) -> DesktopWindow {
        let r = Rect::from_xywh(x, y, w, h);
        DesktopWindow {
            id,
            rect: r,
            raw_rect: r,
            state: WindowState::Normal,
            pid: 1,
            title: String::new(),
        }
    }

    /// Alimenta o detector com uma senoide amostrada a cada `step` s.
    fn shake_x(
        d: &mut ShakeDetector,
        c: &NestsConfig,
        amp: f32,
        hz: f32,
        secs: f32,
        step: f32,
    ) -> bool {
        let pos = |t: f32| (t * hz * std::f32::consts::TAU).sin() * amp;
        let mut t = 0.0;
        let mut fired = false;
        while t < secs {
            let next = t + step;
            fired |= d.feed(Vec2::new(pos(next) - pos(t), 0.0), next, c);
            t = next;
        }
        fired
    }

    #[test]
    fn plain_drag_is_not_a_shake() {
        let c = cfg();
        let mut d = ShakeDetector::default();
        // arrasto longo num sentido so, com uma unica volta
        for k in 0..40 {
            assert!(!d.feed(Vec2::new(25.0, 3.0), k as f32 * 0.05, &c));
        }
        for k in 40..80 {
            assert!(!d.feed(Vec2::new(-25.0, -3.0), k as f32 * 0.05, &c));
        }
    }

    #[test]
    fn back_and_forth_is_a_shake_even_at_coarse_sampling() {
        let c = cfg();
        // 3 Hz, +-80 px, amostrado a 50 ms (taxa rapida) e a 100 ms (laco calmo)
        assert!(shake_x(
            &mut ShakeDetector::default(),
            &c,
            80.0,
            3.0,
            1.5,
            0.05
        ));
        assert!(shake_x(
            &mut ShakeDetector::default(),
            &c,
            80.0,
            3.0,
            1.5,
            0.10
        ));
        // vertical tambem vale
        let mut d = ShakeDetector::default();
        let mut fired = false;
        for k in 0..12 {
            let dy = if k % 2 == 0 { 90.0 } else { -90.0 };
            fired |= d.feed(Vec2::new(0.0, dy), k as f32 * 0.15, &c);
        }
        assert!(fired);
    }

    #[test]
    fn tremor_and_slow_swaying_are_not_shakes() {
        let c = cfg();
        // tremor: muitas inversoes, trechos curtos demais
        assert!(!shake_x(
            &mut ShakeDetector::default(),
            &c,
            c.shake_min_px * 0.2,
            6.0,
            3.0,
            0.02
        ));
        // vai-e-vem lento: trechos longos, mas inversoes espacadas demais
        assert!(!shake_x(
            &mut ShakeDetector::default(),
            &c,
            200.0,
            0.25,
            12.0,
            0.05
        ));
    }

    #[test]
    fn detector_rearms_after_firing() {
        let c = cfg();
        let mut d = ShakeDetector::default();
        let mut fired_at = None;
        for k in 0..40 {
            let dx = if k % 2 == 0 { 90.0 } else { -90.0 };
            if d.feed(Vec2::new(dx, 0.0), k as f32 * 0.1, &c) {
                fired_at = Some(k);
                break;
            }
        }
        // a 1a inversao acontece na 2a amostra; dispara exatamente na `shake_reversals`-esima
        assert_eq!(fired_at, Some(c.shake_reversals as usize));
        assert!(d.reversals.is_empty(), "comeca do zero depois de disparar");
        assert!(!d.feed(Vec2::new(90.0, 0.0), 10.0, &c));
    }

    #[test]
    fn moving_resets_calm_and_carries_the_nest() {
        let c = cfg();
        let mut f = NestField::new(1);
        let a = vec![
            win(1, 100.0, 100.0, 600.0, 400.0),
            win(2, 900.0, 100.0, 400.0, 300.0),
        ];
        assert!(!f.observe_windows(&[], &a, 0.0, &c));
        assert_eq!(f.calm_for(1, 50.0), Some(50.0));
        assert_eq!(f.calm_for(99, 50.0), None);

        let id = f.build(1, Vec2::new(400.0, 100.0), Vec2::NEG_Y, 0, &c);
        let b = vec![
            win(1, 160.0, 130.0, 600.0, 400.0),
            win(2, 900.0, 100.0, 400.0, 300.0),
        ];
        assert!(f.observe_windows(&a, &b, 50.0, &c));
        assert_eq!(f.calm_for(1, 51.0), Some(1.0), "moveu: calma zerada");
        assert_eq!(f.calm_for(2, 51.0), Some(51.0), "a outra continua calma");
        assert_eq!(f.get(id).unwrap().pos, Vec2::new(460.0, 130.0));

        // so a ordem Z mudou: ninguem "se moveu"
        let swapped = vec![b[1].clone(), b[0].clone()];
        assert!(!f.observe_windows(&b, &swapped, 60.0, &c));
        assert_eq!(f.calm_for(1, 61.0), Some(11.0));

        // janela fechada/minimizada leva o ninho junto
        assert!(!f.observe_windows(&swapped, &swapped[..1], 70.0, &c));
        assert!(f.nests.is_empty());
        assert_eq!(f.calm_for(1, 71.0), None);
    }

    #[test]
    fn shaking_a_window_is_reported_once_and_resizing_is_not() {
        let c = cfg();
        let mut f = NestField::new(1);
        let at = |x: f32| vec![win(7, x, 200.0, 500.0, 300.0)];
        let mut prev = at(300.0);
        f.observe_windows(&[], &prev, 0.0, &c);
        for k in 1..=30 {
            let t = k as f32 * 0.05;
            let cur = at(300.0 + (t * 3.0 * std::f32::consts::TAU).sin() * 80.0);
            f.observe_windows(&prev, &cur, t, &c);
            prev = cur;
        }
        // 1,5 s de vai-e-vem continuo: o gesto se repete, mas conta uma vez so (intervalo minimo)
        assert_eq!(f.take_shakes(), vec![7]);
        assert!(f.take_shakes().is_empty(), "consumidas");
        assert_eq!(f.stats.shakes, 1);
        // passado o intervalo, sacudir de novo vale de novo
        let t0 = 1.5 + c.shake_cooldown_secs;
        for k in 1..=30 {
            let t = k as f32 * 0.05;
            let cur = at(300.0 + (t * 3.0 * std::f32::consts::TAU).sin() * 80.0);
            f.observe_windows(&prev, &cur, t0 + t, &c);
            prev = cur;
        }
        assert_eq!(f.take_shakes(), vec![7]);

        // "sanfona" de tamanho nao e sacudida
        let mut g = NestField::new(1);
        let sized = |w: f32| vec![win(7, 300.0, 200.0, w, 300.0)];
        let mut prev = sized(500.0);
        g.observe_windows(&[], &prev, 0.0, &c);
        for k in 1..=30 {
            let cur = sized(if k % 2 == 0 { 500.0 } else { 700.0 });
            assert!(g.observe_windows(&prev, &cur, k as f32 * 0.1, &c));
            prev = cur;
        }
        assert!(g.take_shakes().is_empty());
    }

    #[test]
    fn build_rules_calm_limits_and_spacing() {
        let c = cfg();
        let mut f = NestField::new(1);
        let ws: Vec<DesktopWindow> = (1..=8)
            .map(|i| win(i, i as f32 * 10.0, 100.0, 600.0, 400.0))
            .collect();
        f.observe_windows(&[], &ws, 0.0, &c);
        let p = Vec2::new(300.0, 100.0);

        assert!(!f.can_build(1, p, c.calm_secs - 1.0, &c), "ainda agitada");
        let now = c.calm_secs + 1.0;
        assert!(f.can_build(1, p, now, &c));
        assert!(!f.can_build(99, p, now, &c), "janela desconhecida");
        let mut off = c.clone();
        off.enabled = false;
        assert!(!f.can_build(1, p, now, &off));

        f.build(1, p, Vec2::NEG_Y, 0, &c);
        assert!(
            !f.can_build(1, Vec2::new(900.0, 100.0), now, &c),
            "max_per_window"
        );
        assert!(
            !f.can_build(2, p + Vec2::X * 5.0, now, &c),
            "colado em outro ninho"
        );
        // enche ate max_total em outras janelas
        let mut w = 2;
        while (f.nests.len() as u32) < c.max_total {
            let q = Vec2::new(300.0 + w as f32 * 200.0, 100.0);
            assert!(f.can_build(w, q, now, &c));
            f.build(w, q, Vec2::NEG_Y, 0, &c);
            w += 1;
        }
        assert!(
            !f.can_build(w, Vec2::new(5000.0, 100.0), now, &c),
            "max_total"
        );
    }

    #[test]
    fn try_build_follows_the_chance() {
        let mut c = cfg();
        let mut f = NestField::new(7);
        f.observe_windows(&[], &[win(1, 0.0, 0.0, 600.0, 400.0)], 0.0, &c);
        let now = c.calm_secs + 1.0;
        c.build_chance_per_sec = 0.0;
        for _ in 0..500 {
            assert!(
                f.try_build(1, Vec2::ZERO, Vec2::NEG_Y, 3, now, 0.1, &c)
                    .is_none()
            );
        }
        c.build_chance_per_sec = 1000.0;
        let id = f
            .try_build(1, Vec2::ZERO, Vec2::NEG_Y, 3, now, 0.1, &c)
            .unwrap();
        assert_eq!(f.get(id).unwrap().kind, 3);
        assert_eq!(f.stats.built, 1);
    }

    #[test]
    fn grows_then_hatches_on_schedule_and_turns_elite() {
        let c = cfg();
        let mut f = NestField::new(1);
        let id = f.build(1, Vec2::new(50.0, 60.0), Vec2::NEG_Y, 2, &c);
        assert!(!f.get(id).unwrap().built(&c));
        assert!(f.get(id).unwrap().growth(&c) < 0.5);

        let mut t = 0.0;
        let mut hatches: Vec<(f32, Hatch)> = Vec::new();
        while t < c.elite_after_secs + c.hatch_interval_secs * 2.0 {
            t += 0.1;
            for h in f.tick(0.1, &c, 100) {
                hatches.push((t, h));
            }
        }
        let n = f.get(id).unwrap();
        assert!(n.built(&c) && n.is_elite(&c));
        assert_eq!(n.growth(&c), 1.0);

        let (first_t, first) = hatches[0];
        let expect = c.build_secs + c.hatch_interval_secs;
        assert!((first_t - expect).abs() < 0.3, "{first_t} vs {expect}");
        assert_eq!(
            (first.kind, first.at, first.nest),
            (2, Vec2::new(50.0, 60.0), id)
        );
        assert!(!first.elite, "ninho novo choca filhote comum");
        assert!(hatches.last().unwrap().1.elite, "ninho velho choca elite");
        for pair in hatches.windows(2) {
            let gap = pair[1].0 - pair[0].0;
            assert!((gap - c.hatch_interval_secs).abs() < 0.3, "{gap}");
        }
        assert_eq!(f.stats.hatched as usize, hatches.len());
    }

    #[test]
    fn hatch_waits_for_a_free_slot_and_covered_nests_pause() {
        let c = cfg();
        let mut f = NestField::new(1);
        let id = f.build(1, Vec2::ZERO, Vec2::NEG_Y, 0, &c);
        let run = |f: &mut NestField, secs: f32, slots: u32| -> usize {
            let mut n = 0;
            let mut t = 0.0;
            while t < secs {
                n += f.tick(0.5, &c, slots).len();
                t += 0.5;
            }
            n
        };
        let due = c.build_secs + c.hatch_interval_secs + 5.0;
        assert_eq!(run(&mut f, due, 0), 0, "sem vaga: nao choca");
        assert_eq!(f.tick(0.5, &c, 1).len(), 1, "abriu vaga: choca na hora");

        // coberto por outra janela: nem envelhece
        f.nests[0].exposed = false;
        let age = f.get(id).unwrap().age;
        assert_eq!(run(&mut f, 200.0, 10), 0);
        assert_eq!(f.get(id).unwrap().age, age);
    }

    #[test]
    fn exposure_follows_the_graph() {
        let c = cfg();
        let screen = Rect::from_xywh(0.0, 0.0, 1920.0, 1080.0);
        let back = win(1, 300.0, 300.0, 700.0, 500.0);
        let mut f = NestField::new(1);
        let id = f.build(1, Vec2::new(600.0, 300.0), Vec2::NEG_Y, 0, &c);

        let alone = PathGraph::build(screen, std::slice::from_ref(&back), &BuildParams::default());
        f.refresh_exposure(&alone);
        assert!(f.get(id).unwrap().exposed);
        assert_eq!(f.nest_at(f.get(id).unwrap().center(&c), &c), Some(id));

        // outra janela na frente cobre o trecho do topo onde o ninho esta
        let front = win(2, 400.0, 200.0, 500.0, 400.0);
        let covered = PathGraph::build(screen, &[front, back], &BuildParams::default());
        f.refresh_exposure(&covered);
        assert!(!f.get(id).unwrap().exposed);
        assert_eq!(
            f.nest_at(f.get(id).unwrap().center(&c), &c),
            None,
            "coberto nao e clicavel"
        );
    }

    #[test]
    fn sword_destroys_the_nest_for_bonus_xp() {
        let c = cfg();
        let combat = CombatConfig::default();
        let mut f = NestField::new(1);
        let id = f.build(1, Vec2::new(10.0, 10.0), Vec2::NEG_Y, 0, &c);
        let mut p = Progress::default();
        let mut hits = 0;
        let last = loop {
            let s = f.strike(id, &mut p, &combat, &c).unwrap();
            hits += 1;
            if s.destroyed {
                break s;
            }
            assert_eq!(f.get(id).unwrap().flash, NEST_FLASH_SECS);
            assert!(f.get(id).unwrap().bar_left > 0.0);
            assert!(hits < 1000);
        };
        assert!(f.nests.is_empty());
        assert_eq!(f.stats.destroyed, 1);
        assert!(
            hits > 3,
            "ninho aguenta mais que um mob fraco: {hits} golpes"
        );
        assert!(last.strike.xp_gained >= c.xp_per_nest);
        // o XP total bate com o que foi anunciado golpe a golpe
        assert!(p.xp >= c.xp_per_nest + hits * combat.xp_per_hit);
        assert_eq!(p.sword, Sword::for_xp(p.xp, &combat));
        assert!(f.strike(id, &mut p, &combat, &c).is_none(), "ja destruido");
    }

    #[test]
    fn nest_bonus_can_unlock_a_sword() {
        let c = cfg();
        let combat = CombatConfig::default();
        let mut f = NestField::new(1);
        let id = f.build(1, Vec2::ZERO, Vec2::NEG_Y, 0, &c);
        f.nests[0].hp = 0.5;
        // falta menos que o bonus para o ferro: o golpe final tem de anunciar a evolucao
        let mut p = Progress {
            xp: combat.xp_iron - combat.xp_per_hit - combat.xp_per_kill - 1,
            sword: Sword::Wood,
        };
        let s = f.strike(id, &mut p, &combat, &c).unwrap();
        assert!(s.destroyed);
        assert_eq!(s.strike.upgraded, Some(Sword::Iron));
        assert_eq!(p.sword, Sword::Iron);
    }

    #[test]
    fn shake_cracks_but_never_finishes_a_nest() {
        let c = cfg();
        let mut f = NestField::new(1);
        let a = f.build(1, Vec2::ZERO, Vec2::NEG_Y, 0, &c);
        let b = f.build(2, Vec2::new(900.0, 0.0), Vec2::NEG_Y, 0, &c);
        assert_eq!(f.apply_shake(1, &c, 3.0), 1);
        assert_eq!(f.get(a).unwrap().hp, c.nest_hp - c.shake_nest_damage);
        assert_eq!(f.get(a).unwrap().bar_left, 3.0);
        assert_eq!(f.get(b).unwrap().hp, c.nest_hp, "outra janela intacta");
        for _ in 0..50 {
            f.apply_shake(1, &c, 3.0);
        }
        assert_eq!(f.get(a).unwrap().hp, 1.0, "o golpe final e da espada");
        assert_eq!(f.apply_shake(1, &c, 3.0), 0, "nada mais a rachar");
    }
}
