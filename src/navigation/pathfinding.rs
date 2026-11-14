//! A* generico e planejamento de rotas sobre o [`PathGraph`].

use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::hash::Hash;

use bevy::math::Vec2;

use super::window_graph::{EdgeId, GraphPoint, NodeId, PathGraph, SurfaceOwner};
use crate::rng::Rng;

/// Entrada da fila de prioridade (menor `f` primeiro).
struct HeapItem<N> {
    f: f32,
    node: N,
}

impl<N> PartialEq for HeapItem<N> {
    fn eq(&self, other: &Self) -> bool {
        self.f.total_cmp(&other.f) == Ordering::Equal
    }
}
impl<N> Eq for HeapItem<N> {}
impl<N> PartialOrd for HeapItem<N> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<N> Ord for HeapItem<N> {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap e max-heap: inverte para obter o menor f
        other.f.total_cmp(&self.f)
    }
}

/// A* generico. `starts` traz (no, custo inicial). Devolve o caminho e o custo total.
pub fn astar<N, G, I, H>(
    starts: &[(N, f32)],
    is_goal: G,
    mut neighbors: impl FnMut(N) -> I,
    heuristic: H,
) -> Option<(Vec<N>, f32)>
where
    N: Copy + Eq + Hash,
    G: Fn(N) -> bool,
    I: IntoIterator<Item = (N, f32)>,
    H: Fn(N) -> f32,
{
    let mut open = BinaryHeap::new();
    let mut best_g: HashMap<N, f32> = HashMap::new();
    let mut came_from: HashMap<N, N> = HashMap::new();

    for &(n, g) in starts {
        if best_g.get(&n).is_none_or(|&old| g < old) {
            best_g.insert(n, g);
            open.push(HeapItem {
                f: g + heuristic(n),
                node: n,
            });
        }
    }

    while let Some(HeapItem { f, node }) = open.pop() {
        let g = best_g[&node];
        // entrada obsoleta (um caminho melhor ja foi achado)
        if f > g + heuristic(node) + 1e-3 {
            continue;
        }
        if is_goal(node) {
            let mut path = vec![node];
            let mut cur = node;
            while let Some(&prev) = came_from.get(&cur) {
                path.push(prev);
                cur = prev;
            }
            path.reverse();
            return Some((path, g));
        }
        for (next, cost) in neighbors(node) {
            let ng = g + cost;
            if best_g.get(&next).is_none_or(|&old| ng < old) {
                best_g.insert(next, ng);
                came_from.insert(next, node);
                open.push(HeapItem {
                    f: ng + heuristic(next),
                    node: next,
                });
            }
        }
    }
    None
}

/// Um trecho reto da rota: ir de `from` ate `to` sobre uma superficie com `normal`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Leg {
    pub from: Vec2,
    pub to: Vec2,
    pub normal: Vec2,
    pub owner: SurfaceOwner,
}

impl Leg {
    pub fn length(&self) -> f32 {
        self.from.distance(self.to)
    }

    pub fn direction(&self) -> Vec2 {
        (self.to - self.from).normalize_or_zero()
    }

    pub fn is_vertical(&self) -> bool {
        self.normal.x.abs() > self.normal.y.abs()
    }
}

/// Sequencia de trechos que leva a aranha ate o destino.
#[derive(Debug, Clone, Default)]
pub struct Route {
    pub legs: Vec<Leg>,
}

impl Route {
    pub fn total_length(&self) -> f32 {
        self.legs.iter().map(Leg::length).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.legs.is_empty()
    }

    pub fn destination(&self) -> Option<Vec2> {
        self.legs.last().map(|l| l.to)
    }

