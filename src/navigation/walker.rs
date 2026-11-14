//! Locomocao ao longo de uma [`Route`]: posicao, direcao, normal da superficie e queda livre.

use bevy::math::Vec2;

use super::geometry::Rect;
use super::pathfinding::Route;
use super::window_graph::{GraphPoint, PathGraph, SurfaceOwner, remap_point};
use crate::platform::DesktopWindow;

/// Aceleracao da gravidade na queda (px/s^2).
const GRAVITY: f32 = 2600.0;

/// Resultado de reconciliar a aranha com um grafo recem-reconstruido.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reattach {
    /// A rota atual continua valida.
    Kept,
    /// A rota foi descartada, mas a aranha foi presa em outra superficie proxima.
    Snapped(GraphPoint),
    /// Nao ha superficie por perto: queda livre.
    Falling,
    /// Nao existe nenhuma superficie (grafo vazio).
    Lost,
}

#[derive(Debug, Clone)]
pub struct Walker {
    pub pos: Vec2,
    /// Direcao de deslocamento (unitaria; mantem a ultima valida quando parada).
    pub heading: Vec2,
    /// Normal da superficie atual (a "cima" da aranha).
    pub normal: Vec2,
    pub owner: SurfaceOwner,
    route: Route,
    leg: usize,
    falling: bool,
    fall_speed: f32,
    /// Dono cujas superficies a queda atravessa (mob arremessado de uma janela sacudida).
    fall_through: Option<SurfaceOwner>,
}

impl Walker {
    pub fn new(pos: Vec2) -> Self {
        Self {
            pos,
            heading: Vec2::X,
            normal: Vec2::new(0.0, -1.0),
            owner: SurfaceOwner::Screen,
            route: Route::default(),
            leg: 0,
            falling: false,
            fall_speed: 0.0,
            fall_through: None,
        }
    }

    pub fn is_falling(&self) -> bool {
        self.falling
    }

    pub fn has_route(&self) -> bool {
        self.leg < self.route.legs.len()
    }

    pub fn route(&self) -> &Route {
        &self.route
    }

    /// Trechos que ainda faltam percorrer (inclui o atual, parcialmente).
    pub fn remaining_route(&self) -> Route {
        Route {
            legs: self.route.legs.get(self.leg..).unwrap_or(&[]).to_vec(),
        }
    }

    /// Esta andando sobre superficie vertical?
    pub fn is_climbing(&self) -> bool {
        !self.falling && self.normal.x.abs() > self.normal.y.abs()
    }

    /// Prende a aranha a um ponto do grafo e descarta a rota.
    pub fn place(&mut self, p: GraphPoint, graph: &PathGraph) {
        let e = graph.edge(p.edge);
        self.pos = p.pos;
        self.normal = e.normal;
        self.owner = e.owner;
        self.route = Route::default();
        self.leg = 0;
        self.falling = false;
        self.fall_speed = 0.0;
        self.fall_through = None;
    }

    pub fn set_route(&mut self, route: Route) {
        self.route = route;
        self.leg = 0;
        self.falling = false;
        self.fall_speed = 0.0;
        self.fall_through = None;
        self.sync_from_leg();
    }

    pub fn stop(&mut self) {
        self.route = Route::default();
        self.leg = 0;
    }

    fn sync_from_leg(&mut self) {
        if let Some(l) = self.route.legs.get(self.leg) {
            let dir = l.direction();
            if dir != Vec2::ZERO {
                self.heading = dir;
            }
            self.normal = l.normal;
            self.owner = l.owner;
        }
    }

    /// Avanca `dist` px ao longo da rota. Devolve `true` ao terminar a rota neste passo.
    pub fn advance(&mut self, mut dist: f32) -> bool {
        if self.falling {
            return false;
        }
        while dist > 0.0 {
            let Some(leg) = self.route.legs.get(self.leg).copied() else {
                return false;
            };
            self.normal = leg.normal;
            self.owner = leg.owner;
            let to_go = self.pos.distance(leg.to);
            let dir = (leg.to - self.pos).normalize_or_zero();
            if dir != Vec2::ZERO {
                self.heading = dir;
            }
            if dist >= to_go {
                self.pos = leg.to;
                dist -= to_go;
                self.leg += 1;
                if self.leg >= self.route.legs.len() {
                    return true;
                }
                self.sync_from_leg();
            } else {
                self.pos += dir * dist;
                dist = 0.0;
            }
        }
        false
    }

    pub fn start_fall(&mut self) {
        self.route = Route::default();
        self.leg = 0;
        self.falling = true;
        self.fall_speed = 0.0;
        self.fall_through = None;
        self.normal = Vec2::new(0.0, -1.0);
    }

    /// Arremessada da superficie atual (janela sacudida): cai atravessando as bordas da propria
    /// janela, ate o chao ou o topo de outra janela, e deixa de viajar com a janela de origem.
    pub fn throw_off(&mut self) {
        let from = self.owner;
        self.start_fall();
        self.fall_through = matches!(from, SurfaceOwner::Window(_)).then_some(from);
        self.owner = SurfaceOwner::Screen;
    }

