//! Ninhos + sacudida de janela, de ponta a ponta e sem GPU: a mesma logica que o app roda,
//! dirigida por um relogio simulado.

use bevy::math::Vec2;

use spiderdesk::config::Config;
use spiderdesk::creature::behavior::SpiderState;
use spiderdesk::creature::spider::SpiderSim;
use spiderdesk::game::{NestField, Progress};
use spiderdesk::navigation::geometry::Rect;
use spiderdesk::navigation::window_graph::{BuildParams, PathGraph, SurfaceOwner};
use spiderdesk::platform::{DesktopWindow, WindowState};

const DT: f32 = 0.05;

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

fn graph(windows: &[DesktopWindow]) -> PathGraph {
    PathGraph::build(screen(), windows, &BuildParams::default())
}

fn config() -> Config {
    let mut cfg = Config::default();
    cfg.behavior.seed = 7;
    cfg.behavior.react_probability = 0.0; // o cursor nao espanta ninguem nestes testes
    cfg.behavior.pause_chance_per_sec = 0.0;
    cfg
}

/// Mob nascido em `at` (como um filhote de ninho), ja posicionado.
fn mob_at(at: Vec2, g: &PathGraph, cfg: &Config, seed: u64) -> SpiderSim {
    let mut sim = SpiderSim::new(seed);
    sim.spawn_at = Some(at);
    let ev = sim.step(DT, g, &screen(), None, cfg);
    assert!(ev.placed);
    assert!(sim.walker.pos.distance(at) < 3.0, "{:?}", sim.walker.pos);
    sim
}

#[test]
fn shaken_mob_falls_off_the_window_gets_hurt_stunned_and_recovers() {
    let cfg = config();
    let ws = vec![win(1, 400.0, 200.0, 600.0, 300.0)];
    let g = graph(&ws);
    let mut sim = mob_at(Vec2::new(700.0, 200.0), &g, &cfg, 11);
    assert_eq!(sim.walker.owner, SurfaceOwner::Window(1));
    assert_eq!(sim.hp, cfg.combat.mob_hp);

    assert!(sim.shake_off());
    assert!(!sim.shake_off(), "ja em queda: nada a derrubar");
    let mut steps = 0;
    while sim.walker.is_falling() {
        let ev = sim.step(1.0 / 60.0, &g, &screen(), None, &cfg);
        assert!(!ev.killed);
        steps += 1;
        assert!(steps < 600, "nunca pousou");
    }
    // atravessou a propria janela e foi parar no chao da tela
    assert!(
        (sim.walker.pos.y - 1080.0).abs() < 1.0,
        "{:?}",
        sim.walker.pos
    );
    assert_eq!(sim.walker.owner, SurfaceOwner::Screen);
    assert_eq!(sim.hp, cfg.combat.mob_hp - cfg.nests.fall_damage);
    assert_eq!(sim.fall_hurt, Some(cfg.nests.fall_damage));
    assert!(sim.last_hit.is_some(), "barra de vida aparece");
    assert!(sim.stun_left > 0.0);

    // atordoado: nao sai do lugar ate o tempo acabar
    let landed = sim.walker.pos;
    let mut t = DT;
    while t < cfg.nests.stun_secs - 0.1 {
        sim.step(DT, &g, &screen(), None, &cfg);
        assert_eq!(sim.walker.pos, landed, "andou atordoado (t={t})");
        t += DT;
    }
    // depois volta a vida normal
    let mut walked = false;
    for _ in 0..(20.0 / DT) as usize {
        sim.step(DT, &g, &screen(), None, &cfg);
        walked |= sim.walker.pos.distance(landed) > 30.0;
    }
    assert!(walked, "nunca mais andou depois do atordoamento");
    assert!(sim.brain.state().is_alive_free());
}

#[test]
fn the_fall_kills_a_mob_that_was_already_nearly_dead() {
    let cfg = config();
    let ws = vec![win(1, 400.0, 200.0, 600.0, 300.0)];
    let g = graph(&ws);
    let mut sim = mob_at(Vec2::new(700.0, 200.0), &g, &cfg, 12);
    sim.hp = cfg.nests.fall_damage * 0.5;
    assert!(sim.shake_off());
    let mut killed = 0;
    for _ in 0..600 {
        killed += sim.step(1.0 / 60.0, &g, &screen(), None, &cfg).killed as u32;
    }
    assert_eq!(killed, 1, "morre no pouso, uma vez so");
    assert_eq!(sim.hp, 0.0);
    assert_eq!(sim.stun_left, 0.0);
    assert!(!sim.brain.state().is_alive_free());
}

