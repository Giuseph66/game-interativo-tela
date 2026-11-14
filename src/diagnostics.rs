//! Metricas de desempenho registradas em log (base para otimizacoes futuras).

use bevy::prelude::*;

use crate::config::Config;
use crate::creature::spider::SpiderSim;
use crate::desktop::Desktop;
use crate::overlay::Activity;

#[derive(Resource, Default)]
pub struct Metrics {
    since: f32,
    frames: u32,
    frame_ms_sum: f32,
    frame_ms_max: f32,
}

pub fn log_metrics(
    cfg: Res<Config>,
    time: Res<Time<Real>>,
    activity: Res<Activity>,
    desktop: Option<Res<Desktop>>,
    sim: Option<Res<SpiderSim>>,
    mut m: ResMut<Metrics>,
) {
    let dt_ms = time.delta_secs() * 1000.0;
    m.frames += 1;
    m.frame_ms_sum += dt_ms;
    m.frame_ms_max = m.frame_ms_max.max(dt_ms);

    let interval = cfg.diagnostics.metrics_interval_secs;
    if interval <= 0.0 {
        return;
    }
    let now = time.elapsed_secs();
    if now - m.since < interval {
        return;
    }
    let span = (now - m.since).max(0.001);
    let avg_ms = m.frame_ms_sum / m.frames.max(1) as f32;
    let mem = crate::platform::process_memory_mb()
        .map(|v| format!("{v:.0} MiB"))
        .unwrap_or_else(|| "n/d".into());
    let (win, nodes, edges, polls, rebuilds, poll_us, build_us) = desktop
        .as_ref()
        .map(|d| {
            (
                d.windows.len(),
                d.graph.nodes.len(),
                d.graph.edges.len(),
                d.stats.polls,
                d.stats.rebuilds,
                d.stats.max_poll_us,
                d.stats.max_build_us,
            )
        })
        .unwrap_or_default();
    let state = sim
        .as_ref()
        .map(|s| format!("{:?}", s.brain.state()))
        .unwrap_or_else(|| "-".into());
    info!(
        "metricas: {:.1} fps ({} quadros), frame medio {:.2} ms / max {:.2} ms, atividade {:?}, aranha {state}, \
         janelas {win}, grafo {nodes} nos/{edges} arestas, consultas {polls} (max {poll_us} us), \
         reconstrucoes {rebuilds} (max {build_us} us), memoria {mem}",
        m.frames as f32 / span,
        m.frames,
        avg_ms,
        m.frame_ms_max,
        *activity,
    );
    m.since = now;
    m.frames = 0;
    m.frame_ms_sum = 0.0;
    m.frame_ms_max = 0.0;
}
