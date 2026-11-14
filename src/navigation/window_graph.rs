//! Transforma a geometria das janelas em um grafo de caminhos (nos + arestas).
//!
//! Superficies:
//! * bordas visiveis das janelas (topo, base, laterais), recortadas pelas janelas na frente;
//! * perimetro da tela (normal apontando para dentro);
//! * ligacoes curtas ("links") entre bordas de janelas diferentes que estao a menos de
//!   `link_distance` uma da outra (pontos de encontro).
//!
//! O grafo inteiro e reconstruido somente quando o conjunto de janelas muda
//! (ver [`crate::platform::windows_equivalent`]); isso e barato (centenas de nos).

use bevy::math::Vec2;

use super::geometry::{EPS, Rect, Segment, Side, visible_edge_segments};
use crate::platform::DesktopWindow;

pub type NodeId = u32;
pub type EdgeId = u32;

/// Distancia (px) em que dois nos sao considerados o mesmo ponto.
const MERGE_DIST: f32 = 0.75;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceOwner {
    Screen,
    Window(u64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// Lado de uma janela (ou da tela).
    Surface(Side),
    /// Passagem curta entre superficies proximas.
    Link,
}

#[derive(Debug, Clone, Copy)]
pub struct Edge {
    pub a: NodeId,
    pub b: NodeId,
    pub kind: EdgeKind,
    pub owner: SurfaceOwner,
    /// Normal unitaria da superficie (direcao "para cima" da aranha), Y de tela.
    pub normal: Vec2,
    pub length: f32,
    /// Custo para o A* (distancia ponderada).
    pub cost: f32,
}

impl Edge {
    /// Superficie vertical (a aranha esta escalando).
    pub fn is_vertical(&self) -> bool {
        self.normal.x.abs() > self.normal.y.abs()
    }
}

/// Ponto sobre uma aresta do grafo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GraphPoint {
    pub edge: EdgeId,
    /// 0 = no `a`, 1 = no `b`.
    pub t: f32,
    pub pos: Vec2,
}

#[derive(Debug, Clone, Copy)]
pub struct BuildParams {
    pub link_distance: f32,
    pub min_segment: f32,
    pub min_window_size: f32,
    pub screen_edges: bool,
}

