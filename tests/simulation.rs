//! Simulacao sem janela/GPU: roda `SpiderSim::step` por minutos simulados e confere invariantes
//! (aranha sempre sobre uma superficie, sem NaN, ciclo morte->reaparecimento, fuga do cursor,
//! janelas mudando durante o percurso).

use bevy::math::Vec2;
use spiderdesk::config::Config;
use spiderdesk::creature::behavior::SpiderState;
use spiderdesk::creature::spider::SpiderSim;
use spiderdesk::navigation::PathGraph;
use spiderdesk::navigation::geometry::Rect;
use spiderdesk::navigation::window_graph::BuildParams;
use spiderdesk::platform::{DesktopWindow, WindowState};

const DT: f32 = 1.0 / 60.0;

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
    Rect::from_xywh(0.0, 0.0, 1920.0, 1079.0)
}

fn desktop_graph() -> PathGraph {
    let wins = [
        win(1, 150.0, 120.0, 1000.0, 700.0),
        win(2, 800.0, 300.0, 900.0, 650.0),
    ];
    PathGraph::build(screen(), &wins, &BuildParams::default())
}

fn fast_cfg() -> Config {
    let mut cfg = Config::default();
    cfg.behavior.idle_min_secs = 0.3;
    cfg.behavior.idle_max_secs = 0.8;
    cfg.behavior.respawn_delay_secs = 3.0;
    cfg.behavior.death_anim_secs = 1.0;
    cfg
}

#[test]
fn spider_stays_on_graph_surfaces_for_ten_simulated_minutes() {
    let cfg = fast_cfg();
    let graph = desktop_graph();
    let bounds = screen();
    let mut sim = SpiderSim::new(1234);

    let mut climbed = false;
    let mut walked = false;
    let mut idled = false;
    let mut travelled = 0.0;
    let mut last = Vec2::ZERO;

    for i in 0..(60 * 600) {
        sim.step(DT, &graph, &bounds, None, &cfg);
        let p = sim.walker.pos;
        assert!(p.is_finite(), "NaN na posicao (passo {i})");
        assert!(bounds.inflate(1.0).contains(p), "fora da tela: {p:?}");
        assert!(sim.orientation.is_finite() && sim.orientation.is_normalized());
        if sim.placed && !sim.walker.is_falling() {
            assert!(
                graph.nearest_point(p, 3.0).is_some(),
                "aranha fora de qualquer superficie em {p:?} (passo {i}, estado {:?})",
                sim.brain.state()
            );
        }
        match sim.brain.state() {
            SpiderState::Climbing => climbed = true,
            SpiderState::Walking => walked = true,
            SpiderState::Idle => idled = true,
            _ => {}
        }
        if i > 0 {
            travelled += p.distance(last);
        }
        last = p;
    }
    assert!(
        walked && idled && climbed,
        "walked={walked} idled={idled} climbed={climbed}"
    );
    assert!(travelled > 5_000.0, "andou pouco: {travelled}");
}

#[test]
fn speed_never_exceeds_configured_limits() {
    let cfg = fast_cfg();
    let graph = desktop_graph();
    let mut sim = SpiderSim::new(77);
    let cap = cfg.spider.run_speed * 2.5 + 1.0;
    for _ in 0..(60 * 120) {
        sim.step(DT, &graph, &screen(), None, &cfg);
        assert!(
            sim.speed_px >= 0.0 && sim.speed_px <= cap,
            "{}",
            sim.speed_px
        );
    }
}

#[test]
fn click_kills_spider_then_it_respawns_after_the_configured_delay() {
    let cfg = fast_cfg();
    let graph = desktop_graph();
    let mut sim = SpiderSim::new(5);
    // coloca
    while !sim.placed {
        sim.step(DT, &graph, &screen(), None, &cfg);
    }
    for _ in 0..30 {
        sim.step(DT, &graph, &screen(), None, &cfg);
    }
    assert!(sim.brain.state().is_alive_free());

    sim.clicked = true;
    let ev = sim.step(DT, &graph, &screen(), None, &cfg);
    assert!(ev.killed, "clique deve matar");
    assert_eq!(sim.brain.state(), SpiderState::Hit);

    let mut hidden_at = None;
    let mut respawn_at = None;
    let mut kills = 0;
    for i in 0..(60 * 10) {
        // cliques extras durante a morte nao podem pontuar de novo
        sim.clicked = i % 7 == 0;
        let ev = sim.step(DT, &graph, &screen(), None, &cfg);
        kills += ev.killed as u32;
        if hidden_at.is_none() && !sim.visible {
            hidden_at = Some(i as f32 * DT);
        }
        if ev.respawned {
            respawn_at = Some(i as f32 * DT);
            break;
        }
    }
    assert_eq!(kills, 0);
    let hidden = hidden_at.expect("deve ficar invisivel apos a animacao de morte");
    let respawn = respawn_at.expect("deve reaparecer");
    assert!((0.8..1.6).contains(&hidden), "sumiu em {hidden}");
    assert!((3.0..5.5).contains(&respawn), "reapareceu em {respawn}");
    assert!(sim.visible);
}

