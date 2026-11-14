//! Cenarios de desktop de ponta a ponta (sem janela/GPU): janelas movendo, minimizando,
//! maximizando, Alt+Tab (mudanca de ordem Z) e escalas de DPI.

use bevy::math::Vec2;
use spiderdesk::navigation::geometry::Rect;
use spiderdesk::navigation::walker::{Reattach, Walker};
use spiderdesk::navigation::window_graph::{BuildParams, SurfaceOwner};
use spiderdesk::navigation::{PathGraph, plan_route};
use spiderdesk::platform::{DesktopWindow, WindowState, windows_equivalent};

fn win(id: u64, x: f32, y: f32, w: f32, h: f32) -> DesktopWindow {
    let r = Rect::from_xywh(x, y, w, h);
    DesktopWindow {
        id,
        rect: r,
        raw_rect: r,
        state: WindowState::Normal,
        pid: 7,
        title: format!("app{id}"),
    }
}

fn maximized(id: u64, screen: Rect, taskbar: f32) -> DesktopWindow {
    let r = Rect::from_xywh(0.0, 0.0, screen.width(), screen.height() - taskbar);
    DesktopWindow {
        id,
        rect: r,
        raw_rect: r,
        state: WindowState::Maximized,
        pid: 7,
        title: "max".into(),
    }
}

fn screen(w: f32, h: f32) -> Rect {
    Rect::from_xywh(0.0, 0.0, w, h)
}

fn walker_on_window_top(g: &PathGraph, win_id: u64) -> Walker {
    let idx = (0..g.edges.len() as u32)
        .find(|&e| {
            g.edge(e).owner == SurfaceOwner::Window(win_id)
                && !g.edge(e).is_vertical()
                && g.edge(e).normal.y < 0.0
        })
        .expect("topo da janela");
    let p = g.point_on(idx, 0.5);
    let mut w = Walker::new(p.pos);
    w.place(p, g);
    w
}

// 1) Chrome / VS Code: duas janelas grandes sobrepostas, aranha anda por cima das duas.
#[test]
fn spider_can_reach_both_overlapping_app_windows() {
    let s = screen(1920.0, 1080.0);
    let chrome = win(1, 100.0, 80.0, 1100.0, 800.0);
    let vscode = win(2, 700.0, 200.0, 1100.0, 760.0);
    let g = PathGraph::build(s, &[chrome, vscode], &BuildParams::default());
    let owners = g.owners();
    assert!(owners.contains(&SurfaceOwner::Window(1)));
    assert!(owners.contains(&SurfaceOwner::Window(2)));
    // do topo do chrome ate o topo do vscode existe rota
    let a = g.nearest_point(Vec2::new(300.0, 80.0), 3.0).unwrap();
    let b = g.nearest_point(Vec2::new(1500.0, 200.0), 3.0).unwrap();
    assert!(plan_route(&g, a, b).is_some());
}

// 5) Mover janela => grafo diferente (recalculo), rota antiga invalida, aranha acompanha.
#[test]
fn moving_a_window_triggers_rebuild_and_spider_rides_along() {
    let s = screen(1920.0, 1080.0);
    let before = vec![win(1, 300.0, 300.0, 700.0, 500.0)];
    let after = vec![win(1, 500.0, 420.0, 700.0, 500.0)];
    assert!(
        !windows_equivalent(&before, &after),
        "movimento deve ser detectado"
    );

    let g1 = PathGraph::build(s, &before, &BuildParams::default());
    let g2 = PathGraph::build(s, &after, &BuildParams::default());
    let mut w = walker_on_window_top(&g1, 1);
    let top_before = w.pos.y;
    assert!((top_before - 300.0).abs() < 1.0);

    w.follow_window(&before, &after);
    match w.reattach(&g2, 24.0) {
        Reattach::Snapped(p) => assert!(
            (p.pos.y - 420.0).abs() < 1.0,
            "aranha deve ficar no topo novo: {p:?}"
        ),
        other => panic!("esperava Snapped, veio {other:?}"),
    }
    assert!(
        w.pos.x > 500.0 && w.pos.x < 1200.0,
        "x acompanhou a janela: {}",
        w.pos.x
    );
}

// 6) Janela minimizada some da enumeracao => trajetorias invalidas removidas.
#[test]
fn minimized_window_removes_its_paths() {
    let s = screen(1920.0, 1080.0);
    let with = PathGraph::build(
        s,
        &[win(1, 300.0, 300.0, 700.0, 500.0)],
        &BuildParams::default(),
    );
    let without = PathGraph::build(s, &[], &BuildParams::default());
    assert!(with.owners().contains(&SurfaceOwner::Window(1)));
    assert!(!without.owners().contains(&SurfaceOwner::Window(1)));
    assert!(without.edges.len() < with.edges.len());

    // spider em cima da janela que sumiu cai ate o chao
    let mut w = walker_on_window_top(&with, 1);
    assert_eq!(w.reattach(&without, 24.0), Reattach::Falling);
    let mut landed = false;
    for _ in 0..600 {
        if w.update_fall(1.0 / 60.0, &without).is_some() {
            landed = true;
            break;
        }
    }
    assert!(landed);
    assert!((w.pos.y - 1080.0).abs() < 1.0);
}

