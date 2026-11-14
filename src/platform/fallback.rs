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

/// Cursor global: no Linux/X11 via `QueryPointer` na janela raiz; senao `None` (o chamador usa
/// o cursor da janela do Bevy).
pub fn cursor_position() -> Option<Vec2> {
    #[cfg(target_os = "linux")]
    {
        x11::cursor_position()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Click-through nativo do X11: so os retangulos dados (coordenadas locais da janela, px)
/// recebem o mouse; o resto atravessa para as janelas de baixo (extensao SHAPE, regiao INPUT).
/// Devolve `true` se aplicou (janela X11), `false` se a plataforma nao suporta (ex.: Wayland
/// nativo, onde o `hit_test` do winit funciona).
pub fn set_input_region(
    handle: &bevy::window::RawHandleWrapper,
    rects: &[(i32, i32, u32, u32)],
) -> bool {
    #[cfg(target_os = "linux")]
    {
        x11::set_input_region(handle, rects)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (handle, rects);
        false
    }
}

#[cfg(target_os = "linux")]
mod x11 {
    use std::sync::{Mutex, OnceLock};

    use bevy::math::Vec2;
    use raw_window_handle::RawWindowHandle;
    use x11rb::connection::Connection;
    use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
    use x11rb::protocol::xproto::{
        ClientMessageEvent, ClipOrdering, ConfigureWindowAux, ConnectionExt as _, EventMask,
        Rectangle, StackMode,
    };
    use x11rb::rust_connection::RustConnection;

    struct X {
        conn: RustConnection,
        root: u32,
        net_wm_state: u32,
        net_wm_state_above: u32,
    }

    /// Conexao propria com o servidor X (o X permite alterar janelas de outra conexao).
    fn x() -> &'static Mutex<Option<X>> {
        static X11: OnceLock<Mutex<Option<X>>> = OnceLock::new();
        X11.get_or_init(|| {
            let conn = std::env::var_os("DISPLAY")
                .and_then(|_| RustConnection::connect(None).ok())
                .map(|(conn, screen)| {
                    let root = conn.setup().roots.get(screen).map(|s| s.root).unwrap_or(0);
                    let atom = |name: &[u8]| {
                        conn.intern_atom(false, name)
                            .ok()
                            .and_then(|c| c.reply().ok())
                            .map(|r| r.atom)
                            .unwrap_or(0)
                    };
                    let net_wm_state = atom(b"_NET_WM_STATE");
                    let net_wm_state_above = atom(b"_NET_WM_STATE_ABOVE");
                    X {
                        conn,
                        root,
                        net_wm_state,
                        net_wm_state_above,
                    }
                });
            Mutex::new(conn)
        })
    }

    pub fn cursor_position() -> Option<Vec2> {
        let guard = x().lock().ok()?;
        let x = guard.as_ref()?;
        let reply = x.conn.query_pointer(x.root).ok()?.reply().ok()?;
        Some(Vec2::new(reply.root_x as f32, reply.root_y as f32))
    }

    fn window_id(handle: &bevy::window::RawHandleWrapper) -> Option<u32> {
        match handle.get_window_handle() {
            RawWindowHandle::Xlib(h) => Some(h.window as u32),
            RawWindowHandle::Xcb(h) => Some(h.window.get()),
            _ => None,
        }
    }

    pub fn keep_on_top(handle: &bevy::window::RawHandleWrapper) -> Result<(), String> {
        let Some(window) = window_id(handle) else {
            return Ok(()); // Wayland nativo: nao ha como
        };
        let guard = x().lock().map_err(|e| e.to_string())?;
        let Some(x) = guard.as_ref() else {
            return Ok(());
        };
        if x.net_wm_state == 0 || x.net_wm_state_above == 0 {
            return Err("atomos EWMH indisponiveis".into());
        }
        // _NET_WM_STATE_ADD = 1; fonte = aplicativo normal (1)
        let ev = ClientMessageEvent::new(
            32,
            window,
            x.net_wm_state,
            [1u32, x.net_wm_state_above, 0, 1, 0],
        );
        let mask = EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY;
        x.conn
            .send_event(false, x.root, mask, ev)
            .map_err(|e| e.to_string())?;
        x.conn
            .configure_window(
                window,
                &ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
            )
            .map_err(|e| e.to_string())?;
        x.conn.flush().map_err(|e| e.to_string())
    }

    pub fn set_input_region(
        handle: &bevy::window::RawHandleWrapper,
        rects: &[(i32, i32, u32, u32)],
    ) -> bool {
        let Some(window) = window_id(handle) else {
            return false; // Wayland nativo etc.
        };
        let Ok(guard) = x().lock() else { return false };
        let Some(x) = guard.as_ref() else {
            return false;
        };
        let rects: Vec<Rectangle> = rects
            .iter()
            .map(|&(rx, ry, w, h)| Rectangle {
                x: rx.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                y: ry.clamp(i16::MIN as i32, i16::MAX as i32) as i16,
                width: w.min(u16::MAX as u32) as u16,
                height: h.min(u16::MAX as u32) as u16,
            })
            .collect();
        let ok = x
            .conn
            .shape_rectangles(
                SO::SET,
                SK::INPUT,
                ClipOrdering::UNSORTED,
                window,
                0,
                0,
                &rects,
            )
            .is_ok()
            && x.conn.flush().is_ok();
        if !ok {
            bevy::log::warn!("X11: falha ao definir a regiao de entrada (extensao SHAPE)");
        }
        ok
    }
}

/// Linux: o Wayland nao deixa um app ficar "sempre no topo" (GNOME nao tem protocolo para isso),
/// entao, se houver XWayland (`DISPLAY`), forca o backend X11 do winit, onde o gerenciador de
/// janelas respeita `_NET_WM_STATE_ABOVE`. Para manter Wayland nativo: `SPIDERDESK_WAYLAND=1`.
pub fn prepare_graphics_env() {
    if cfg!(target_os = "linux")
        && std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var_os("DISPLAY").is_some()
        && std::env::var_os("SPIDERDESK_WAYLAND").is_none()
    {
        // SAFETY: chamado no inicio de `main`/`build_app`, antes de qualquer thread do app.
        unsafe { std::env::remove_var("WAYLAND_DISPLAY") };
        eprintln!(
            "spiderdesk: sessao Wayland detectada; usando XWayland (X11) para manter os mobs sempre \
             no topo (SPIDERDESK_WAYLAND=1 desativa)"
        );
    }
}

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

/// Linux/X11: pede ao gerenciador de janelas `_NET_WM_STATE_ABOVE` (EWMH) e eleva a janela.
/// Precisa ser repetido depois que a janela fica visivel: o winit manda o pedido quando ela ainda
/// esta oculta e o WM ignora pedidos de janelas nao mapeadas.
pub fn keep_on_top(handle: &bevy::window::RawHandleWrapper) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        x11::keep_on_top(handle)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = handle;
        Ok(())
    }
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
