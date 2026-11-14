//! Estado do desktop em tempo de execucao: lista de janelas (consultada periodicamente, nunca a
//! cada frame) e o grafo de caminhos, reconstruido somente quando as janelas mudam.

use std::time::Instant;

use bevy::prelude::*;

use crate::config::Config;
use crate::creature::spider::Mob;
use crate::game::NestField;
use crate::navigation::geometry::Rect;
use crate::navigation::walker::Reattach;
use crate::navigation::window_graph::{BuildParams, PathGraph};
use crate::overlay::OverlayGeometry;
use crate::platform::{self, DesktopWindow};

#[derive(Resource)]
pub struct Desktop {
    /// Janelas em coordenadas LOCAIS do overlay, da frente para o fundo.
    pub windows: Vec<DesktopWindow>,
    pub graph: PathGraph,
    /// Incrementa a cada reconstrucao do grafo.
    pub version: u64,
    pub screen: Rect,
    /// Area util de cada tela (sem barras do sistema), px locais. Vazia = a tela inteira.
    pub work_areas: Vec<Rect>,
    last_poll: Option<f32>,
    /// Ate quando considerar que ha janela em movimento (consulta rapida p/ detectar sacudida).
    moving_until: Option<f32>,
    pub stats: DesktopStats,
}

/// Depois do ultimo movimento visto, a consulta rapida continua por este tempo (s).
const MOVING_HOLD_SECS: f32 = 0.6;

impl Desktop {
    /// Alguma janela se moveu ha pouco (o usuario pode estar arrastando/sacudindo)?
    pub fn window_moving(&self, now: f32) -> bool {
        self.moving_until.is_some_and(|t| now < t)
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DesktopStats {
    pub polls: u64,
    pub rebuilds: u64,
    pub last_poll_us: u128,
    pub max_poll_us: u128,
    pub last_build_us: u128,
    pub max_build_us: u128,
}

impl Default for Desktop {
    fn default() -> Self {
        Self {
            windows: Vec::new(),
            graph: PathGraph::default(),
            version: 0,
            screen: Rect::from_xywh(0.0, 0.0, 1280.0, 720.0),
            work_areas: Vec::new(),
            last_poll: None,
            moving_until: None,
            stats: DesktopStats::default(),
        }
    }
}

/// Deslocamento de uma janela para coordenadas locais do overlay.
fn to_local(w: &DesktopWindow, origin: Vec2) -> DesktopWindow {
    let shift = |r: Rect| Rect::new(r.min - origin, r.max - origin);
    DesktopWindow {
        rect: shift(w.rect),
        raw_rect: shift(w.raw_rect),
        ..w.clone()
    }
}

pub fn build_params(cfg: &Config) -> BuildParams {
    BuildParams {
        link_distance: cfg.navigation.link_distance_px,
        min_segment: cfg.navigation.min_segment_px,
        min_window_size: cfg.navigation.min_window_size_px,
        screen_edges: cfg.navigation.screen_edges,
        silk_threads: cfg.navigation.silk_threads,
    }
}

/// Consulta as janelas a cada `window_poll_ms` e, se algo mudou (movida, redimensionada,
/// minimizada, maximizada, aberta, fechada ou reordenada), reconstroi o grafo e reconcilia a aranha.
pub fn poll_desktop(
    cfg: Res<Config>,
    geo: Res<OverlayGeometry>,
    time: Res<Time<Real>>,
    mut desktop: ResMut<Desktop>,
    mut nests: ResMut<NestField>,
    mut mobs: Query<&mut Mob>,
) {
    if !geo.ready {
        return;
    }
    let now = time.elapsed_secs();
    // janela em movimento: consulta mais rapida, senao uma sacudida passa entre duas amostras
    let poll_ms = if cfg.nests.enabled && desktop.window_moving(now) {
        cfg.nests.shake_poll_ms.min(cfg.navigation.window_poll_ms)
    } else {
        cfg.navigation.window_poll_ms
    };
    let period = poll_ms as f32 / 1000.0;
    if desktop.last_poll.is_some_and(|t| now - t < period) {
        return;
    }
    desktop.last_poll = Some(now);

    let screen = geo.local_rect();
    let t0 = Instant::now();
    let desktop_rect = Rect::from_xywh(
        geo.origin.x as f32,
        geo.origin.y as f32,
        geo.size.x as f32,
        geo.size.y as f32,
    );
    let origin = geo.origin.as_vec2();
    // barras do sistema (fora da area util) entram na frente de tudo: ninguem anda sobre elas
    let work: Vec<Rect> = platform::work_areas(desktop_rect)
        .into_iter()
        .map(|r| Rect::new(r.min - origin, r.max - origin))
        .collect();
    let mut fresh = platform::panel_windows(screen, &work);
    fresh.extend(
        platform::list_windows(desktop_rect, platform::current_pid())
            .iter()
            .map(|w| to_local(w, origin))
            .filter(|w| w.rect.intersects(&screen)),
    );
    desktop.work_areas = work;
    let poll_us = t0.elapsed().as_micros();
    desktop.stats.polls += 1;
    desktop.stats.last_poll_us = poll_us;
    desktop.stats.max_poll_us = desktop.stats.max_poll_us.max(poll_us);

    let first = desktop.version == 0;
    if !first && desktop.screen == screen && platform::windows_equivalent(&desktop.windows, &fresh)
    {
        return; // nada mudou: nenhum recalculo
    }

    let t1 = Instant::now();
    let graph = PathGraph::build(screen, &fresh, &build_params(&cfg));
    let build_us = t1.elapsed().as_micros();

    let old = std::mem::replace(&mut desktop.windows, fresh);
    desktop.graph = graph;
    desktop.screen = screen;
    desktop.version += 1;
    desktop.stats.rebuilds += 1;
    desktop.stats.last_build_us = build_us;
    desktop.stats.max_build_us = desktop.stats.max_build_us.max(build_us);

    // calma/sacudida de cada janela; os ninhos viajam com a janela deles
    if cfg.nests.enabled && nests.observe_windows(&old, &desktop.windows, now, &cfg.nests) {
        desktop.moving_until = Some(now + MOVING_HOLD_SECS);
    }

    debug!(
        "grafo v{}: {} janelas, {} nos, {} arestas ({} us)",
        desktop.version,
        desktop.windows.len(),
        desktop.graph.nodes.len(),
        desktop.graph.edges.len(),
        build_us
    );

    // cada mob viaja com a janela em que esta e e reconciliado com o grafo novo
    for mut mob in &mut mobs {
        if !mob.sim.placed {
            continue;
        }
        mob.sim.walker.follow_window(&old, &desktop.windows);
        match mob.sim.walker.reattach(&desktop.graph, 24.0) {
            Reattach::Kept => {}
            Reattach::Snapped(_) => debug!("rota invalidada; mob reposicionado"),
            Reattach::Falling => debug!("superficie sumiu; mob em queda"),
            Reattach::Lost => debug!("grafo sem superficies"),
        }
    }
}
