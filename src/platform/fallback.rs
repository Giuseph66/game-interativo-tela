//! Implementacao para SOs que nao sao Windows: existe apenas para desenvolvimento e testes.
//! Simula um desktop com algumas janelas para exercitar navegacao e comportamento.

use bevy::math::Vec2;

use super::{DesktopWindow, Hotkey, WindowState};
use crate::navigation::geometry::Rect;

/// Nada a configurar fora do Windows.
pub fn init_process() {}

/// Desktop simulado: tres janelas proporcionais a `screen`, da frente para o fundo.
/// As janelas "se mexem" levemente com o tempo para exercitar o recalculo de caminhos
/// quando `SPIDERDESK_FAKE_DRIFT=1`.
pub fn list_windows(screen: Rect, _own_pid: u32) -> Vec<DesktopWindow> {
    let (w, h) = (screen.width(), screen.height());
    let drift = if std::env::var_os("SPIDERDESK_FAKE_DRIFT").is_some() {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f32())
            .unwrap_or(0.0);
        (t * 0.5).sin() * 0.05 * w
    } else {
        0.0
    };
    let mk = |id: u64, x: f32, y: f32, ww: f32, hh: f32, title: &str| {
        let r = Rect::from_xywh(screen.min.x + x, screen.min.y + y, ww, hh);
        DesktopWindow {
            id,
            rect: r,
            raw_rect: r,
            state: WindowState::Normal,
            pid: 4242,
            title: title.to_string(),
        }
    };
    vec![
        mk(
            1,
            w * 0.50 + drift,
            h * 0.18,
            w * 0.40,
            h * 0.45,
            "Janela simulada A",
        ),
        mk(
            2,
            w * 0.10,
            h * 0.30,
            w * 0.45,
            h * 0.55,
            "Janela simulada B",
        ),
        mk(
            3,
            w * 0.62,
            h * 0.55,
            w * 0.30,
            h * 0.38,
            "Janela simulada C",
        ),
    ]
}

/// Cursor global nao e consultavel de forma portavel; o chamador usa o cursor da janela do Bevy.
pub fn cursor_position() -> Option<Vec2> {
    None
}

/// Nada a preparar fora do Windows.
pub fn prepare_graphics_env() {}

pub fn current_pid() -> u32 {
    std::process::id()
}

/// Sem Win32: nada a ajustar na janela.
pub fn configure_overlay_window(_handle: &bevy::window::RawHandleWrapper) -> Result<(), String> {
    Ok(())
}

/// Fora do Windows nao ha atalho global; use Ctrl+C no terminal (ou `--timeout`).
pub fn spawn_hotkey_thread(
    _hotkey: Hotkey,
    _requested: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    bevy::log::info!("atalho global indisponivel nesta plataforma; use Ctrl+C no terminal");
}

/// Sem Win32: nada a corrigir.
pub fn ensure_overlay_styles(_handle: &bevy::window::RawHandleWrapper, _layered_alpha_fix: bool) {}

/// Sem Win32: nada a reafirmar.
pub fn keep_on_top(_handle: &bevy::window::RawHandleWrapper) -> Result<(), String> {
    Ok(())
}

/// Sem estado fisico global: o chamador usa os eventos da janela.
pub fn mouse_button_down() -> Option<bool> {
    None
}

/// Memoria residente (MiB) via /proc no Linux.
pub fn process_memory_mb() -> Option<f32> {
    let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
    let pages: f32 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096.0 / (1024.0 * 1024.0))
}