    /// Todos os pontos da rota estao a no maximo `tol` px do grafo atual?
    /// Usado apos o grafo ser reconstruido para decidir se a rota continua valida.
    pub fn still_valid(&self, graph: &PathGraph, tol: f32) -> bool {
        self.legs.iter().all(|l| {
            [l.from, l.from.lerp(l.to, 0.5), l.to]
                .iter()
                .all(|&p| graph.contains_point(p, tol))
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum State {
    Node(NodeId),
    Goal,
}

/// Planeja a rota entre dois pontos do grafo com A*.
pub fn plan_route(graph: &PathGraph, from: GraphPoint, to: GraphPoint) -> Option<Route> {
    if from.edge as usize >= graph.edges.len() || to.edge as usize >= graph.edges.len() {
        return None;
    }
    let from_edge = *graph.edge(from.edge);
    let to_edge = *graph.edge(to.edge);

    // mesma aresta: linha reta
    if from.edge == to.edge {
        return Some(Route {
            legs: vec![Leg {
                from: from.pos,
                to: to.pos,
                normal: from_edge.normal,
                owner: from_edge.owner,
            }],
        });
    }

    let cost_ratio = |e: &super::window_graph::Edge| {
        if e.length > 0.0 {
            e.cost / e.length
        } else {
            1.0
        }
    };
    let from_ratio = cost_ratio(&from_edge);
    let to_ratio = cost_ratio(&to_edge);

    let a_pos = graph.node(from_edge.a);
    let b_pos = graph.node(from_edge.b);
    let starts = [
        (
            State::Node(from_edge.a),
            from.pos.distance(a_pos) * from_ratio,
        ),
        (
            State::Node(from_edge.b),
            from.pos.distance(b_pos) * from_ratio,
        ),
    ];

    let goal_pos = to.pos;
    let found = astar(
        &starts,
        |s| s == State::Goal,
        |s| {
            let mut out: Vec<(State, f32)> = Vec::new();
            if let State::Node(n) = s {
                for &eid in graph.edges_at(n) {
                    let e = graph.edge(eid);
                    let other = if e.a == n { e.b } else { e.a };
                    out.push((State::Node(other), e.cost));
                }
                if n == to_edge.a || n == to_edge.b {
                    out.push((State::Goal, graph.node(n).distance(goal_pos) * to_ratio));
                }
            }
            out
        },
        |s| match s {
            State::Node(n) => graph.node(n).distance(goal_pos),
            State::Goal => 0.0,
        },
    )?;

    let nodes: Vec<NodeId> = found
        .0
        .iter()
        .filter_map(|s| {
            if let State::Node(n) = s {
                Some(*n)
            } else {
                None
            }
        })
        .collect();
    let first = *nodes.first()?;
    let last = *nodes.last()?;

    let mut legs = Vec::with_capacity(nodes.len() + 1);
    legs.push(Leg {
        from: from.pos,
        to: graph.node(first),
        normal: from_edge.normal,
        owner: from_edge.owner,
    });
    for pair in nodes.windows(2) {
        let (u, v) = (pair[0], pair[1]);
        // aresta que liga u-v (a de menor custo, se houver paralelas)
        let best: Option<EdgeId> = graph
            .edges_at(u)
            .iter()
            .copied()
            .filter(|&eid| {
                let e = graph.edge(eid);
                (e.a == u && e.b == v) || (e.a == v && e.b == u)
            })
            .min_by(|&x, &y| graph.edge(x).cost.total_cmp(&graph.edge(y).cost));
        let e = graph.edge(best?);
        legs.push(Leg {
            from: graph.node(u),
            to: graph.node(v),
            normal: e.normal,
            owner: e.owner,
        });
    }
    legs.push(Leg {
        from: graph.node(last),
        to: to.pos,
        normal: to_edge.normal,
        owner: to_edge.owner,
    });

    // remove trechos degenerados (aranha ja estava exatamente no no)
    legs.retain(|l| l.length() > 0.25);
    Some(Route { legs })
}

/// Sorteia um ponto do grafo proporcional ao comprimento das arestas.
pub fn random_point(graph: &PathGraph, rng: &mut Rng) -> Option<GraphPoint> {
    let weights: Vec<f32> = graph.edges.iter().map(|e| e.length).collect();
    let idx = rng.weighted(&weights)?;
    Some(graph.point_on(idx as EdgeId, rng.f32()))
}

/// Escolhe um destino alcancavel. Com `away_from`, prefere o candidato mais distante daquele ponto
/// (fuga); caso contrario sorteia entre candidatos a pelo menos `min_dist` do ponto atual.
pub fn choose_destination(
    graph: &PathGraph,
    from: GraphPoint,
    away_from: Option<Vec2>,
    min_dist: f32,
    rng: &mut Rng,
) -> Option<Route> {
    const CANDIDATES: usize = 8;
    let mut best: Option<(f32, Route)> = None;
    for _ in 0..CANDIDATES {
        let Some(target) = random_point(graph, rng) else {
            break;
        };
        if target.pos.distance(from.pos) < min_dist {
            continue;
        }
        let Some(route) = plan_route(graph, from, target) else {
            continue;
        };
        if route.is_empty() {
            continue;
        }
        let score = match away_from {
            Some(c) => target.pos.distance(c),
            None => rng.f32(), // sorteio uniforme entre os viaveis
        };
        if best.as_ref().is_none_or(|(s, _)| score > *s) {
            best = Some((score, route));
        }
    }
    best.map(|(_, r)| r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::geometry::Rect;
    use crate::navigation::window_graph::{BuildParams, SurfaceOwner};
    use crate::platform::{DesktopWindow, WindowState};

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

    fn screen() -> Rect {
        Rect::from_xywh(0.0, 0.0, 1920.0, 1080.0)
    }

    // --- A* generico em grade 5x5 com paredes ---

    #[test]
    fn astar_finds_shortest_path_around_wall() {
        // 0,0 ... 4,4 ; parede vertical em x=2 para y in 0..=3
        let wall = |p: (i32, i32)| p.0 == 2 && (0..=3).contains(&p.1);
        let goal = (4, 0);
        let res = astar(
            &[((0, 0), 0.0)],
            |p| p == goal,
            |(x, y)| {
                let mut v = Vec::new();
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let n = (x + dx, y + dy);
                    if (0..5).contains(&n.0) && (0..5).contains(&n.1) && !wall(n) {
                        v.push((n, 1.0));
                    }
                }
                v
            },
            |(x, y)| ((x - goal.0).abs() + (y - goal.1).abs()) as f32,
        )
        .expect("deve existir caminho");
        // desce ate y=4 (4 passos), atravessa x=2 (4 passos) e sobe (4 passos) = 12
        assert_eq!(res.1, 12.0);
        assert_eq!(res.0.first(), Some(&(0, 0)));
        assert_eq!(res.0.last(), Some(&goal));
        assert!(res.0.iter().all(|p| !wall(*p)));
    }

    #[test]
    fn astar_returns_none_when_unreachable() {
        let res = astar(
            &[(0u32, 0.0)],
            |n| n == 9,
            |n| {
                if n < 3 {
                    vec![(n + 1, 1.0)]
                } else {
                    Vec::new()
                }
            },
            |_| 0.0,
        );
        assert!(res.is_none());
    }

    #[test]
    fn astar_multiple_starts_pick_best_total() {
        // dois inicios: 'A' (custo inicial 10) esta ao lado do alvo; 'B' (custo 0) esta longe (5)
        let res = astar(
            &[('A', 10.0), ('B', 0.0)],
            |n| n == 'G',
            |n| match n {
                'A' => vec![('G', 1.0)],
                'B' => vec![('G', 5.0)],
                _ => vec![],
            },
            |_| 0.0,
        )
        .unwrap();
        assert_eq!(res.1, 5.0);
        assert_eq!(res.0, vec!['B', 'G']);
    }

    // --- planejamento sobre o grafo ---

    #[test]
    fn route_on_same_edge_is_straight_line() {
        let g = PathGraph::build(screen(), &[], &BuildParams::default());
        let a = g.nearest_point(Vec2::new(300.0, 1080.0), 5.0).unwrap();
        let b = g.nearest_point(Vec2::new(900.0, 1080.0), 5.0).unwrap();
        let r = plan_route(&g, a, b).unwrap();
        assert_eq!(r.legs.len(), 1);
        assert!((r.total_length() - 600.0).abs() < 1.0);
    }

    #[test]
    fn route_goes_around_screen_corner_climbing_the_wall() {
        let g = PathGraph::build(screen(), &[], &BuildParams::default());
        let floor = g.nearest_point(Vec2::new(100.0, 1080.0), 5.0).unwrap();
        let wall = g.nearest_point(Vec2::new(0.0, 700.0), 5.0).unwrap();
        let r = plan_route(&g, floor, wall).unwrap();
        // 100 px no chao + 380 px subindo a parede esquerda
        assert!(
            (r.total_length() - 480.0).abs() < 2.0,
            "{}",
            r.total_length()
        );
        assert!(
            r.legs.iter().any(|l| l.is_vertical()),
            "deveria escalar a parede"
        );
        // continuidade da rota
        for w in r.legs.windows(2) {
            assert!(w[0].to.distance(w[1].from) < 1.0);
        }
        assert!(r.destination().unwrap().distance(wall.pos) < 1.0);
    }

    #[test]
    fn route_takes_shorter_direction_around_the_loop() {
        let g = PathGraph::build(screen(), &[], &BuildParams::default());
        // dois pontos no chao: uma volta direta de 1700 px vs a volta completa pelo perimetro
        let a = g.nearest_point(Vec2::new(100.0, 1080.0), 5.0).unwrap();
        let b = g.nearest_point(Vec2::new(1800.0, 1080.0), 5.0).unwrap();
        let r = plan_route(&g, a, b).unwrap();
        assert!((r.total_length() - 1700.0).abs() < 2.0);
    }

    #[test]
    fn route_climbs_from_floor_onto_window_via_shared_corner() {
        // janela apoiada no chao: da para ir do chao ate o topo dela escalando uma lateral
        let w = win(1, 600.0, 680.0, 500.0, 400.0);
        let g = PathGraph::build(screen(), &[w], &BuildParams::default());
        let floor = g.nearest_point(Vec2::new(300.0, 1080.0), 5.0).unwrap();
        let top = g.nearest_point(Vec2::new(850.0, 680.0), 5.0).unwrap();
        let r = plan_route(&g, floor, top).expect("rota ate o topo da janela");
        let expected = 300.0 + 400.0 + 250.0; // chao ate a base da janela + subir + andar no topo
        assert!(r.total_length() >= expected - 5.0);
        assert!(r.total_length() < expected * 1.6, "{}", r.total_length());
        assert!(r.legs.iter().any(|l| l.owner == SurfaceOwner::Window(1)));
    }

    #[test]
    fn disconnected_islands_have_no_route() {
        // sem perimetro da tela e janelas distantes => grafo desconexo
        let params = BuildParams {
            screen_edges: false,
            ..Default::default()
        };
        let a = win(1, 100.0, 300.0, 300.0, 300.0);
        let b = win(2, 900.0, 300.0, 300.0, 300.0);
        let g = PathGraph::build(screen(), &[a, b], &params);
        let pa = g.nearest_point(Vec2::new(250.0, 300.0), 5.0).unwrap();
        let pb = g.nearest_point(Vec2::new(1050.0, 300.0), 5.0).unwrap();
        assert!(plan_route(&g, pa, pb).is_none());
    }

    #[test]
    fn invalid_edge_ids_do_not_panic() {
        let g = PathGraph::build(screen(), &[], &BuildParams::default());
        let ok = g.nearest_point(Vec2::new(10.0, 1080.0), 5.0).unwrap();
        let bad = GraphPoint {
            edge: 9999,
            t: 0.0,
            pos: Vec2::ZERO,
        };
        assert!(plan_route(&g, ok, bad).is_none());
        assert!(plan_route(&g, bad, ok).is_none());
    }

    #[test]
    fn route_validity_after_window_moves() {
        let w = win(1, 400.0, 300.0, 600.0, 400.0);
        let g1 = PathGraph::build(screen(), std::slice::from_ref(&w), &BuildParams::default());
        let a = g1.nearest_point(Vec2::new(450.0, 300.0), 5.0).unwrap();
        let b = g1.nearest_point(Vec2::new(900.0, 300.0), 5.0).unwrap();
        let route = plan_route(&g1, a, b).unwrap();
        assert!(route.still_valid(&g1, 2.0));

        // janela movida 200 px para baixo: a rota antiga some do grafo
        let moved = win(1, 400.0, 500.0, 600.0, 400.0);
        let g2 = PathGraph::build(screen(), &[moved], &BuildParams::default());
        assert!(!route.still_valid(&g2, 2.0));

        // janela minimizada (ausente): idem
        let g3 = PathGraph::build(screen(), &[], &BuildParams::default());
        assert!(!route.still_valid(&g3, 2.0));
    }

    #[test]
    fn choose_destination_flees_far_from_cursor() {
        let g = PathGraph::build(screen(), &[], &BuildParams::default());
        let from = g.nearest_point(Vec2::new(960.0, 1080.0), 5.0).unwrap();
        let cursor = Vec2::new(960.0, 1000.0);
        let mut rng = Rng::new(11);
        let mut total = 0.0;
        let mut n = 0.0;
        for _ in 0..30 {
            let route = choose_destination(&g, from, Some(cursor), 100.0, &mut rng).unwrap();
            total += route.destination().unwrap().distance(cursor);
            n += 1.0;
        }
        // com 8 candidatos, a media de distancia fica bem acima de um sorteio qualquer (~700 px)
        assert!(total / n > 900.0, "media {}", total / n);
    }

    #[test]
    fn choose_destination_respects_min_distance() {
        let g = PathGraph::build(screen(), &[], &BuildParams::default());
        let from = g.nearest_point(Vec2::new(960.0, 1080.0), 5.0).unwrap();
        let mut rng = Rng::new(5);
        for _ in 0..50 {
            if let Some(r) = choose_destination(&g, from, None, 300.0, &mut rng) {
                assert!(r.destination().unwrap().distance(from.pos) >= 300.0);
            }
        }
    }
}
