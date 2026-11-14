//! Estado do desktop em tempo de execucao: lista de janelas (consultada periodicamente, nunca a
//! cada frame) e o grafo de caminhos, reconstruido somente quando as janelas mudam.

use std::time::Instant;

use bevy::prelude::*;

use crate::config::Config;
use crate::creature::spider::SpiderSim;
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
    last_poll: Option<f32>,
    pub stats: DesktopStats,
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
            last_poll: None,
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
    }
}

/// Consulta as janelas a cada `window_poll_ms` e, se algo mudou (movida, redimensionada,
/// minimizada, maximizada, aberta, fechada ou reordenada), reconstroi o grafo e reconcilia a aranha.
pub fn poll_desktop(
    cfg: Res<Config>,
    geo: Res<OverlayGeometry>,
    time: Res<Time<Real>>,
    mut desktop: ResMut<Desktop>,
    mut sim: ResMut<SpiderSim>,
) {
    if !geo.ready {
        return;
    }
    let now = time.elapsed_secs();
    let period = cfg.navigation.window_poll_ms as f32 / 1000.0;
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
    let fresh: Vec<DesktopWindow> = platform::list_windows(desktop_rect, platform::current_pid())
        .iter()
        .map(|w| to_local(w, origin))
        .filter(|w| w.rect.intersects(&screen))
        .collect();
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

    debug!(
        "grafo v{}: {} janelas, {} nos, {} arestas ({} us)",
        desktop.version,
        desktop.windows.len(),
        desktop.graph.nodes.len(),
        desktop.graph.edges.len(),
        build_us
    );

    // a aranha viaja com a janela em que esta e e reconciliada com o grafo novo
    if sim.placed {
        let new_windows = desktop.windows.clone();
        sim.walker.follow_window(&old, &new_windows);
        match sim.walker.reattach(&desktop.graph, 24.0) {
            Reattach::Kept => {}
            Reattach::Snapped(_) => debug!("rota invalidada; aranha reposicionada"),
            Reattach::Falling => debug!("superficie sumiu; aranha em queda"),
            Reattach::Lost => debug!("grafo sem superficies"),
        }
    }
}