#[test]
fn a_mob_on_the_floor_is_not_affected_by_a_window_shake() {
    let cfg = config();
    let ws = vec![win(1, 400.0, 200.0, 600.0, 300.0)];
    let g = graph(&ws);
    let sim = mob_at(Vec2::new(100.0, 1080.0), &g, &cfg, 13);
    // o jogo so derruba quem esta numa superficie da janela sacudida
    assert_ne!(sim.walker.owner, SurfaceOwner::Window(1));
}

/// Conta mordidas mantendo o cursor a `offset` px do corpo por `secs`.
fn bites(sim: &mut SpiderSim, g: &PathGraph, cfg: &Config, offset: f32, secs: f32) -> u32 {
    let mut n = 0;
    let mut t = 0.0;
    while t < secs {
        let cursor = sim.body_center + Vec2::new(offset, 0.0);
        sim.step(DT, g, &screen(), Some(cursor), cfg);
        n += std::mem::take(&mut sim.pending_bite) as u32;
        t += DT;
    }
    n
}

#[test]
fn only_an_elite_bites_and_only_a_cursor_that_lingers() {
    let cfg = config();
    let g = graph(&[]);
    let at = Vec2::new(900.0, 1080.0);

    let mut common = mob_at(at, &g, &cfg, 21);
    assert_eq!(
        bites(&mut common, &g, &cfg, 10.0, 12.0),
        0,
        "mob comum nao morde"
    );

    let mut elite = SpiderSim::new(22);
    elite.elite = true;
    elite.spawn_at = Some(at);
    elite.step(DT, &g, &screen(), None, &cfg);
    assert_eq!(
        elite.max_hp,
        cfg.combat.mob_hp * cfg.nests.elite_hp_multiplier
    );
    assert_eq!(elite.hp, elite.max_hp);

    // cursor longe: nada
    let far = cfg.nests.bite_radius_px * 3.0;
    assert_eq!(bites(&mut elite, &g, &cfg, far, 8.0), 0);
    // passada rapida (menos que o tempo de bote): nada
    assert_eq!(
        bites(&mut elite, &g, &cfg, 10.0, cfg.nests.bite_windup_secs * 0.6),
        0
    );
    assert_eq!(bites(&mut elite, &g, &cfg, far, 0.5), 0);
    // cursor parado em cima: morde no fim do bote e de novo so depois do intervalo
    let span = cfg.nests.bite_windup_secs + cfg.nests.bite_cooldown_secs * 0.9;
    assert_eq!(bites(&mut elite, &g, &cfg, 10.0, span), 1);
    assert_eq!(
        bites(&mut elite, &g, &cfg, 10.0, cfg.nests.bite_cooldown_secs),
        1
    );

    // a mordida rouba XP, mas nunca a espada
    let mut p = Progress::default();
    p.add_xp(cfg.combat.xp_iron + 2, &cfg.combat);
    let sword = p.sword;
    assert_eq!(p.lose_xp(cfg.nests.bite_xp, &cfg.combat), 2);
    assert_eq!(p.sword, sword);
}

