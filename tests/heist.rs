//! Sequestro do mouse de ponta a ponta e sem GPU: um mob de verdade (grafo, rota, andar pelas
//! superficies) dirigido pelas regras da cacada, com um cursor simulado que obedece aos "warps".

use bevy::math::Vec2;

use spiderdesk::config::Config;
use spiderdesk::creature::spider::SpiderSim;
use spiderdesk::game::heist::{Heist, HeistEvent, HeistInput, Hunter, web_center};
use spiderdesk::navigation::geometry::Rect;
use spiderdesk::navigation::pathfinding::plan_route;
use spiderdesk::navigation::window_graph::{BuildParams, PathGraph};
use spiderdesk::platform::{DesktopWindow, WindowState};

const DT: f32 = 1.0 / 30.0;

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

fn graph(windows: &[DesktopWindow], silk_threads: bool) -> PathGraph {
    PathGraph::build(
        screen(),
        windows,
        &BuildParams {
            silk_threads,
            ..BuildParams::default()
        },
    )
}

fn config() -> Config {
    let mut cfg = Config::default();
    cfg.behavior.seed = 7;
    cfg.web.idle_secs = 5.0;
    cfg
}

fn mob_at(at: Vec2, g: &PathGraph, cfg: &Config) -> SpiderSim {
    let mut sim = SpiderSim::new(3);
    sim.spawn_at = Some(at);
    assert!(sim.step(DT, g, &screen(), None, cfg).placed);
    sim
}

#[test]
fn silk_threads_bridge_a_floating_window_to_the_floor() {
    // janela solta no meio da tela: sem fios e uma ilha; com fios, da para chegar ao chao
    let ws = vec![win(1, 600.0, 300.0, 500.0, 300.0)];
    let on_window = Vec2::new(850.0, 300.0);
    let on_floor = Vec2::new(200.0, 1080.0);

    let island = graph(&ws, false);
    assert!(island.threads.is_empty());
    let (a, b) = (
        island.nearest_point(on_window, 5.0).unwrap(),
        island.nearest_point(on_floor, 5.0).unwrap(),
    );
    assert!(plan_route(&island, a, b).is_none(), "sem fio: ilha");

    let bridged = graph(&ws, true);
    assert_eq!(bridged.threads.len(), 2, "um fio em cada canto de baixo");
    for t in &bridged.threads {
        assert_eq!(t.a.y, 600.0, "sai da base da janela");
        assert_eq!(t.b.y, 1080.0, "chega ao chao");
        assert!(t.a.x > 600.0 && t.a.x < 1100.0);
    }
    let (a, b) = (
        bridged.nearest_point(on_window, 5.0).unwrap(),
        bridged.nearest_point(on_floor, 5.0).unwrap(),
    );
    assert!(plan_route(&bridged, a, b).is_some(), "com fio: ha caminho");
}

#[test]
fn silk_thread_lands_on_the_window_below_not_through_it() {
    let ws = vec![
        win(1, 600.0, 200.0, 400.0, 250.0),
        win(2, 500.0, 650.0, 700.0, 300.0),
    ];
    let g = graph(&ws, true);
    let from_top = g.threads.iter().filter(|t| t.a.y == 450.0);
    assert_eq!(from_top.clone().count(), 2);
    for t in from_top {
        assert_eq!(t.b.y, 650.0, "para no topo da janela de baixo");
    }
}

#[test]
fn errand_walks_the_mob_along_surfaces_to_the_target_and_reports_arrival() {
    let cfg = config();
    let g = graph(&[], false);
    let mut sim = mob_at(Vec2::new(900.0, 1080.0), &g, &cfg);
    let target = Vec2::new(40.0, 30.0); // perto do canto de cima: precisa subir a parede
    let mut arrived_at = None;
    for i in 0..(60.0 / DT) as u32 {
        sim.send_to(target, 300.0);
        sim.step(DT, &g, &screen(), Some(sim.body_center), &cfg);
        // sempre em cima de alguma superficie (nunca voando em linha reta)
        assert!(
            g.contains_point(sim.walker.pos, 2.0),
            "{:?}",
            sim.walker.pos
        );
        if sim.errand.is_some_and(|e| e.arrived) {
            arrived_at = Some(i as f32 * DT);
            break;
        }
    }
    let secs = arrived_at.expect("nao chegou em 60 s");
    assert!(
        sim.walker.pos.distance(target) < 60.0,
        "{:?}",
        sim.walker.pos
    );
    // 860 px de chao + ~1050 de parede a 300 px/s
    assert!((4.0..12.0).contains(&secs), "levou {secs} s");

    sim.end_errand();
    assert!(sim.errand.is_none() && !sim.walker.has_route());
}

/// Roda a cacada com um mob real; devolve os eventos na ordem.
struct Sim {
    cfg: Config,
    g: PathGraph,
    mob: SpiderSim,
    heist: Heist,
    cursor: Vec2,
    sent: Option<Vec2>,
    events: Vec<HeistEvent>,
    /// Maior folga vista entre o cursor e o comprimento do fio.
    max_rope: f32,
}

