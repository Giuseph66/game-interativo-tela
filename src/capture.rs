//! `--screenshot <arquivo.png>`: salva o quadro renderizado (RGBA, com alfa) e imprime estatisticas
//! de transparencia. Serve para verificar, sem depender do compositor do sistema, que o fundo
//! realmente sai com alfa 0 e que a aranha/cubo foi desenhada. Depois de salvar, o app encerra.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured};

use crate::creature::spider::SpiderSim;
use crate::overlay::{OverlayGeometry, RunOptions};

#[derive(Default)]
pub struct CaptureState {
    ready_at: Option<f32>,
    requested: bool,
}

pub fn capture_if_requested(
    mut commands: Commands,
    opts: Res<RunOptions>,
    geo: Res<OverlayGeometry>,
    sim: Option<Res<SpiderSim>>,
    time: Res<Time<Real>>,
    mut state: Local<CaptureState>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = opts.screenshot.clone() else {
        return;
    };
    let now = time.elapsed_secs();
    let scene_ready = geo.ready && (opts.poc || sim.as_ref().is_some_and(|s| s.placed));
    if state.ready_at.is_none() {
        if scene_ready {
            state.ready_at = Some(now);
        }
        return;
    }
    let waited = now - state.ready_at.unwrap_or(now);
    if !state.requested && waited >= 2.5 {
        state.requested = true;
        commands
            .spawn(Screenshot::primary_window())
            .observe(move |cap: On<ScreenshotCaptured>| save_with_stats(&cap.image, &path));
        if let Some(s) = &sim {
            info!(
                "estado da aranha na captura: {:?}, direcao {:.2},{:.2}, normal {:.2},{:.2}, velocidade {:.0} px/s",
                s.brain.state(),
                s.walker.heading.x,
                s.walker.heading.y,
                s.walker.normal.x,
                s.walker.normal.y,
                s.speed_px
            );
        }
    }
    if waited >= 6.0 {
        exit.write(AppExit::Success);
    }
}

fn save_with_stats(image: &Image, path: &PathBuf) {
    let dynamic = match image.clone().try_into_dynamic() {
        Ok(d) => d,
        Err(e) => {
            error!("screenshot: formato nao suportado ({e})");
            return;
        }
    };
    let rgba = dynamic.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let (mut clear, mut opaque, mut partial) = (0u64, 0u64, 0u64);
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (w, h, 0u32, 0u32);
    for (x, y, p) in rgba.enumerate_pixels() {
        match p.0[3] {
            0 => clear += 1,
            255 => opaque += 1,
            _ => partial += 1,
        }
        if p.0[3] != 0 {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    let total = (w as u64 * h as u64).max(1);
    let pct = |n: u64| 100.0 * n as f64 / total as f64;
    info!(
        "screenshot {w}x{h} formato {:?}: alfa=0 em {:.3}% , alfa=255 em {:.3}% , parcial {:.3}% ; \
         area desenhada: ({min_x},{min_y})-({max_x},{max_y})",
        image.texture_descriptor.format,
        pct(clear),
        pct(opaque),
        pct(partial)
    );

    if let Err(e) = rgba.save(path) {
        error!(
            "screenshot: nao foi possivel salvar {}: {e}",
            path.display()
        );
        return;
    }
    // versao composta sobre cinza (o RGB do alfa-premultiplicado ja vem multiplicado)
    let mut comp = rgba.clone();
    for p in comp.pixels_mut() {
        let a = p.0[3] as f32 / 255.0;
        for c in 0..3 {
            let v = p.0[c] as f32 + (1.0 - a) * 120.0;
            p.0[c] = v.clamp(0.0, 255.0) as u8;
        }
        p.0[3] = 255;
    }
    let comp_path = path.with_extension("composite.png");
    match comp.save(&comp_path) {
        Ok(()) => info!(
            "screenshot salvo em {} (e {} sobre fundo cinza)",
            path.display(),
            comp_path.display()
        ),
        Err(e) => error!("screenshot composto: {e}"),
    }
}
