//! Geometria 2D pura, em coordenadas de tela (pixels fisicos, Y cresce para baixo).

use bevy::math::Vec2;

/// Tolerancia para comparacoes de coordenadas em px.
pub const EPS: f32 = 0.5;

/// Retangulo alinhado aos eixos. `min` = canto superior esquerdo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub min: Vec2,
    pub max: Vec2,
}

impl Rect {
    pub fn new(min: Vec2, max: Vec2) -> Self {
        Self {
            min: min.min(max),
            max: min.max(max),
        }
    }

    pub fn from_xywh(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self::new(Vec2::new(x, y), Vec2::new(x + w, y + h))
    }

    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    pub fn center(&self) -> Vec2 {
        (self.min + self.max) * 0.5
    }

    pub fn area(&self) -> f32 {
        self.width() * self.height()
    }

    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.min.x && p.x <= self.max.x && p.y >= self.min.y && p.y <= self.max.y
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.min.x < other.max.x
            && self.max.x > other.min.x
            && self.min.y < other.max.y
            && self.max.y > other.min.y
    }

    pub fn intersection(&self, other: &Rect) -> Option<Rect> {
        let min = self.min.max(other.min);
        let max = self.max.min(other.max);
        (min.x < max.x && min.y < max.y).then_some(Rect { min, max })
    }

    pub fn inflate(&self, d: f32) -> Rect {
        Rect {
            min: self.min - Vec2::splat(d),
            max: self.max + Vec2::splat(d),
        }
    }

    /// Compara com tolerancia (usado para detectar janela "mudou").
    pub fn approx_eq(&self, other: &Rect, tol: f32) -> bool {
        (self.min - other.min).abs().max_element() <= tol
            && (self.max - other.max).abs().max_element() <= tol
    }

    pub fn edge(&self, side: Side) -> Segment {
        match side {
            Side::Top => Segment::new(self.min, Vec2::new(self.max.x, self.min.y)),
            Side::Bottom => Segment::new(Vec2::new(self.min.x, self.max.y), self.max),
            Side::Left => Segment::new(self.min, Vec2::new(self.min.x, self.max.y)),
            Side::Right => Segment::new(Vec2::new(self.max.x, self.min.y), self.max),
        }
    }
}

