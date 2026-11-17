//! Criacao e dimensionamento da janela do overlay.

use bevy::prelude::*;
use bevy::window::{
    CompositeAlphaMode, ExitCondition, Monitor, PresentMode, PrimaryMonitor, PrimaryWindow,
    RawHandleWrapper, WindowLevel, WindowPlugin, WindowPosition, WindowResolution,
};

use crate::config::Config;
use crate::navigation::geometry::Rect;

/// Geometria final do overlay sobre o desktop, em pixels FISICOS.
#[derive(Resource, Debug, Clone)]
pub struct OverlayGeometry {
    pub ready: bool,
    /// Canto superior esquerdo no desktop virtual.
    pub origin: IVec2,
    pub size: UVec2,
    pub monitor_scale: f32,
    /// Origem do monitor escolhido (pedida). `origin` e onde a janela REALMENTE ficou: com
    /// varias telas o gerenciador de janelas pode coloca-la em outro monitor.
    requested_origin: IVec2,
    /// Tamanho pedido (o aplicado pode diferir se o sistema arredondar/limitar).
    requested_size: UVec2,
    /// Monitor principal (origem, tamanho) em pixels fisicos do desktop virtual. O painel fica nele.
    pub primary: Option<(IVec2, UVec2)>,
    attempts: u32,
    stable_frames: u32,
}

impl Default for OverlayGeometry {
    fn default() -> Self {
        Self {
            ready: false,
            origin: IVec2::ZERO,
            size: UVec2::new(1280, 720),
            monitor_scale: 1.0,
            requested_origin: IVec2::ZERO,
            requested_size: UVec2::ZERO,
            primary: None,
            attempts: 0,
            stable_frames: 0,
        }
    }
}

impl OverlayGeometry {
    /// Retangulo do overlay em coordenadas locais (origem no canto da janela).
    pub fn local_rect(&self) -> Rect {
        Rect::from_xywh(0.0, 0.0, self.size.x as f32, self.size.y as f32)
    }

    pub fn size_f32(&self) -> Vec2 {
        self.size.as_vec2()
    }

    /// Posicao de tela local (Y para baixo) -> mundo do jogo (centro da janela, Y para cima).
    pub fn local_to_world(&self, p: Vec2) -> Vec2 {
        let s = self.size_f32();
        Vec2::new(p.x - s.x * 0.5, s.y * 0.5 - p.y)
    }

    /// Inverso de [`local_to_world`](Self::local_to_world).
    pub fn world_to_local(&self, p: Vec2) -> Vec2 {
        let s = self.size_f32();
        Vec2::new(p.x + s.x * 0.5, s.y * 0.5 - p.y)
    }
}

/// Retangulo que envolve todos os monitores (origem, tamanho), em pixels fisicos do desktop
/// virtual. `None` sem monitores.
pub fn desktop_bounds(monitors: &[(IVec2, UVec2)]) -> Option<(IVec2, UVec2)> {
    let (first, rest) = monitors.split_first()?;
    let (mut lo, mut hi) = (first.0, first.0 + first.1.as_ivec2());
    for (o, s) in rest {
        lo = lo.min(*o);
        hi = hi.max(*o + s.as_ivec2());
    }
    Some((lo, (hi - lo).max(IVec2::ONE).as_uvec2()))
}

/// Plugin de janela configurado para o overlay.
pub fn window_plugin(cfg: &Config) -> WindowPlugin {
    // No Windows, `Auto` escolheria Opaque; com DX12 + DirectComposition (swapchain "Visual") o
    // modo pre-multiplicado e suportado. Vulkan no Windows geralmente NAO oferece alfa e um modo
    // nao suportado derruba a criacao da superficie, entao nesse caso fica em `Auto` (sem
    // transparencia, mas sem travar). Em Linux (compositor) usa PreMultiplied.
    let alpha = if cfg!(target_os = "macos") {
        CompositeAlphaMode::PostMultiplied
    } else if cfg!(windows) && cfg.overlay.backend == crate::config::BackendChoice::Vulkan {
        CompositeAlphaMode::Auto
    } else {
        CompositeAlphaMode::PreMultiplied
    };

    WindowPlugin {
        primary_window: Some(Window {
            title: "SpiderDesk Overlay".into(),
            transparent: true,
            decorations: false,
            resizable: false,
            focused: false,
            // so aparece depois de dimensionada e configurada (evita "piscar" em tela cheia)
            visible: false,
            skip_taskbar: true,
            window_level: WindowLevel::AlwaysOnTop,
            composite_alpha_mode: alpha,
            present_mode: PresentMode::AutoVsync,
            position: WindowPosition::At(IVec2::ZERO),
            resolution: WindowResolution::new(640, 360),
            ..default()
        }),
        // nasce em click-through total: o desktop continua utilizavel mesmo se algo falhar
        primary_cursor_options: Some(bevy::window::CursorOptions {
            hit_test: false,
            ..default()
        }),
        exit_condition: ExitCondition::OnPrimaryClosed,
        close_when_requested: true,
    }
}