    /// Passo da queda livre. Devolve o ponto de pouso, se tocou uma superficie.
    pub fn update_fall(&mut self, dt: f32, graph: &PathGraph) -> Option<GraphPoint> {
        if !self.falling {
            return None;
        }
        self.fall_speed += GRAVITY * dt;
        let new_y = self.pos.y + self.fall_speed * dt;
        let landing = graph.surface_below_except(self.pos, self.fall_through);
        match landing {
            Some(l) if new_y >= l.pos.y => {
                self.place(l, graph);
                Some(l)
            }
            Some(_) => {
                self.pos.y = new_y;
                None
            }
            None => {
                // sem chao algum (ex.: grafo vazio): continua ate sair do limite e pede nova fixacao
                self.pos.y = new_y;
                None
            }
        }
    }

    /// A aranha viaja com a janela em que esta presa quando ela e movida/redimensionada.
    pub fn follow_window(&mut self, old: &[DesktopWindow], new: &[DesktopWindow]) {
        let SurfaceOwner::Window(id) = self.owner else {
            return;
        };
        let (Some(o), Some(n)) = (
            old.iter().find(|w| w.id == id),
            new.iter().find(|w| w.id == id),
        ) else {
            return;
        };
        if !o.rect.approx_eq(&n.rect, 0.5) {
            self.pos = remap_point(self.pos, &o.rect, &n.rect);
        }
    }

    /// Reconcilia com o grafo novo. `snap_tol` = distancia maxima para "grudar" na superficie.
    pub fn reattach(&mut self, graph: &PathGraph, snap_tol: f32) -> Reattach {
        if graph.is_empty() {
            self.stop();
            return Reattach::Lost;
        }
        if self.falling {
            return Reattach::Kept;
        }
        if self.has_route()
            && self.remaining_route().still_valid(graph, 2.5)
            && graph.contains_point(self.pos, 2.5)
        {
            return Reattach::Kept;
        }
        self.stop();
        if let Some(p) = graph.nearest_point(self.pos, snap_tol) {
            self.place(p, graph);
            return Reattach::Snapped(p);
        }
        self.start_fall();
        Reattach::Falling
    }