#[test]
fn approaching_cursor_makes_the_spider_flee_farther_away() {
    let mut cfg = fast_cfg();
    cfg.behavior.react_probability = 1.0;
    cfg.behavior.weight_flee = 1.0;
    cfg.behavior.weight_freeze = 0.0;
    cfg.behavior.weight_speedup = 0.0;
    cfg.behavior.weight_reroute = 0.0;
    let graph = desktop_graph();
    let mut sim = SpiderSim::new(42);
    while !sim.placed {
        sim.step(DT, &graph, &screen(), None, &cfg);
    }
    // espera ela parar de caminhar para uma situacao previsivel
    for _ in 0..600 {
        sim.step(
            DT,
            &graph,
            &screen(),
            Some(Vec2::new(-5000.0, -5000.0)),
            &cfg,
        );
    }
    // cursor aparece colado nela
    let cursor = sim.body_center + Vec2::new(30.0, -10.0);
    let start_dist = cursor.distance(sim.body_center);
    let mut fled = false;
    let mut max_dist = start_dist;
    for _ in 0..(60 * 3) {
        sim.step(DT, &graph, &screen(), Some(cursor), &cfg);
        if sim.brain.state() == SpiderState::Fleeing {
            fled = true;
        }
        max_dist = max_dist.max(cursor.distance(sim.body_center));
    }
    assert!(fled, "deveria ter entrado em Fleeing");
    assert!(
        max_dist > start_dist + 150.0,
        "afastou apenas {}",
        max_dist - start_dist
    );
}

#[test]
fn windows_changing_during_the_walk_never_break_the_spider() {
    use spiderdesk::navigation::walker::Reattach;
    let cfg = fast_cfg();
    let bounds = screen();
    let mut sim = SpiderSim::new(2024);

    let mut windows = vec![
        win(1, 150.0, 120.0, 1000.0, 700.0),
        win(2, 800.0, 300.0, 900.0, 650.0),
    ];
    let mut graph = PathGraph::build(bounds, &windows, &BuildParams::default());
    let mut falls = 0;

    for i in 0..(60 * 240) {
        // a cada 3 s o usuario "mexe" nas janelas
        if i % (60 * 3) == 0 && i > 0 {
            let old = windows.clone();
            let k = (i / 180) as f32;
            windows[0] = win(
                1,
                100.0 + (k * 97.0) % 600.0,
                80.0 + (k * 53.0) % 300.0,
                900.0 + (k * 31.0) % 200.0,
                650.0,
            );
            if (i / 180) % 5 == 0 {
                windows.pop(); // janela 2 minimizada
            } else if windows.len() < 2 {
                windows.push(win(2, 900.0, 350.0, 800.0, 600.0));
            }
            graph = PathGraph::build(bounds, &windows, &BuildParams::default());
            if sim.placed {
                sim.walker.follow_window(&old, &windows);
                if sim.walker.reattach(&graph, 24.0) == Reattach::Falling {
                    falls += 1;
                }
            }
        }
        sim.step(DT, &graph, &bounds, None, &cfg);
        assert!(sim.walker.pos.is_finite());
        assert!(
            bounds.inflate(1.0).contains(sim.walker.pos),
            "{:?}",
            sim.walker.pos
        );
    }
    assert!(sim.placed);
    // depois de assentar, volta a estar sobre uma superficie
    for _ in 0..600 {
        sim.step(DT, &graph, &bounds, None, &cfg);
    }
    assert!(sim.walker.is_falling() || graph.nearest_point(sim.walker.pos, 3.0).is_some());
    let _ = falls;
}

#[test]
fn identical_seed_gives_identical_run() {
    let cfg = fast_cfg();
    let graph = desktop_graph();
    let run = |seed| {
        let mut sim = SpiderSim::new(seed);
        for _ in 0..3000 {
            sim.step(DT, &graph, &screen(), None, &cfg);
        }
        sim.walker.pos
    };
    assert_eq!(run(9), run(9));
    assert_ne!(run(9), run(10));
}

/// Regressao: com a aranha oculta o app roda a ~4 Hz (dt = 0.25 s). O intervalo de respawn
/// configurado precisa valer em tempo REAL, nao em "passos de 0.1 s".
#[test]
fn respawn_delay_is_honored_at_low_update_rates() {
    let cfg = fast_cfg(); // death_anim 1.0 s + respawn_delay 3.0 s
    let graph = desktop_graph();
    for dt in [1.0 / 60.0, 0.25, 0.5] {
        let mut sim = SpiderSim::new(31);
        while !sim.placed {
            sim.step(dt, &graph, &screen(), None, &cfg);
        }
        sim.clicked = true;
        let ev = sim.step(dt, &graph, &screen(), None, &cfg);
        assert!(ev.killed);
        let mut t = 0.0;
        let mut respawn_at = None;
        while t < 20.0 {
            let ev = sim.step(dt, &graph, &screen(), None, &cfg);
            t += dt;
            if ev.respawned {
                respawn_at = Some(t);
                break;
            }
        }
        let r = respawn_at.unwrap_or_else(|| panic!("nao reapareceu (dt={dt})"));
        // alvo: death_anim + respawn_delay = 4.0 s (menos o flinch ja consumido), +- um passo
        assert!(
            (3.5..=4.0 + dt + 0.3).contains(&r),
            "dt={dt}: reapareceu em {r:.2}s"
        );
    }
}