#[test]
fn full_loop_calm_window_nest_hatchlings_then_a_shake() {
    let mut cfg = config();
    cfg.nests.calm_secs = 5.0;
    cfg.nests.build_chance_per_sec = 50.0; // constroi assim que puder
    cfg.nests.build_secs = 2.0;
    cfg.nests.hatch_interval_secs = 4.0;
    cfg.nests.elite_after_secs = 9.0;
    cfg.behavior.idle_min_secs = 3.0; // repousos longos: tempo de sobra para construir
    cfg.behavior.idle_max_secs = 3.0;
    let c = cfg.nests.clone();

    let ws = vec![win(1, 400.0, 300.0, 700.0, 400.0)];
    let g = graph(&ws);
    let mut field = NestField::new(3);
    field.observe_windows(&[], &ws, 0.0, &c);
    let mut builder = mob_at(Vec2::new(700.0, 300.0), &g, &cfg, 31);

    // --- a janela fica parada: um ninho aparece, mas so depois de `calm_secs`
    let mut now = 0.0;
    let mut built_at = None;
    let mut hatched: Vec<(f32, bool, Vec2)> = Vec::new();
    while now < 40.0 {
        now += DT;
        builder.step(DT, &g, &screen(), None, &cfg);
        if let SurfaceOwner::Window(w) = builder.walker.owner
            && builder.brain.state() == SpiderState::Idle
            && field
                .try_build(w, builder.walker.pos, builder.walker.normal, 4, now, DT, &c)
                .is_some()
        {
            assert!(built_at.is_none(), "max_per_window = 1");
            built_at = Some(now);
        }
        for h in field.tick(DT, &c, 10) {
            hatched.push((now, h.elite, h.at));
            assert_eq!(h.kind, 4, "filhote do mesmo tipo de quem construiu");
        }
    }
    let built_at = built_at.expect("nenhum ninho em 40 s numa janela parada");
    assert!(built_at >= c.calm_secs, "construiu cedo demais: {built_at}");
    assert_eq!(field.nests.len(), 1);
    let nest = field.nests[0].clone();
    assert_eq!(nest.window, 1);
    assert!(g.contains_point(nest.pos, 1.0), "ninho fora da borda");

    // --- chocou no ritmo certo; os primeiros sao comuns, os tardios sao elite
    assert!(hatched.len() >= 4, "{hatched:?}");
    let first = hatched[0].0 - built_at;
    assert!(
        (first - (c.build_secs + c.hatch_interval_secs)).abs() < 0.5,
        "1o filhote {first:.1} s depois do ninho"
    );
    assert!(!hatched[0].1);
    assert!(
        hatched.last().unwrap().1,
        "ninho velho deveria chocar elite"
    );
    assert!(hatched.iter().all(|h| h.2 == nest.pos));

    // --- o filhote nasce no ninho, na janela
    let mut chick = mob_at(nest.pos, &g, &cfg, 32);
    assert_eq!(chick.walker.owner, SurfaceOwner::Window(1));

    // --- o jogador sacode a janela (3 Hz, +-80 px, amostrado a 50 ms)
    let mut prev = ws.clone();
    let t0 = now;
    for k in 1..=30 {
        let t = k as f32 * 0.05;
        let dx = (t * 3.0 * std::f32::consts::TAU).sin() * 80.0;
        let cur = vec![win(1, 400.0 + dx, 300.0, 700.0, 400.0)];
        field.observe_windows(&prev, &cur, t0 + t, &c);
        prev = cur;
    }
    assert_eq!(field.take_shakes(), vec![1]);

    // mobs da janela caem; o ninho racha mas sobrevive; a janela deixa de estar calma
    for sim in [&mut builder, &mut chick] {
        if sim.walker.owner == SurfaceOwner::Window(1) {
            assert!(sim.shake_off());
        }
    }
    assert!(chick.walker.is_falling());
    assert_eq!(field.apply_shake(1, &c, cfg.combat.health_bar_secs), 1);
    let cracked = &field.nests[0];
    assert_eq!(cracked.hp, c.nest_hp - c.shake_nest_damage);
    assert!(
        (cracked.pos.x - nest.pos.x).abs() <= 81.0 && cracked.pos.y == nest.pos.y,
        "o ninho viajou com a janela: {:?} -> {:?}",
        nest.pos,
        cracked.pos
    );
    let end = t0 + 1.5;
    assert!(field.calm_for(1, end).unwrap() < 1.0);
    assert!(!field.can_build(1, Vec2::new(900.0, 300.0), end, &c));

    // --- e a espada termina o servico, com o bonus de XP
    let id = cracked.id;
    let mut p = Progress::default();
    let mut hits = 0;
    while let Some(s) = field.strike(id, &mut p, &cfg.combat, &c) {
        hits += 1;
        if s.destroyed {
            assert!(s.strike.xp_gained >= c.xp_per_nest);
        }
    }
    let untouched = (c.nest_hp / cfg.combat.damage_wood).ceil() as u32;
    assert!(
        hits > 0 && hits < untouched,
        "sacudir poupou golpes: {hits} < {untouched}"
    );
    assert!(field.nests.is_empty());
    assert!(p.xp >= c.xp_per_nest);
}