/// Dimensiona o overlay para cobrir todos os monitores (`overlay.all_monitors`) ou so o
/// principal (menos `bottom_inset_px` de altura), confirma que o tamanho foi aplicado e so entao mostra a janela.
pub fn fit_overlay_window(
    cfg: Res<Config>,
    mut geo: ResMut<OverlayGeometry>,
    monitors: Query<(&Monitor, Has<PrimaryMonitor>)>,
    mut windows: Query<(&mut Window, Option<&RawHandleWrapper>), With<PrimaryWindow>>,
) {
    let Ok((mut window, handle)) = windows.single_mut() else {
        return;
    };

    // monitor alvo: o principal, ou o primeiro disponivel
    let chosen = monitors
        .iter()
        .max_by_key(|(_, primary)| *primary)
        .map(|(m, _)| m);

    let Some(mon) = chosen else {
        // sem info de monitor ainda: espera alguns frames e usa o tamanho atual da janela
        geo.attempts += 1;
        if geo.attempts < 120 || geo.ready {
            return;
        }
        warn!("nenhum monitor detectado; usando o tamanho atual da janela");
        geo.size = window.resolution.physical_size();
        geo.ready = true;
        window.visible = true;
        return;
    };

    let primary = (
        mon.physical_position,
        UVec2::new(mon.physical_width, mon.physical_height),
    );
    let (origin, full) = if cfg.overlay.all_monitors {
        let all: Vec<(IVec2, UVec2)> = monitors
            .iter()
            .map(|(m, _)| {
                (
                    m.physical_position,
                    UVec2::new(m.physical_width, m.physical_height),
                )
            })
            .collect();
        desktop_bounds(&all).unwrap_or(primary)
    } else {
        primary
    };
    let inset = cfg.overlay.bottom_inset_px;
    let target = UVec2::new(full.x.max(64), full.y.saturating_sub(inset).max(64));

    // monitor mudou de resolucao/posicao (troca de monitor, docking): refaz
    if geo.ready && (geo.requested_size != target || geo.requested_origin != origin) {
        info!(
            "monitor alterado; reajustando overlay para {}x{}",
            target.x, target.y
        );
        geo.ready = false;
        geo.attempts = 0;
        geo.stable_frames = 0;
    }
    if geo.ready {
        return;
    }

    geo.attempts += 1;
    geo.origin = origin;
    geo.requested_origin = origin;
    geo.requested_size = target;
    geo.size = target;
    geo.monitor_scale = mon.scale_factor as f32;
    geo.primary = Some(primary);

    if window.position != WindowPosition::At(origin) {
        window.position = WindowPosition::At(origin);
    }
    if geo.attempts == 1 {
        info!(
            "monitores: {}; alvo do overlay {}x{} em ({}, {}), escala {:.2}; janela agora {}x{}",
            monitors.iter().count(),
            target.x,
            target.y,
            origin.x,
            origin.y,
            mon.scale_factor,
            window.resolution.physical_width(),
            window.resolution.physical_height()
        );
    }
    let actual = window.resolution.physical_size();
    if actual != target {
        // o sistema pode arredondar/limitar o tamanho (escala de DPI, janela maior que a tela):
        // depois de varias tentativas, aceita o que foi aplicado em vez de esperar para sempre
        if geo.attempts < 90 {
            window
                .resolution
                .set_physical_resolution(target.x, target.y);
            geo.stable_frames = 0;
            return;
        }
        warn!(
            "o sistema nao aplicou {}x{} (ficou {}x{}); seguindo com o tamanho atual",
            target.x, target.y, actual.x, actual.y
        );
        geo.size = actual;
    }

    // tamanho confere; espera alguns frames para o sistema aplicar de fato
    geo.stable_frames += 1;
    if geo.stable_frames < 3 && geo.attempts < 60 {
        return;
    }

    if let Some(h) = handle
        && let Err(e) = crate::platform::configure_overlay_window(h)
    {
        warn!("configure_overlay_window: {e}");
    }
    window.visible = true;
    geo.ready = true;
    info!(
        "overlay pronto: {}x{} em ({}, {}), {} monitor(es), escala {:.2}",
        target.x,
        target.y,
        origin.x,
        origin.y,
        if cfg.overlay.all_monitors {
            monitors.iter().count()
        } else {
            1
        },
        geo.monitor_scale
    );
}