/// Lado de um retangulo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::Top, Side::Bottom, Side::Left, Side::Right];

    pub fn is_horizontal(self) -> bool {
        matches!(self, Side::Top | Side::Bottom)
    }

    /// Normal que aponta para fora do retangulo (tela com Y para baixo).
    pub fn outward_normal(self) -> Vec2 {
        match self {
            Side::Top => Vec2::new(0.0, -1.0),
            Side::Bottom => Vec2::new(0.0, 1.0),
            Side::Left => Vec2::new(-1.0, 0.0),
            Side::Right => Vec2::new(1.0, 0.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub a: Vec2,
    pub b: Vec2,
}

impl Segment {
    pub fn new(a: Vec2, b: Vec2) -> Self {
        Self { a, b }
    }

    pub fn length(&self) -> f32 {
        self.a.distance(self.b)
    }

    pub fn point_at(&self, t: f32) -> Vec2 {
        self.a.lerp(self.b, t)
    }

    /// Parametro `t` em [0,1] do ponto do segmento mais proximo de `p`.
    pub fn closest_t(&self, p: Vec2) -> f32 {
        let ab = self.b - self.a;
        let len2 = ab.length_squared();
        if len2 <= f32::EPSILON {
            return 0.0;
        }
        ((p - self.a).dot(ab) / len2).clamp(0.0, 1.0)
    }

    pub fn closest_point(&self, p: Vec2) -> Vec2 {
        self.point_at(self.closest_t(p))
    }

    pub fn distance_to_point(&self, p: Vec2) -> f32 {
        self.closest_point(p).distance(p)
    }
}

/// Subtrai `cuts` do intervalo `span`, devolvendo os trechos restantes (ordenados).
/// Intervalos invertidos sao normalizados; trechos de comprimento <= 0 sao descartados.
pub fn subtract_intervals(span: (f32, f32), cuts: &[(f32, f32)]) -> Vec<(f32, f32)> {
    let (lo, hi) = (span.0.min(span.1), span.0.max(span.1));
    let mut sorted: Vec<(f32, f32)> = cuts
        .iter()
        .map(|&(a, b)| (a.min(b), a.max(b)))
        .filter(|&(a, b)| b > lo && a < hi)
        .collect();
    sorted.sort_by(|x, y| x.0.total_cmp(&y.0));

    let mut out = Vec::new();
    let mut cursor = lo;
    for (a, b) in sorted {
        if a > cursor {
            out.push((cursor, a.min(hi)));
        }
        cursor = cursor.max(b);
        if cursor >= hi {
            break;
        }
    }
    if cursor < hi {
        out.push((cursor, hi));
    }
    out.retain(|&(a, b)| b > a);
    out
}

/// Trechos de um lado de `rect` que estao realmente visiveis:
/// recortados pelos `bounds` (a tela) e sem as partes cobertas por `occluders`
/// (janelas que estao na frente na ordem Z).
pub fn visible_edge_segments(
    rect: &Rect,
    side: Side,
    occluders: &[Rect],
    bounds: &Rect,
    min_len: f32,
) -> Vec<Segment> {
    let edge = rect.edge(side);
    let horizontal = side.is_horizontal();

    // coordenada fixa da borda e intervalo ao longo dela
    let (fixed, span, b_fixed, b_span) = if horizontal {
        (
            edge.a.y,
            (edge.a.x, edge.b.x),
            (bounds.min.y, bounds.max.y),
            (bounds.min.x, bounds.max.x),
        )
    } else {
        (
            edge.a.x,
            (edge.a.y, edge.b.y),
            (bounds.min.x, bounds.max.x),
            (bounds.min.y, bounds.max.y),
        )
    };

    if fixed < b_fixed.0 - EPS || fixed > b_fixed.1 + EPS {
        return Vec::new();
    }
    let span = (span.0.max(b_span.0), span.1.min(b_span.1));
    if span.1 - span.0 < min_len {
        return Vec::new();
    }

    // trechos cobertos: o ocluso cobre se a coordenada fixa cai estritamente dentro dele
    let cuts: Vec<(f32, f32)> = occluders
        .iter()
        .filter_map(|o| {
            if horizontal {
                (o.min.y + EPS < fixed && fixed < o.max.y - EPS).then_some((o.min.x, o.max.x))
            } else {
                (o.min.x + EPS < fixed && fixed < o.max.x - EPS).then_some((o.min.y, o.max.y))
            }
        })
        .collect();

    subtract_intervals(span, &cuts)
        .into_iter()
        .filter(|&(a, b)| b - a >= min_len)
        .map(|(a, b)| {
            if horizontal {
                Segment::new(Vec2::new(a, fixed), Vec2::new(b, fixed))
            } else {
                Segment::new(Vec2::new(fixed, a), Vec2::new(fixed, b))
            }
        })
        .collect()
}

/// Angulo (rad) de um vetor de tela, 0 = +X, anti-horario com Y para cima.
/// Util para orientar o modelo; recebe vetor ja em espaco "Y para cima".
pub fn angle_of(v: Vec2) -> f32 {
    v.y.atan2(v.x)
}

/// Menor diferenca angular assinada entre dois angulos, em (-pi, pi].
pub fn angle_diff(from: f32, to: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let mut d = (to - from) % TAU;
    if d > PI {
        d -= TAU;
    } else if d <= -PI {
        d += TAU;
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_basics() {
        let r = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        assert_eq!(r.width(), 100.0);
        assert_eq!(r.height(), 50.0);
        assert_eq!(r.center(), Vec2::new(60.0, 45.0));
        assert!(r.contains(Vec2::new(10.0, 20.0)));
        assert!(!r.contains(Vec2::new(9.9, 20.0)));
        // construcao com cantos invertidos e normalizada
        let flipped = Rect::new(Vec2::new(110.0, 70.0), Vec2::new(10.0, 20.0));
        assert_eq!(flipped, r);
    }

    #[test]
    fn rect_intersection() {
        let a = Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        let b = Rect::from_xywh(50.0, 50.0, 100.0, 100.0);
        let c = Rect::from_xywh(100.0, 0.0, 10.0, 10.0); // so encosta
        assert!(a.intersects(&b));
        assert!(!a.intersects(&c));
        assert_eq!(
            a.intersection(&b).unwrap(),
            Rect::from_xywh(50.0, 50.0, 50.0, 50.0)
        );
        assert!(a.intersection(&c).is_none());
    }

    #[test]
    fn segment_closest_point() {
        let s = Segment::new(Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0));
        assert_eq!(s.closest_t(Vec2::new(5.0, 7.0)), 0.5);
        assert_eq!(s.closest_t(Vec2::new(-5.0, 1.0)), 0.0);
        assert_eq!(s.closest_t(Vec2::new(50.0, 1.0)), 1.0);
        assert_eq!(s.distance_to_point(Vec2::new(5.0, 3.0)), 3.0);
        // segmento degenerado nao gera NaN
        let d = Segment::new(Vec2::ONE, Vec2::ONE);
        assert_eq!(d.closest_t(Vec2::new(9.0, 9.0)), 0.0);
    }

    #[test]
    fn subtract_no_cuts_and_full_cover() {
        assert_eq!(subtract_intervals((0.0, 100.0), &[]), vec![(0.0, 100.0)]);
        assert!(subtract_intervals((0.0, 100.0), &[(-10.0, 200.0)]).is_empty());
    }

    #[test]
    fn subtract_middle_and_overlapping_cuts() {
        let out = subtract_intervals((0.0, 100.0), &[(20.0, 40.0), (30.0, 60.0), (90.0, 120.0)]);
        assert_eq!(out, vec![(0.0, 20.0), (60.0, 90.0)]);
    }

    #[test]
    fn subtract_handles_unsorted_inverted_and_outside_cuts() {
        let out = subtract_intervals((0.0, 100.0), &[(60.0, 50.0), (-50.0, -10.0), (10.0, 20.0)]);
        assert_eq!(out, vec![(0.0, 10.0), (20.0, 50.0), (60.0, 100.0)]);
    }

    #[test]
    fn visible_edges_unobstructed_window_has_four_edges() {
        let screen = Rect::from_xywh(0.0, 0.0, 1920.0, 1080.0);
        let w = Rect::from_xywh(200.0, 200.0, 600.0, 400.0);
        for side in Side::ALL {
            let segs = visible_edge_segments(&w, side, &[], &screen, 10.0);
            assert_eq!(segs.len(), 1, "{side:?}");
        }
    }

    #[test]
    fn covered_top_edge_is_split_by_window_in_front() {
        let screen = Rect::from_xywh(0.0, 0.0, 1920.0, 1080.0);
        let back = Rect::from_xywh(100.0, 100.0, 800.0, 500.0);
        // janela na frente cobre x 300..500 sobre a borda superior (y=100)
        let front = Rect::from_xywh(300.0, 50.0, 200.0, 300.0);
        let segs = visible_edge_segments(&back, Side::Top, &[front], &screen, 10.0);
        assert_eq!(segs.len(), 2);
        assert_eq!((segs[0].a.x, segs[0].b.x), (100.0, 300.0));
        assert_eq!((segs[1].a.x, segs[1].b.x), (500.0, 900.0));
        assert!(segs.iter().all(|s| s.a.y == 100.0 && s.b.y == 100.0));
    }

    #[test]
    fn tiled_windows_do_not_hide_each_others_shared_top() {
        let screen = Rect::from_xywh(0.0, 0.0, 1920.0, 1080.0);
        let left = Rect::from_xywh(0.0, 0.0, 960.0, 1040.0);
        let right = Rect::from_xywh(960.0, 0.0, 960.0, 1040.0);
        let segs = visible_edge_segments(&left, Side::Top, &[right], &screen, 10.0);
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].length(), 960.0);
    }

    #[test]
    fn side_edge_covered_vertically() {
        let screen = Rect::from_xywh(0.0, 0.0, 1920.0, 1080.0);
        let back = Rect::from_xywh(100.0, 100.0, 400.0, 600.0);
        // cobre a lateral direita (x=500) entre y=200..400
        let front = Rect::from_xywh(450.0, 200.0, 300.0, 200.0);
        let segs = visible_edge_segments(&back, Side::Right, &[front], &screen, 10.0);
        assert_eq!(segs.len(), 2);
        assert_eq!((segs[0].a.y, segs[0].b.y), (100.0, 200.0));
        assert_eq!((segs[1].a.y, segs[1].b.y), (400.0, 700.0));
    }

    #[test]
    fn edges_outside_screen_are_clipped_or_dropped() {
        let screen = Rect::from_xywh(0.0, 0.0, 1000.0, 800.0);
        let w = Rect::from_xywh(-300.0, 100.0, 600.0, 400.0);
        // lateral esquerda esta fora da tela -> descartada
        assert!(visible_edge_segments(&w, Side::Left, &[], &screen, 10.0).is_empty());
        // borda superior e recortada em x>=0
        let top = visible_edge_segments(&w, Side::Top, &[], &screen, 10.0);
        assert_eq!((top[0].a.x, top[0].b.x), (0.0, 300.0));
    }

    #[test]
    fn short_segments_are_discarded() {
        let screen = Rect::from_xywh(0.0, 0.0, 1920.0, 1080.0);
        let back = Rect::from_xywh(100.0, 100.0, 300.0, 300.0);
        let front = Rect::from_xywh(110.0, 0.0, 300.0, 200.0); // deixa so 10 px livres
        let segs = visible_edge_segments(&back, Side::Top, &[front], &screen, 24.0);
        assert!(segs.is_empty());
    }

    #[test]
    fn angle_diff_wraps_shortest_way() {
        use std::f32::consts::PI;
        assert!((angle_diff(0.1, 0.3) - 0.2).abs() < 1e-6);
        assert!((angle_diff(PI - 0.1, -PI + 0.1) - 0.2).abs() < 1e-5);
        assert!((angle_diff(-PI + 0.1, PI - 0.1) + 0.2).abs() < 1e-5);
    }
}