impl Sim {
    fn new() -> Self {
        let cfg = config();
        let g = graph(&[win(1, 700.0, 350.0, 600.0, 350.0)], true);
        let mob = mob_at(Vec2::new(1500.0, 1080.0), &g, &cfg);
        let heist = Heist::new(9, &cfg.web);
        Self {
            cfg,
            g,
            mob,
            heist,
            cursor: Vec2::new(400.0, 600.0),
            sent: None,
            events: Vec::new(),
            max_rope: 0.0,
        }
    }

    fn tick(&mut self) {
        let web = web_center(&self.cfg.web.corner, &screen(), &self.cfg.web);
        self.mob
            .step(DT, &self.g, &screen(), Some(self.cursor), &self.cfg);
        let arrived = self.mob.errand.is_some_and(|e| {
            (e.arrived || e.unreachable) && self.sent.is_some_and(|g| g.distance(e.target) < 0.5)
        });
        let out = self.heist.update(
            HeistInput {
                dt: DT,
                hunter: Some(Hunter {
                    pos: self.mob.body_center,
                    arrived,
                }),
                cursor: Some(self.cursor),
                screen: screen(),
                web_center: web,
                idle_hint: None,
            },
            &self.cfg.web,
        );
        if self.heist.active() && !self.heist.is_trapped() {
            let (to, speed) = out.goal.unwrap_or((self.mob.walker.pos, 1.0));
            self.mob.send_to(to, speed);
            self.sent = out.goal.map(|g| g.0);
        } else {
            self.mob.end_errand();
            self.sent = None;
        }
        if let Some(w) = out.warp {
            self.cursor = w;
        }
        if self.heist.has_cursor() {
            self.max_rope = self
                .max_rope
                .max(self.cursor.distance(self.mob.body_center) - self.heist.rope());
        }
        self.events.extend(out.events);
    }
}

#[test]
fn real_spider_hooks_the_cursor_carries_it_around_and_traps_it_in_the_corner_web() {
    let mut s = Sim::new();
    let start = s.cursor;
    let mut carried_far = 0.0f32;
    for _ in 0..(180.0 / DT) as u32 {
        s.tick();
        if s.heist.has_cursor() {
            carried_far = carried_far.max(s.cursor.distance(start));
        }
        if s.heist.is_trapped() {
            break;
        }
    }
    assert_eq!(
        s.events,
        vec![HeistEvent::Started, HeistEvent::Hooked, HeistEvent::Trapped]
    );
    assert!(carried_far > 400.0, "passeou so {carried_far} px");
    // pendurado no fio o tempo todo (folga: o corpo anda ate ~12 px por passo)
    assert!(s.max_rope <= 20.0, "folga do fio: {}", s.max_rope);
    assert!(
        (s.heist.rope() - s.cfg.web.rope_px).abs() < 1.0,
        "fio recolhido"
    );
    let web = web_center(&s.cfg.web.corner, &screen(), &s.cfg.web);
    assert!(s.cursor.distance(web) < 1.0);
    assert!(s.mob.body_center.distance(web) < s.cfg.web.radius_px * 2.5);

    // preso: a aranha volta a vida normal, e 3 golpes de espada de ferro (4) rasgam a teia (12)
    s.tick();
    assert!(s.mob.errand.is_none());
    assert_eq!(s.heist.click(web, 4.0, web, &s.cfg.web), None);
    assert_eq!(s.heist.click(web, 4.0, web, &s.cfg.web), None);
    assert_eq!(
        s.heist.click(web, 4.0, web, &s.cfg.web),
        Some(HeistEvent::WebDestroyed)
    );
    assert!(!s.heist.active());
}

/// Icone da area de trabalho: elemento pequeno (id na faixa de elementos).
fn icon(n: u64, x: f32, y: f32) -> DesktopWindow {
    win(spiderdesk::platform::ELEMENT_ID_BASE + n, x, y, 89.0, 76.0)
}

#[test]
fn desktop_icons_are_walkable_even_though_they_are_smaller_than_a_window() {
    // uma janela de app de 89x76 seria ignorada; um icone do mesmo tamanho, nao
    let small_app = graph(&[win(5, 800.0, 500.0, 89.0, 76.0)], false);
    assert!(
        small_app
            .nearest_point(Vec2::new(840.0, 500.0), 3.0)
            .is_none()
    );

    let g = graph(&[icon(1, 800.0, 500.0)], false);
    let top = g.nearest_point(Vec2::new(840.0, 500.0), 3.0).unwrap();
    assert_eq!(
        g.edge(top.edge).owner,
        spiderdesk::navigation::window_graph::SurfaceOwner::Window(
            spiderdesk::platform::ELEMENT_ID_BASE + 1
        )
    );
}