/// Le onde a janela do overlay realmente esta e corrige `origin` (cursor, janelas e painel usam
/// essa origem). Necessario com mais de um monitor: o GNOME/X11 pode ignorar a posicao pedida.
pub fn track_overlay_origin(
    time: Res<Time<Real>>,
    mut geo: ResMut<OverlayGeometry>,
    windows: Query<&RawHandleWrapper, With<PrimaryWindow>>,
    mut last_check: Local<f32>,
    mut move_attempts: Local<u32>,
) {
    if !geo.ready || time.elapsed_secs() - *last_check < 0.5 {
        return;
    }
    *last_check = time.elapsed_secs();
    let Ok(handle) = windows.single() else { return };
    let Some(actual) = crate::platform::window_origin(handle) else {
        return;
    };
    // primeiro tenta devolver a janela ao monitor escolhido (algumas vezes); se o sistema
    // recusar, passa a usar a posicao real
    if actual != geo.requested_origin && *move_attempts < 3 {
        *move_attempts += 1;
        if crate::platform::move_window(handle, geo.requested_origin) {
            debug!(
                "overlay fora do monitor pedido; pedindo para mover (tentativa {})",
                *move_attempts
            );
            return;
        }
    }
    if actual != geo.origin {
        info!(
            "overlay esta em ({}, {}), nao em ({}, {}) (varios monitores?); ajustando coordenadas",
            actual.x, actual.y, geo.origin.x, geo.origin.y
        );
        geo.origin = actual;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_world_mapping_roundtrip_and_orientation() {
        let g = OverlayGeometry {
            size: UVec2::new(1920, 1079),
            ..Default::default()
        };
        // centro da tela = origem do mundo
        assert_eq!(g.local_to_world(Vec2::new(960.0, 539.5)), Vec2::ZERO);
        // canto superior esquerdo = (-w/2, +h/2): Y do mundo cresce para cima
        assert_eq!(g.local_to_world(Vec2::ZERO), Vec2::new(-960.0, 539.5));
        for p in [
            Vec2::new(10.0, 20.0),
            Vec2::new(1919.0, 1078.0),
            Vec2::new(333.3, 77.7),
        ] {
            let back = g.world_to_local(g.local_to_world(p));
            assert!((back - p).length() < 1e-3);
        }
    }

    #[test]
    fn desktop_bounds_wraps_all_monitors() {
        assert_eq!(desktop_bounds(&[]), None);
        let a = (IVec2::ZERO, UVec2::new(1920, 1080));
        assert_eq!(desktop_bounds(&[a]), Some(a));
        // lado a lado (como eDP-1 + HDMI-1)
        let b = (IVec2::new(1920, 0), UVec2::new(1920, 1080));
        assert_eq!(
            desktop_bounds(&[b, a]),
            Some((IVec2::ZERO, UVec2::new(3840, 1080)))
        );
        // monitor a esquerda (coordenada negativa) e mais alto
        let c = (IVec2::new(-1280, -200), UVec2::new(1280, 1440));
        assert_eq!(
            desktop_bounds(&[a, c]),
            Some((IVec2::new(-1280, -200), UVec2::new(3200, 1440)))
        );
    }

    #[test]
    fn local_rect_matches_size() {
        let g = OverlayGeometry {
            size: UVec2::new(800, 600),
            ..Default::default()
        };
        let r = g.local_rect();
        assert_eq!((r.width(), r.height()), (800.0, 600.0));
    }
}