// 7) Janela maximizada: bordas coincidentes com a tela viram o perimetro; so a base sobra.
#[test]
fn maximized_window_is_handled() {
    let s = screen(1920.0, 1080.0);
    let g = PathGraph::build(s, &[maximized(1, s, 40.0)], &BuildParams::default());
    let own: Vec<_> = g
        .edges
        .iter()
        .filter(|e| e.owner == SurfaceOwner::Window(1))
        .collect();
    assert_eq!(
        own.len(),
        1,
        "apenas a base da janela (acima da barra de tarefas)"
    );
    // ainda e possivel ir do chao (tela) ate a base da janela maximizada
    let floor = g.nearest_point(Vec2::new(500.0, 1080.0), 2.0).unwrap();
    let base = g.nearest_point(Vec2::new(900.0, 1040.0), 2.0).unwrap();
    assert!(plan_route(&g, floor, base).is_some());
}

// 8) DPI: o mesmo desktop em 100%, 125% e 150% gera grafos topologicamente identicos
//    (coordenadas fisicas escalam, limiares escalam via Config::scaled).
#[test]
fn graph_topology_is_dpi_invariant() {
    use spiderdesk::config::Config;
    let base_cfg = Config::default();
    let logical = [(100.0, 80.0, 800.0, 600.0), (600.0, 300.0, 700.0, 500.0)];
    let mut counts = Vec::new();
    for dpi in [1.0_f32, 1.25, 1.5] {
        let cfg = base_cfg.scaled(dpi);
        let s = screen(1920.0 * dpi, 1080.0 * dpi);
        let wins: Vec<DesktopWindow> = logical
            .iter()
            .enumerate()
            .map(|(i, &(x, y, w, h))| win(i as u64 + 1, x * dpi, y * dpi, w * dpi, h * dpi))
            .collect();
        let params = BuildParams {
            link_distance: cfg.navigation.link_distance_px,
            min_segment: cfg.navigation.min_segment_px,
            min_window_size: cfg.navigation.min_window_size_px,
            screen_edges: true,
            silk_threads: false,
        };
        let g = PathGraph::build(s, &wins, &params);
        counts.push((g.nodes.len(), g.edges.len()));
    }
    assert!(counts.windows(2).all(|w| w[0] == w[1]), "{counts:?}");
}

// 9) Alt+Tab: a janela de tras vai para a frente; trechos antes cobertos passam a ser visiveis.
#[test]
fn alt_tab_reorders_occlusion() {
    let s = screen(1920.0, 1080.0);
    let a = win(1, 200.0, 200.0, 800.0, 600.0);
    let b = win(2, 600.0, 100.0, 800.0, 600.0); // cobre parte da lateral direita de `a`... e o topo de `a`
    let a_front = PathGraph::build(s, &[a.clone(), b.clone()], &BuildParams::default());
    let b_front = PathGraph::build(s, &[b.clone(), a.clone()], &BuildParams::default());
    assert!(
        !windows_equivalent(&[a.clone(), b.clone()], &[b.clone(), a.clone()]),
        "mudanca de ordem Z e detectada"
    );

    // com `a` na frente, o topo de `a` (y=200) fica livre em x=700; com `b` na frente, fica coberto
    assert!(
        a_front
            .nearest_point(Vec2::new(700.0, 200.0), 1.5)
            .is_some()
    );
    assert!(
        b_front
            .nearest_point(Vec2::new(700.0, 200.0), 1.5)
            .is_none()
    );
}

// Janela arrastada em pequenos passos: graficos sempre consistentes (sem indices invalidos).
#[test]
fn dragging_a_window_never_produces_invalid_graphs() {
    let s = screen(1920.0, 1080.0);
    let other = win(9, 1000.0, 400.0, 600.0, 500.0);
    for step in 0..120 {
        let x = -300.0 + step as f32 * 20.0;
        let y = 50.0 + (step as f32 * 0.2).sin() * 150.0;
        let g = PathGraph::build(
            s,
            &[win(1, x, y, 700.0, 500.0), other.clone()],
            &BuildParams::default(),
        );
        for e in &g.edges {
            assert!((e.a as usize) < g.nodes.len() && (e.b as usize) < g.nodes.len());
            assert!(e.length.is_finite() && e.length > 0.0);
            assert!(e.cost >= e.length);
        }
        for n in &g.nodes {
            assert!(n.is_finite());
        }
        assert!(!g.is_empty());
    }
}
