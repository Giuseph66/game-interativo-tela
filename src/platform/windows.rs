//! Integracao Win32. Unico modulo do projeto com `unsafe`; cada bloco documenta sua garantia.
//!
//! Nenhuma API aqui exige privilegios de administrador. Nada le o conteudo dos aplicativos:
//! apenas geometria e estado das janelas de topo.

use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bevy::math::Vec2;
use raw_window_handle::RawWindowHandle;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::System::Threading::GetCurrentProcessId;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
    RegisterHotKey, UnregisterHotKey,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GWL_EXSTYLE, GetClassNameW, GetCursorPos, GetWindow, GetWindowLongPtrW,
    GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, HWND_TOPMOST, IsIconic,
    IsWindowVisible, IsZoomed, MSG, PM_REMOVE, PeekMessageW, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SetWindowLongPtrW, SetWindowPos, WM_HOTKEY, WS_EX_APPWINDOW, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW,
};
use windows::core::BOOL;

use super::{DesktopWindow, Hotkey, WindowState};
use crate::navigation::geometry::Rect;

/// Deve ser chamado antes de qualquer janela existir: coordenadas passam a ser pixels fisicos
/// (necessario com escala de 125%/150%). O winit tambem faz isso; repetir e inofensivo.
pub fn init_process() {
    // SAFETY: chamada sem ponteiros; falha (ja definido) e ignorada de proposito.
    let _ = unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
}

/// Antes de criar o renderizador: o wgpu/DX12 so suporta janela transparente com o swapchain
/// "DxgiFromVisual" (DirectComposition). O Bevy 0.20 le essa opcao apenas da variavel de ambiente.
pub fn prepare_graphics_env() {
    if std::env::var_os("WGPU_DX12_PRESENTATION_SYSTEM").is_none() {
        // SAFETY: chamado no inicio de `main`, antes de criar qualquer thread.
        unsafe { std::env::set_var("WGPU_DX12_PRESENTATION_SYSTEM", "Visual") };
    }
}

pub fn current_pid() -> u32 {
    // SAFETY: sem argumentos nem efeitos colaterais.
    unsafe { GetCurrentProcessId() }
}

/// Posicao global do cursor em pixels fisicos de tela.
pub fn cursor_position() -> Option<Vec2> {
    let mut p = POINT::default();
    // SAFETY: `p` e um POINT valido e vive durante a chamada.
    unsafe { GetCursorPos(&mut p) }.ok()?;
    Some(Vec2::new(p.x as f32, p.y as f32))
}

struct EnumCtx {
    own_pid: u32,
    out: Vec<DesktopWindow>,
}

/// Janelas de aplicativos visiveis, da frente para o fundo (ordem Z).
pub fn list_windows(_screen: Rect, own_pid: u32) -> Vec<DesktopWindow> {
    let mut ctx = EnumCtx {
        own_pid,
        out: Vec::with_capacity(32),
    };
    // SAFETY: `ctx` sobrevive a chamada (EnumWindows e sincrono) e o callback so a acessa por
    // este ponteiro, na mesma thread.
    let _ = unsafe { EnumWindows(Some(enum_proc), LPARAM(&mut ctx as *mut EnumCtx as isize)) };
    ctx.out
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: lparam e o `&mut EnumCtx` criado em `list_windows`, ainda vivo.
    let ctx = unsafe { &mut *(lparam.0 as *mut EnumCtx) };
    if let Some(w) = inspect_window(hwnd, ctx.own_pid) {
        ctx.out.push(w);
    }
    BOOL(1) // continua enumerando
}