impl Default for BuildParams {
    fn default() -> Self {
        Self {
            link_distance: 14.0,
            min_segment: 24.0,
            min_window_size: 120.0,
            screen_edges: true,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PathGraph {
    pub nodes: Vec<Vec2>,
    pub edges: Vec<Edge>,
    adjacency: Vec<Vec<EdgeId>>,
    pub screen: Option<Rect>,
}

struct Surface {
    seg: Segment,
    owner: SurfaceOwner,
    side: Side,
    normal: Vec2,
}

impl PathGraph {
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    pub fn edges_at(&self, node: NodeId) -> &[EdgeId] {
        &self.adjacency[node as usize]
    }

    pub fn edge(&self, id: EdgeId) -> &Edge {
        &self.edges[id as usize]
    }

    pub fn node(&self, id: NodeId) -> Vec2 {
        self.nodes[id as usize]
    }

    pub fn edge_segment(&self, id: EdgeId) -> Segment {
        let e = self.edge(id);
        Segment::new(self.node(e.a), self.node(e.b))
    }

    pub fn point_on(&self, edge: EdgeId, t: f32) -> GraphPoint {
        let t = t.clamp(0.0, 1.0);
        GraphPoint {
            edge,
            t,
            pos: self.edge_segment(edge).point_at(t),
        }
    }

    /// Ponto do grafo mais proximo de `p`, se estiver a no maximo `max_dist` px.
    pub fn nearest_point(&self, p: Vec2, max_dist: f32) -> Option<GraphPoint> {
        let mut best: Option<(f32, GraphPoint)> = None;
        for id in 0..self.edges.len() as EdgeId {
            let seg = self.edge_segment(id);
            let t = seg.closest_t(p);
            let q = seg.point_at(t);
            let d = q.distance(p);
            if d <= max_dist && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((
                    d,
                    GraphPoint {
                        edge: id,
                        t,
                        pos: q,
                    },
                ));
            }
        }
        best.map(|(_, g)| g)
    }

    /// `true` se `p` esta a ate `tol` px de alguma aresta do grafo.
    pub fn contains_point(&self, p: Vec2, tol: f32) -> bool {
        self.nearest_point(p, tol).is_some()
    }

    /// Primeira superficie "chao" (normal para cima) abaixo de `p`: usada na queda livre.
    pub fn surface_below(&self, p: Vec2) -> Option<GraphPoint> {
        let mut best: Option<(f32, GraphPoint)> = None;
        for id in 0..self.edges.len() as EdgeId {
            let e = self.edge(id);
            if e.normal.y >= -0.5 || e.is_vertical() {
                continue; // so superficies voltadas para cima
            }
            let seg = self.edge_segment(id);
            let (lo, hi) = (seg.a.x.min(seg.b.x), seg.a.x.max(seg.b.x));
            if p.x < lo || p.x > hi {
                continue;
            }
            let y = seg.a.y; // horizontal
            if y + EPS < p.y {
                continue; // acima de p
            }
            let dy = y - p.y;
            if best.as_ref().is_none_or(|(bd, _)| dy < *bd) {
                let t = seg.closest_t(Vec2::new(p.x, y));
                best = Some((
                    dy,
                    GraphPoint {
                        edge: id,
                        t,
                        pos: Vec2::new(p.x, y),
                    },
                ));
            }
        }
        best.map(|(_, g)| g)
    }

    /// Comprimento total de todas as arestas (para sorteio de destinos).
    pub fn total_length(&self) -> f32 {
        self.edges.iter().map(|e| e.length).sum()
    }

    /// Janelas que contribuem com pelo menos uma aresta.
    pub fn owners(&self) -> Vec<SurfaceOwner> {
        let mut v: Vec<SurfaceOwner> = Vec::new();
        for e in &self.edges {
            if !v.contains(&e.owner) {
                v.push(e.owner);
            }
        }
        v
    }

    /// Constroi o grafo. `windows` deve estar ordenada da frente para o fundo (ordem Z).
    pub fn build(screen: Rect, windows: &[DesktopWindow], params: &BuildParams) -> PathGraph {
        let mut surfaces: Vec<Surface> = Vec::new();

        if params.screen_edges {
            for side in Side::ALL {
                surfaces.push(Surface {
                    seg: screen.edge(side),
                    owner: SurfaceOwner::Screen,
                    side,
                    normal: -side.outward_normal(),
                });
            }
        }

        // so janelas grandes o bastante participam (inclusive como obstaculo)
        let eligible: Vec<&DesktopWindow> = windows
            .iter()
            .filter(|w| {
                w.rect.width() >= params.min_window_size
                    && w.rect.height() >= params.min_window_size
                    && w.rect.intersects(&screen)
            })
            .collect();

        for (i, w) in eligible.iter().enumerate() {
            let occluders: Vec<Rect> = eligible[..i].iter().map(|o| o.rect).collect();
            for side in Side::ALL {
                let normal = side.outward_normal();
                for seg in
                    visible_edge_segments(&w.rect, side, &occluders, &screen, params.min_segment)
                {
                    // lado voltado para fora da tela nao e caminhavel (a tela assume)
                    let probe = seg.point_at(0.5) + normal * 3.0;
                    if !screen.contains(probe) {
                        continue;
                    }
                    surfaces.push(Surface {
                        seg,
                        owner: SurfaceOwner::Window(w.id),
                        side,
                        normal,
                    });
                }
            }
        }

        Self::from_surfaces(screen, &surfaces, params.link_distance)
    }

    fn from_surfaces(screen: Rect, surfaces: &[Surface], link_distance: f32) -> PathGraph {
        let mut g = PathGraph {
            screen: Some(screen),
            ..Default::default()
        };

        // 1) pontos de corte em cada superficie: onde extremidades alheias encostam nela
        let mut cuts: Vec<Vec<f32>> = surfaces.iter().map(|_| vec![0.0, 1.0]).collect();
        for (i, s) in surfaces.iter().enumerate() {
            let len = s.seg.length();
            if len <= EPS {
                continue;
            }
            for (j, other) in surfaces.iter().enumerate() {
                if i == j {
                    continue;
                }
                for p in [other.seg.a, other.seg.b] {
                    let t = s.seg.closest_t(p);
                    if s.seg.point_at(t).distance(p) <= link_distance {
                        cuts[i].push(t);
                    }
                }
            }
        }

        // 2) nos + arestas de superficie
        for (i, s) in surfaces.iter().enumerate() {
            let len = s.seg.length();
            if len <= EPS {
                continue;
            }
            let ts = &mut cuts[i];
            ts.sort_by(|a, b| a.total_cmp(b));
            let mut prev: Option<NodeId> = None;
            let mut prev_pos = Vec2::ZERO;
            for &t in ts.iter() {
                let pos = s.seg.point_at(t);
                let node = g.node_at(pos);
                if let Some(p) = prev
                    && p != node
                    && prev_pos.distance(pos) > MERGE_DIST
                {
                    g.add_edge(p, node, EdgeKind::Surface(s.side), s.owner, s.normal);
                }
                prev = Some(node);
                prev_pos = pos;
            }
        }

        // 3) links entre extremidades e superficies de outros donos, quando nao coincidem
        for (i, s) in surfaces.iter().enumerate() {
            for (j, other) in surfaces.iter().enumerate() {
                if i == j || s.owner == other.owner {
                    continue;
                }
                for p in [other.seg.a, other.seg.b] {
                    if s.seg.length() <= EPS {
                        continue;
                    }
                    let t = s.seg.closest_t(p);
                    let q = s.seg.point_at(t);
                    let d = q.distance(p);
                    if d > MERGE_DIST && d <= link_distance {
                        let (np, nq) = (g.node_at(p), g.node_at(q));
                        if np != nq {
                            g.add_edge(np, nq, EdgeKind::Link, other.owner, Vec2::new(0.0, -1.0));
                        }
                    }
                }
            }
        }

        g.adjacency = vec![Vec::new(); g.nodes.len()];
        for (id, e) in g.edges.iter().enumerate() {
            g.adjacency[e.a as usize].push(id as EdgeId);
            g.adjacency[e.b as usize].push(id as EdgeId);
        }
        g
    }

    fn node_at(&mut self, pos: Vec2) -> NodeId {
        if let Some(i) = self
            .nodes
            .iter()
            .position(|n| n.distance_squared(pos) <= MERGE_DIST * MERGE_DIST)
        {
            return i as NodeId;
        }
        self.nodes.push(pos);
        (self.nodes.len() - 1) as NodeId
    }

    fn add_edge(
        &mut self,
        a: NodeId,
        b: NodeId,
        kind: EdgeKind,
        owner: SurfaceOwner,
        normal: Vec2,
    ) {
        // evita arestas duplicadas (mesmo par de nos e mesmo tipo)
        if self
            .edges
            .iter()
            .any(|e| e.kind == kind && ((e.a == a && e.b == b) || (e.a == b && e.b == a)))
        {
            return;
        }
        let length = self.nodes[a as usize].distance(self.nodes[b as usize]);
        let vertical = normal.x.abs() > normal.y.abs();
        let cost = match kind {
            EdgeKind::Link => length * 2.0 + 4.0,
            EdgeKind::Surface(_) if vertical => length * 1.4,
            EdgeKind::Surface(_) => length,
        };
        self.edges.push(Edge {
            a,
            b,
            kind,
            owner,
            normal,
            length,
            cost,
        });
    }
}

/// Mapeia um ponto que estava preso a `old` para a posicao equivalente em `new`
/// (proporcional ao retangulo). Usado para a aranha "viajar" junto com a janela
/// quando ela e movida ou redimensionada.
pub fn remap_point(p: Vec2, old: &Rect, new: &Rect) -> Vec2 {
    let ow = old.width().max(1.0);
    let oh = old.height().max(1.0);
    let rel = (p - old.min) / Vec2::new(ow, oh);
    new.min + rel * Vec2::new(new.width(), new.height())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::WindowState;

    pub(crate) fn win(id: u64, x: f32, y: f32, w: f32, h: f32) -> DesktopWindow {
        let r = Rect::from_xywh(x, y, w, h);
        DesktopWindow {
            id,
            rect: r,
            raw_rect: r,
            state: WindowState::Normal,
            pid: 1,
            title: format!("w{id}"),
        }
    }

    fn screen() -> Rect {
        Rect::from_xywh(0.0, 0.0, 1920.0, 1080.0)
    }

    #[test]
    fn empty_desktop_has_screen_perimeter_loop() {
        let g = PathGraph::build(screen(), &[], &BuildParams::default());
        assert_eq!(g.nodes.len(), 4);
        assert_eq!(g.edges.len(), 4);
        // cada canto liga duas arestas -> da para contornar a tela inteira
        for n in 0..4 {
            assert_eq!(g.edges_at(n).len(), 2);
        }
        let perimeter = 2.0 * (1920.0 + 1080.0);
        assert!((g.total_length() - perimeter).abs() < 1.0);
    }

    #[test]
    fn screen_normals_point_inward() {
        let g = PathGraph::build(screen(), &[], &BuildParams::default());
        for e in &g.edges {
            let seg = Segment::new(g.node(e.a), g.node(e.b));
            let probe = seg.point_at(0.5) + e.normal * 5.0;
            assert!(screen().contains(probe), "{e:?}");
        }
    }

    #[test]
    fn isolated_window_adds_four_edges_with_connected_corners() {
        let w = win(1, 400.0, 300.0, 600.0, 400.0);
        let g = PathGraph::build(screen(), &[w], &BuildParams::default());
        let owned: Vec<&Edge> = g
            .edges
            .iter()
            .filter(|e| {
                e.owner == SurfaceOwner::Window(1) && matches!(e.kind, EdgeKind::Surface(_))
            })
            .collect();
        assert_eq!(owned.len(), 4);
        // cantos compartilhados: 4 nos de janela, cada um com grau 2 (nao ha links: longe da tela)
        let corner = g
            .nodes
            .iter()
            .position(|n| n.distance(Vec2::new(400.0, 300.0)) < 1.0)
            .expect("canto superior esquerdo") as NodeId;
        assert_eq!(g.edges_at(corner).len(), 2);
        // normais para fora
        let top = owned
            .iter()
            .find(|e| e.kind == EdgeKind::Surface(Side::Top))
            .unwrap();
        assert_eq!(top.normal, Vec2::new(0.0, -1.0));
        assert!(!top.is_vertical());
        let right = owned
            .iter()
            .find(|e| e.kind == EdgeKind::Surface(Side::Right))
            .unwrap();
        assert!(right.is_vertical());
        assert!(right.cost > right.length);
    }

    #[test]
    fn small_windows_are_ignored() {
        let w = win(1, 400.0, 300.0, 80.0, 400.0);
        let g = PathGraph::build(screen(), &[w], &BuildParams::default());
        assert_eq!(g.owners(), vec![SurfaceOwner::Screen]);
    }

    #[test]
    fn maximized_window_defers_to_screen_edges() {
        // janela maximizada: topo/laterais coincidem com a tela e ficam voltados para fora
        let w = win(1, 0.0, 0.0, 1920.0, 1040.0);
        let g = PathGraph::build(screen(), &[w], &BuildParams::default());
        let win_edges: Vec<_> = g
            .edges
            .iter()
            .filter(|e| {
                e.owner == SurfaceOwner::Window(1) && matches!(e.kind, EdgeKind::Surface(_))
            })
            .collect();
        // so a base (y=1040, normal para baixo, dentro da tela) sobrevive
        assert_eq!(win_edges.len(), 1);
        assert_eq!(win_edges[0].kind, EdgeKind::Surface(Side::Bottom));
        assert_eq!(
            g.nearest_point(Vec2::new(500.0, 0.0), 1.0).unwrap().pos.y,
            0.0
        );
    }

    #[test]
    fn window_in_front_hides_back_window_edge() {
        let back = win(2, 300.0, 300.0, 600.0, 400.0);
        let front = win(1, 500.0, 200.0, 800.0, 300.0); // cobre parte da borda superior de back (y=300)
        let g = PathGraph::build(screen(), &[front, back], &BuildParams::default());
        // x=700, y=300 esta dentro da janela da frente: nao pode haver caminho ali
        // (a borda superior de `front` esta em y=200, nao 300)
        let hit = g.nearest_point(Vec2::new(700.0, 300.0), 2.0);
        assert!(
            hit.is_none(),
            "borda coberta nao deve ser caminhavel: {hit:?}"
        );
        // mas o trecho livre x=310..500 continua
        assert!(g.nearest_point(Vec2::new(400.0, 300.0), 2.0).is_some());
    }

    #[test]
    fn nearby_windows_get_a_link_edge() {
        // duas janelas com 8 px de folga horizontal: bordas laterais distintas e proximas
        let a = win(1, 100.0, 300.0, 400.0, 300.0);
        let b = win(2, 508.0, 400.0, 400.0, 300.0);
        let g = PathGraph::build(screen(), &[a, b], &BuildParams::default());
        let links = g.edges.iter().filter(|e| e.kind == EdgeKind::Link).count();
        assert!(
            links >= 1,
            "esperava ao menos um link entre janelas proximas"
        );
    }

    #[test]
    fn far_windows_have_no_links() {
        let a = win(1, 100.0, 300.0, 400.0, 300.0);
        let b = win(2, 900.0, 400.0, 400.0, 300.0);
        let g = PathGraph::build(screen(), &[a, b], &BuildParams::default());
        assert_eq!(
            g.edges.iter().filter(|e| e.kind == EdgeKind::Link).count(),
            0
        );
    }

    #[test]
    fn window_touching_screen_floor_is_split_and_connected() {
        // base da janela encosta no chao da tela: ponto de encontro
        let w = win(1, 600.0, 680.0, 500.0, 400.0); // base em y=1080
        let g = PathGraph::build(screen(), &[w], &BuildParams::default());
        // a aresta de chao da tela e partida nas extremidades da janela
        let floor_edges = g
            .edges
            .iter()
            .filter(|e| {
                e.owner == SurfaceOwner::Screen && e.kind == EdgeKind::Surface(Side::Bottom)
            })
            .count();
        assert!(
            floor_edges >= 3,
            "chao deveria ser partido em 3 trechos, got {floor_edges}"
        );
    }

    #[test]
    fn surface_below_finds_floor_and_window_tops() {
        let w = win(1, 400.0, 600.0, 600.0, 300.0);
        let g = PathGraph::build(screen(), &[w], &BuildParams::default());
        // sobre a janela: cai no topo dela (y=600)
        let p = g.surface_below(Vec2::new(700.0, 100.0)).unwrap();
        assert!((p.pos.y - 600.0).abs() < 1.0);
        // fora da janela: cai no chao da tela
        let p = g.surface_below(Vec2::new(100.0, 100.0)).unwrap();
        assert!((p.pos.y - 1080.0).abs() < 1.0);
        // abaixo do chao: nada
        assert!(g.surface_below(Vec2::new(100.0, 1500.0)).is_none());
    }

    #[test]
    fn nearest_point_respects_max_distance() {
        let g = PathGraph::build(screen(), &[], &BuildParams::default());
        assert!(g.nearest_point(Vec2::new(960.0, 540.0), 50.0).is_none());
        let p = g.nearest_point(Vec2::new(960.0, 1070.0), 50.0).unwrap();
        assert!((p.pos.y - 1080.0).abs() < 0.01);
    }

    #[test]
    fn remap_point_follows_translation_and_resize() {
        let old = Rect::from_xywh(100.0, 100.0, 400.0, 200.0);
        let moved = Rect::from_xywh(300.0, 250.0, 400.0, 200.0);
        // ponto na borda superior, a 1/4 do comprimento
        let p = Vec2::new(200.0, 100.0);
        assert_eq!(remap_point(p, &old, &moved), Vec2::new(400.0, 250.0));
        let resized = Rect::from_xywh(100.0, 100.0, 800.0, 200.0);
        let q = remap_point(p, &old, &resized);
        assert_eq!(q, Vec2::new(300.0, 100.0)); // continua na borda superior
    }
}