#[test]
fn a_window_in_front_hides_the_icons_behind_it() {
    // lista da frente para o fundo: a janela cobre o icone inteiro
    let g = graph(
        &[win(1, 700.0, 400.0, 500.0, 400.0), icon(1, 800.0, 500.0)],
        true,
    );
    assert!(g.nearest_point(Vec2::new(840.0, 500.0), 3.0).is_none());
    assert!(g.threads.iter().all(|t| (t.a.x - 844.5).abs() > 1.0));
}

#[test]
fn neighbouring_icons_link_up_and_each_island_gets_one_thread_to_the_nearest_support() {
    // dois icones lado a lado perto do chao + um sozinho perto do teto
    let ws = [
        icon(1, 109.0, 917.0),
        icon(2, 210.0, 917.0),
        icon(3, 1200.0, 107.0),
    ];
    let g = graph(&ws, true);
    let on = |x: f32, y: f32| g.nearest_point(Vec2::new(x, y), 3.0).unwrap();
    // vizinhos de linha: da para ir de um ao outro sem fio
    assert!(plan_route(&g, on(150.0, 917.0), on(250.0, 917.0)).is_some());

    assert_eq!(g.threads.len(), 3, "um fio por icone: {:?}", g.threads);
    let thread_at = |x: f32| {
        *g.threads
            .iter()
            .find(|t| (t.a.x - x).abs() < 1.0)
            .unwrap_or_else(|| panic!("sem fio em x={x}"))
    };
    // perto do chao: desce (993 -> 1080); perto do teto: fica pendurado (0 -> 107)
    let low = thread_at(153.5);
    assert_eq!((low.a.y, low.b.y), (993.0, 1080.0));
    let high = thread_at(1244.5);
    assert_eq!((high.a.y, high.b.y), (0.0, 107.0));

    // e o mob chega do chao a qualquer um deles
    let floor = on(600.0, 1080.0);
    assert!(plan_route(&g, floor, on(150.0, 917.0)).is_some());
    assert!(plan_route(&g, floor, on(1240.0, 107.0)).is_some());
}

#[test]
fn elements_touching_the_screen_edge_need_no_thread() {
    // barra superior de ponta a ponta + janela encostada na esquerda
    let ws = [
        win(
            spiderdesk::platform::ELEMENT_ID_BASE + 9,
            0.0,
            0.0,
            1920.0,
            32.0,
        ),
        win(1, 0.0, 200.0, 600.0, 400.0),
    ];
    let g = graph(&ws, true);
    assert!(g.threads.is_empty(), "{:?}", g.threads);
    // a parte de baixo da barra e caminho
    assert!(g.nearest_point(Vec2::new(900.0, 32.0), 2.0).is_some());
}

#[test]
fn mobs_never_walk_over_a_system_bar() {
    // tela do usuario: barra superior de 29 px so na tela da esquerda
    let screen = Rect::from_xywh(0.0, 0.0, 3840.0, 1080.0);
    let work = [
        Rect::from_xywh(1920.0, 0.0, 1920.0, 1080.0),
        Rect::from_xywh(0.0, 29.0, 1920.0, 1051.0),
    ];
    let mut ws = spiderdesk::platform::panel_windows(screen, &work);
    // janela maximizada embaixo da barra
    ws.push(win(1, 0.0, 29.0, 1920.0, 1051.0));
    let g = PathGraph::build(screen, &ws, &BuildParams::default());
    for e in 0..g.edges.len() as u32 {
        let s = g.edge_segment(e);
        for t in [0.0, 0.5, 1.0] {
            let p = s.point_at(t);
            assert!(
                !(p.x < 1919.0 && p.y < 28.5),
                "aresta {e} passa por cima da barra: {s:?}"
            );
        }
    }
    // nenhuma superficie "olha" para dentro da barra (bicho ali seria desenhado por cima dela)
    let bar = Rect::from_xywh(0.0, 0.0, 1920.0, 29.0);
    for e in 0..g.edges.len() as u32 {
        let edge = g.edge(e);
        // (ligacoes curtas entre superficies nao sao chao/parede: so passagem)
        if edge.kind == spiderdesk::navigation::window_graph::EdgeKind::Link {
            continue;
        }
        let probe = g.edge_segment(e).point_at(0.5) + edge.normal * 3.0;
        assert!(
            !bar.contains(probe),
            "aresta {e} ({:?}) olha para dentro da barra",
            g.edge_segment(e)
        );
    }
    // o teto da tela da direita continua caminho, e liga com a base da barra
    let right = g.nearest_point(Vec2::new(2800.0, 0.0), 1.0).unwrap();
    let under_bar = g.nearest_point(Vec2::new(900.0, 29.0), 1.0).unwrap();
    assert!(plan_route(&g, right, under_bar).is_some());

    // teia: canto da area util, nao da tela
    let cfg = config();
    let area = spiderdesk::platform::corner_area("top_left", screen, &work);
    let web = web_center("top_left", &area, &cfg.web);
    assert!(web.y > 29.0 + cfg.web.radius_px * 0.5, "{web}");
}