fn inspect_window(hwnd: HWND, own_pid: u32) -> Option<DesktopWindow> {
    // SAFETY: todas as chamadas abaixo recebem um HWND fornecido pelo EnumWindows e buffers locais.
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return None;
        }

        // janelas "fantasma" (UWP em segundo plano, outro desktop virtual) ficam cloaked
        let mut cloaked: u32 = 0;
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut c_void,
            size_of::<u32>() as u32,
        )
        .is_ok()
            && cloaked != 0
        {
            return None;
        }

        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let is_tool = ex & WS_EX_TOOLWINDOW.0 != 0 && ex & WS_EX_APPWINDOW.0 == 0;
        if is_tool {
            return None;
        }
        // janelas com dono (dialogos, popups) nao sao superficies "principais"
        if GetWindow(hwnd, GW_OWNER).is_ok_and(|h| !h.0.is_null()) {
            return None;
        }

        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == own_pid {
            return None;
        }

        let mut class = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut class).max(0) as usize;
        let class = String::from_utf16_lossy(&class[..n.min(class.len())]);
        if matches!(
            class.as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            return None;
        }

        let mut title_buf = [0u16; 256];
        let n = GetWindowTextW(hwnd, &mut title_buf).max(0) as usize;
        if n == 0 {
            return None; // sem titulo: normalmente nao e uma janela de aplicativo
        }
        let title = String::from_utf16_lossy(&title_buf[..n.min(title_buf.len())]);

        let mut raw = RECT::default();
        GetWindowRect(hwnd, &mut raw).ok()?;
        let raw_rect = to_rect(raw);

        // limites visiveis (sem a sombra/borda invisivel do Windows 10/11)
        let mut frame = RECT::default();
        let rect = if DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut frame as *mut RECT as *mut c_void,
            size_of::<RECT>() as u32,
        )
        .is_ok()
        {
            to_rect(frame)
        } else {
            raw_rect
        };
        if rect.width() < 1.0 || rect.height() < 1.0 {
            return None;
        }

        Some(DesktopWindow {
            id: hwnd.0 as usize as u64,
            rect,
            raw_rect,
            state: if IsZoomed(hwnd).as_bool() {
                WindowState::Maximized
            } else {
                WindowState::Normal
            },
            pid,
            title,
        })
    }
}

fn to_rect(r: RECT) -> Rect {
    Rect::from_xywh(
        r.left as f32,
        r.top as f32,
        (r.right - r.left) as f32,
        (r.bottom - r.top) as f32,
    )
}

fn hwnd_of(handle: &bevy::window::RawHandleWrapper) -> Result<HWND, String> {
    match handle.get_window_handle() {
        RawWindowHandle::Win32(h) => Ok(HWND(h.hwnd.get() as *mut c_void)),
        other => Err(format!("handle de janela inesperado: {other:?}")),
    }
}

/// Estilos do overlay: nunca ativa/rouba foco e nao aparece no Alt+Tab.
/// (`WS_EX_TRANSPARENT`/`WS_EX_LAYERED` sao gerenciados pelo winit via `CursorOptions::hit_test`.)
pub fn configure_overlay_window(handle: &bevy::window::RawHandleWrapper) -> Result<(), String> {
    let hwnd = hwnd_of(handle)?;
    // SAFETY: hwnd pertence a este processo e esta viva (acabou de ser criada pelo winit).
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let wanted = ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize;
        if wanted != ex {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted);
        }
    }
    keep_on_top(handle)
}

/// O winit recalcula o ex-style INTEIRO a cada troca de `hit_test` (`SetWindowLongW(GWL_EXSTYLE,
/// ...)` a partir das suas proprias flags), apagando `NOACTIVATE`/`TOOLWINDOW`. Esta funcao
/// devolve os bits que faltarem (leitura-modificacao-escrita; preserva TRANSPARENT/LAYERED do
/// winit). Barata: roda todo quadro e se auto-corrige em <= 1 quadro.
///
/// Com `layered_alpha_fix`, quando o bit `WS_EX_LAYERED` aparece (o winit o liga ao ativar o
/// click-through, SEM nunca chamar `SetLayeredWindowAttributes`/`UpdateLayeredWindow`, e o Win32
/// documenta que uma janela layered assim pode nao ser exibida), aplica alfa constante 255, que
/// nao altera a aparencia. Desligado por padrao: e um remedio hipotetico, ainda nao testado.
pub fn ensure_overlay_styles(handle: &bevy::window::RawHandleWrapper, layered_alpha_fix: bool) {
    use std::sync::atomic::AtomicBool;
    use windows::Win32::Foundation::COLORREF;
    use windows::Win32::UI::WindowsAndMessaging::{
        LWA_ALPHA, SetLayeredWindowAttributes, WS_EX_LAYERED,
    };
    static LAYERED_SEEN: AtomicBool = AtomicBool::new(false);

    let Ok(hwnd) = hwnd_of(handle) else { return };
    // SAFETY: hwnd pertence a este processo; so mexe em bits de estilo estendido.
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let wanted = ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize;
        if wanted != ex {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted);
        }
        let layered = ex & WS_EX_LAYERED.0 as isize != 0;
        if layered_alpha_fix && layered && !LAYERED_SEEN.swap(true, Ordering::Relaxed) {
            if let Err(e) = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA) {
                bevy::log::warn!("SetLayeredWindowAttributes: {e}");
            }
        } else if !layered {
            LAYERED_SEEN.store(false, Ordering::Relaxed);
        }
    }
}