    /// Mantem a aranha dentro de `bounds` (rede de seguranca contra coordenadas absurdas).
    pub fn clamp_to(&mut self, bounds: &Rect) {
        self.pos = self.pos.clamp(bounds.min, bounds.max);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::navigation::pathfinding::plan_route;
    use crate::navigation::window_graph::BuildParams;
    use crate::platform::WindowState;

    fn screen() -> Rect {
        Rect::from_xywh(0.0, 0.0, 1920.0, 1080.0)
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

    #[test]
    fn walks_along_route_and_reports_arrival() {
        let g = PathGraph::build(screen(), &[], &BuildParams::default());
        let a = g.nearest_point(Vec2::new(100.0, 1080.0), 5.0).unwrap();
        let b = g.nearest_point(Vec2::new(400.0, 1080.0), 5.0).unwrap();
        let mut w = Walker::new(a.pos);
        w.set_route(plan_route(&g, a, b).unwrap());
        assert!(w.has_route());
        assert!(!w.advance(100.0));
        assert!((w.pos.x - 200.0).abs() < 0.01);
        assert!(w.heading.x > 0.99);
        assert!(!w.is_climbing());
        assert!(w.advance(1000.0));
        assert!((w.pos.x - 400.0).abs() < 0.01);
        assert!(!w.has_route());
    }

    #[test]
    fn climbing_flag_follows_the_surface() {
        let g = PathGraph::build(screen(), &[], &BuildParams::default());
        let floor = g.nearest_point(Vec2::new(60.0, 1080.0), 5.0).unwrap();
        let wall = g.nearest_point(Vec2::new(0.0, 800.0), 5.0).unwrap();
        let mut w = Walker::new(floor.pos);
        w.set_route(plan_route(&g, floor, wall).unwrap());
        assert!(!w.is_climbing());
        w.advance(60.0 + 10.0); // passa do canto
        assert!(w.is_climbing());
        assert!(
            w.heading.y < -0.9,
            "subindo: heading.y negativo (Y de tela)"
        );
    }

    #[test]
    fn single_big_advance_crosses_multiple_legs_without_overshoot() {
        let g = PathGraph::build(screen(), &[], &BuildParams::default());
        let a = g.nearest_point(Vec2::new(100.0, 1080.0), 5.0).unwrap();
        let b = g.nearest_point(Vec2::new(1000.0, 0.0), 5.0).unwrap();
        let route = plan_route(&g, a, b).unwrap();
        let total = route.total_length();
        let mut w = Walker::new(a.pos);
        w.set_route(route);
        assert!(w.advance(total + 500.0));
        assert!(w.pos.distance(b.pos) < 0.5);
    }

    #[test]
    fn follows_moved_window_then_route_is_replanned_by_reattach() {
        let before = vec![win(1, 400.0, 300.0, 600.0, 400.0)];
        let after = vec![win(1, 500.0, 450.0, 600.0, 400.0)];
        let g1 = PathGraph::build(screen(), &before, &BuildParams::default());
        let g2 = PathGraph::build(screen(), &after, &BuildParams::default());
        let p = g1.nearest_point(Vec2::new(600.0, 300.0), 3.0).unwrap();
        let q = g1.nearest_point(Vec2::new(900.0, 300.0), 3.0).unwrap();
        let mut w = Walker::new(p.pos);
        w.place(p, &g1);
        w.set_route(plan_route(&g1, p, q).unwrap());
        assert_eq!(w.owner, SurfaceOwner::Window(1));

        w.follow_window(&before, &after);
        assert!(
            (w.pos - Vec2::new(700.0, 450.0)).length() < 0.5,
            "{:?}",
            w.pos
        );
        // rota antiga invalida => re-fixa no topo da janela nova
        match w.reattach(&g2, 20.0) {
            Reattach::Snapped(p) => assert!((p.pos.y - 450.0).abs() < 0.5),
            other => panic!("esperava Snapped, veio {other:?}"),
        }
        assert!(!w.has_route());
    }

    #[test]
    fn window_gone_makes_spider_fall_to_floor() {
        let before = vec![win(1, 400.0, 300.0, 600.0, 400.0)];
        let g1 = PathGraph::build(screen(), &before, &BuildParams::default());
        let g2 = PathGraph::build(screen(), &[], &BuildParams::default());
        let p = g1.nearest_point(Vec2::new(600.0, 300.0), 3.0).unwrap();
        let mut w = Walker::new(p.pos);
        w.place(p, &g1);
        // sem a janela, o topo some e a aranha esta longe de qualquer superficie
        assert_eq!(w.reattach(&g2, 20.0), Reattach::Falling);
        assert!(w.is_falling());
        let mut landed = None;
        for _ in 0..600 {
            if let Some(l) = w.update_fall(1.0 / 60.0, &g2) {
                landed = Some(l);
                break;
            }
        }
        let l = landed.expect("deveria pousar no chao");
        assert!((l.pos.y - 1080.0).abs() < 0.5);
        assert!(!w.is_falling());
    }

    #[test]
    fn thrown_off_a_window_falls_through_it_and_stops_following_it() {
        // janela 1 em cima, janela 2 logo abaixo e mais larga: o pouso e no topo da 2
        let ws = vec![
            win(1, 400.0, 200.0, 400.0, 200.0),
            win(2, 300.0, 600.0, 800.0, 300.0),
        ];
        let g = PathGraph::build(screen(), &ws, &BuildParams::default());
        let top = g.nearest_point(Vec2::new(600.0, 200.0), 3.0).unwrap();
        let mut w = Walker::new(top.pos);
        w.place(top, &g);
        assert_eq!(w.owner, SurfaceOwner::Window(1));

        // queda comum: "pousa" na hora no proprio topo em que ja esta
        let mut plain = w.clone();
        plain.start_fall();
        assert!(plain.update_fall(1.0 / 60.0, &g).is_some());
        assert!((plain.pos.y - 200.0).abs() < 0.5);

        w.throw_off();
        assert!(w.is_falling());
        assert_eq!(w.owner, SurfaceOwner::Screen);
        // a janela de origem se move durante a queda: o mob nao vai junto
        let moved = vec![win(1, 900.0, 200.0, 400.0, 200.0), ws[1].clone()];
        let x = w.pos.x;
        w.follow_window(&ws, &moved);
        assert_eq!(w.pos.x, x);

        let mut landed = None;
        for _ in 0..600 {
            if let Some(l) = w.update_fall(1.0 / 60.0, &g) {
                landed = Some(l);
                break;
            }
        }
        let l = landed.expect("deveria pousar");
        assert!((l.pos.y - 600.0).abs() < 0.5, "topo da janela 2: {l:?}");
        assert_eq!(w.owner, SurfaceOwner::Window(2));
        assert!(!w.is_falling());

        // depois de pousar a excecao acabou: uma nova queda comum respeita todas as superficies
        w.start_fall();
        assert!(w.update_fall(1.0 / 60.0, &g).is_some());
        assert!((w.pos.y - 600.0).abs() < 0.5);
    }

    #[test]
    fn valid_route_is_kept_when_graph_is_unchanged() {
        let g = PathGraph::build(screen(), &[], &BuildParams::default());
        let a = g.nearest_point(Vec2::new(100.0, 1080.0), 5.0).unwrap();
        let b = g.nearest_point(Vec2::new(900.0, 1080.0), 5.0).unwrap();
        let mut w = Walker::new(a.pos);
        w.set_route(plan_route(&g, a, b).unwrap());
        w.advance(50.0);
        assert_eq!(w.reattach(&g, 20.0), Reattach::Kept);
        assert!(w.has_route());
    }

    #[test]
    fn empty_graph_is_lost_not_a_panic() {
        let g = PathGraph::build(
            screen(),
            &[],
            &BuildParams {
                screen_edges: false,
                ..Default::default()
            },
        );
        let mut w = Walker::new(Vec2::new(10.0, 10.0));
        assert_eq!(w.reattach(&g, 20.0), Reattach::Lost);
    }
}
