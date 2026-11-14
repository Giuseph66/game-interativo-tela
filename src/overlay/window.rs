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

/// Dimensiona o overlay para cobrir o monitor principal (menos `bottom_inset_px` de altura),
/// confirma que o tamanho foi aplicado e so entao mostra a janela.
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

    let inset = cfg.overlay.bottom_inset_px;
    let target = UVec2::new(
        mon.physical_width.max(64),
        mon.physical_height.saturating_sub(inset).max(64),
    );
    let origin = mon.physical_position;

    // monitor mudou de resolucao/posicao (troca de monitor, docking): refaz
    if geo.ready && (geo.size != target || geo.origin != origin) {
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
    geo.size = target;
    geo.monitor_scale = mon.scale_factor as f32;

    if window.position != WindowPosition::At(origin) {
        window.position = WindowPosition::At(origin);
    }
    if window.resolution.physical_size() != target {
        window
            .resolution
            .set_physical_resolution(target.x, target.y);
        geo.stable_frames = 0;
        return;
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
        "overlay pronto: {}x{} em ({}, {}), escala do monitor {:.2}",
        target.x, target.y, origin.x, origin.y, geo.monitor_scale
    );
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
    fn local_rect_matches_size() {
        let g = OverlayGeometry {
            size: UVec2::new(800, 600),
            ..Default::default()
        };
        let r = g.local_rect();
        assert_eq!((r.width(), r.height()), (800.0, 600.0));
    }
}