/// Reafirma o estado "sempre no topo" sem ativar a janela nem mexer em posicao/tamanho.
pub fn keep_on_top(handle: &bevy::window::RawHandleWrapper) -> Result<(), String> {
    let hwnd = hwnd_of(handle)?;
    // SAFETY: ver acima; flags garantem que nada alem da ordem Z muda.
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .map_err(|e| e.to_string())
    }
}

const KEY_DOWN_MASK: u16 = 0x8000;

/// Algum botao do mouse (esquerdo/direito) esta fisicamente pressionado?
pub fn mouse_button_down() -> Option<bool> {
    Some(key_down(0x01) || key_down(0x02)) // VK_LBUTTON, VK_RBUTTON
}

/// Memoria residente do processo, em MiB.
pub fn process_memory_mb() -> Option<f32> {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::GetCurrentProcess;
    let mut c = PROCESS_MEMORY_COUNTERS {
        cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        ..Default::default()
    };
    // SAFETY: `c` e valido durante a chamada e `cb` informa seu tamanho.
    unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) }.ok()?;
    Some(c.WorkingSetSize as f32 / (1024.0 * 1024.0))
}

fn key_down(vk: i32) -> bool {
    // SAFETY: consulta simples de estado de tecla.
    (unsafe { GetAsyncKeyState(vk) } as u16) & KEY_DOWN_MASK != 0
}

fn combo_down(hk: &Hotkey) -> bool {
    key_down(hk.vk_code() as i32)
        && (!hk.ctrl || key_down(0x11))
        && (!hk.alt || key_down(0x12))
        && (!hk.shift || key_down(0x10))
        && (!hk.win || key_down(0x5B) || key_down(0x5C))
}

/// Thread dedicada ao atalho de emergencia: `RegisterHotKey` (evento) + `GetAsyncKeyState`
/// (reserva, caso outro programa ja tenha registrado o mesmo atalho).
/// Independe do laco do jogo, da aranha e do click-through.
pub fn spawn_hotkey_thread(hotkey: Hotkey, requested: Arc<AtomicBool>) {
    let spawn = std::thread::Builder::new()
        .name("spiderdesk-hotkey".into())
        .spawn(move || {
            let mut mods = MOD_NOREPEAT;
            let mut add = |cond: bool, m: HOT_KEY_MODIFIERS| {
                if cond {
                    mods |= m;
                }
            };
            add(hotkey.ctrl, MOD_CONTROL);
            add(hotkey.alt, MOD_ALT);
            add(hotkey.shift, MOD_SHIFT);
            add(hotkey.win, MOD_WIN);

            let mut msg = MSG::default();
            // SAFETY: `msg` e local; a chamada inicial garante que a fila de mensagens da thread exista
            // antes do RegisterHotKey, e todas as chamadas ocorrem nesta mesma thread.
            let registered = unsafe {
                let _ = PeekMessageW(&mut msg, None, WM_HOTKEY, WM_HOTKEY, PM_REMOVE);
                RegisterHotKey(None, 0x5344, mods, hotkey.vk_code()).is_ok()
            };
            if !registered {
                bevy::log::warn!("RegisterHotKey falhou (atalho em uso?); usando somente polling");
            }

            while !requested.load(Ordering::Relaxed) {
                // SAFETY: ver acima.
                unsafe {
                    while PeekMessageW(&mut msg, None, WM_HOTKEY, WM_HOTKEY, PM_REMOVE).as_bool() {
                        if msg.message == WM_HOTKEY {
                            requested.store(true, Ordering::Relaxed);
                        }
                    }
                }
                if combo_down(&hotkey) {
                    requested.store(true, Ordering::Relaxed);
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            if registered {
                // SAFETY: desfaz o registro feito acima nesta thread.
                let _ = unsafe { UnregisterHotKey(None, 0x5344) };
            }
        });
    if let Err(e) = spawn {
        bevy::log::error!("nao foi possivel iniciar a thread do atalho de emergencia: {e}");
    }
}
