//! Integracao Win32. Unico modulo do projeto com `unsafe`; cada bloco documenta sua garantia.
//!
//! Nenhuma API aqui exige privilegios de administrador. Le geometria e estado das janelas de
//! topo e, so enquanto o heroi pede, a imagem das 3 janelas da frente (`captures`), que fica em
//! memoria e vira apenas uma lista de apoios.

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
    SWP_NOSIZE, SWP_NOZORDER, SetCursorPos, SetWindowLongPtrW, SetWindowPos, WM_HOTKEY,
    WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
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

/// Move o cursor real para `global` (px fisicos do desktop virtual).
pub fn set_cursor_pos(global: Vec2) -> bool {
    // SAFETY: chamada sem ponteiros; coordenadas fora da tela sao ajustadas pelo sistema.
    unsafe { SetCursorPos(global.x.round() as i32, global.y.round() as i32) }.is_ok()
}

/// O Windows sempre permite mover o cursor.
pub fn can_move_cursor() -> bool {
    true
}

/// Liga/desliga as imagens das janelas (o heroi andando por dentro dos apps).
pub fn request_window_images(on: bool) {
    captures::request(on);
}

/// Apoios vistos por dentro da janela `id` (da ultima imagem analisada), se houver.
pub fn window_ledges(id: u64) -> Option<crate::game::hero::WindowLedges> {
    captures::ledges(id)
}

/// Muda sempre que os apoios de alguma janela mudam (ou somem).
pub fn window_ledges_version() -> u64 {
    captures::version()
}

/// Na saida: para de copiar janelas (a thread fica ociosa ate o processo terminar).
pub fn shutdown() {
    captures::request(false);
}

/// Imagens das janelas para o heroi andar por dentro dos apps.
///
/// Uma thread copia as `FRONT_WINDOWS` janelas de aplicativo da frente com
/// `PrintWindow(PW_RENDERFULLCONTENT)` (funciona com Chrome, VS Code e outros apps desenhados
/// pela GPU, Windows 8.1+), recorta a parte visivel (limites do DWM, sem a borda invisivel),
/// converte para brilho e procura os apoios (`navigation::vision`). Tudo fica em memoria: nenhuma
/// imagem e gravada em disco e nada sai do computador. So roda enquanto o heroi pede.
mod captures {
    use std::collections::HashMap;
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HGDIOBJ, ReleaseDC, SelectObject,
    };
    use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
    use windows::Win32::UI::WindowsAndMessaging::PW_RENDERFULLCONTENT;

    use crate::game::hero::WindowLedges;
    use crate::navigation::geometry::Rect;
    use crate::navigation::vision::{VisionParams, find_ledges, luma_from_bgra_crop};
    use crate::platform::DesktopWindow;

    /// Quantas janelas da frente sao copiadas (as de tras quase nao aparecem).
    const FRONT_WINDOWS: usize = 3;
    /// Intervalo entre rodadas de copia.
    const PERIOD: Duration = Duration::from_millis(1000);
    /// Janelas maiores que isso (em qualquer lado) sao ignoradas.
    const MAX_SIDE: i32 = 8192;
    /// Menor recorte que vale analisar.
    const MIN_SIDE: i32 = 16;

    static REQUESTED: AtomicBool = AtomicBool::new(false);
    static VERSION: AtomicU64 = AtomicU64::new(0);

    fn cache() -> &'static Mutex<HashMap<u64, WindowLedges>> {
        static CACHE: OnceLock<Mutex<HashMap<u64, WindowLedges>>> = OnceLock::new();
        CACHE.get_or_init(Default::default)
    }

    pub fn version() -> u64 {
        VERSION.load(Ordering::Relaxed)
    }

    pub fn ledges(id: u64) -> Option<WindowLedges> {
        cache().lock().ok()?.get(&id).cloned()
    }

    pub fn request(on: bool) {
        REQUESTED.store(on, Ordering::Relaxed);
        static STARTED: OnceLock<()> = OnceLock::new();
        if on {
            STARTED.get_or_init(|| {
                let spawned = std::thread::Builder::new()
                    .name("spiderdesk-vision".into())
                    .spawn(|| {
                        loop {
                            if REQUESTED.load(Ordering::Relaxed) {
                                refresh();
                            } else {
                                clear();
                            }
                            std::thread::sleep(PERIOD);
                        }
                    });
                if let Err(e) = spawned {
                    bevy::log::warn!("imagens das janelas indisponiveis: {e}");
                }
            });
        }
    }

    fn clear() {
        if let Ok(mut c) = cache().lock()
            && !c.is_empty()
        {
            c.clear();
            VERSION.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn refresh() {
        let windows =
            super::list_windows(Rect::from_xywh(0.0, 0.0, 0.0, 0.0), super::current_pid());
        let front: Vec<&DesktopWindow> = windows.iter().take(FRONT_WINDOWS).collect();
        // copia e analisa fora do cadeado (dezenas de ms por janela)
        let fresh: Vec<(u64, WindowLedges)> = front
            .iter()
            .filter_map(|w| Some((w.id, capture(w)?)))
            .collect();
        let Ok(mut c) = cache().lock() else { return };
        let before = c.len();
        c.retain(|id, _| front.iter().any(|w| w.id == *id));
        let mut changed = c.len() != before;
        for (id, seen) in fresh {
            if c.get(&id) != Some(&seen) {
                c.insert(id, seen);
                changed = true;
            }
        }
        if changed {
            VERSION.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Apoios da parte visivel da janela `w` (coordenadas do recorte = `w.rect`).
    fn capture(w: &DesktopWindow) -> Option<WindowLedges> {
        let raw = w.raw_rect;
        let (rw, rh) = (raw.width().round() as i32, raw.height().round() as i32);
        if !(MIN_SIDE..=MAX_SIDE).contains(&rw) || !(MIN_SIDE..=MAX_SIDE).contains(&rh) {
            return None;
        }
        // parte visivel (limites do DWM) dentro do retangulo bruto, que tem bordas invisiveis
        let cx = ((w.rect.min.x - raw.min.x).round() as i32).clamp(0, rw - 1);
        let cy = ((w.rect.min.y - raw.min.y).round() as i32).clamp(0, rh - 1);
        let cw = (w.rect.width().round() as i32).min(rw - cx);
        let ch = (w.rect.height().round() as i32).min(rh - cy);
        if cw < MIN_SIDE || ch < MIN_SIDE {
            return None;
        }
        let bgra = print_window(HWND(w.id as usize as *mut c_void), rw, rh)?;
        let crop = (cx as usize, cy as usize, cw as usize, ch as usize);
        let luma = luma_from_bgra_crop(&bgra, rw as usize, crop);
        let (cw, ch) = (cw as usize, ch as usize);
        if luma.len() != cw * ch {
            return None;
        }
        Some(WindowLedges {
            ledges: find_ledges(&luma, cw, ch, &VisionParams::default()),
            image_size: (cw as u32, ch as u32),
        })
    }

    /// Copia a janela inteira (`w` x `h`, retangulo de `GetWindowRect`) como BGRA de cima para
    /// baixo. `None` se o app nao deixar ou a janela sumir no meio.
    fn print_window(hwnd: HWND, w: i32, h: i32) -> Option<Vec<u8>> {
        // SAFETY: todos os objetos GDI sao criados e destruidos aqui, nesta thread; o bitmap e
        // tirado do DC de memoria antes do `GetDIBits` (exigencia da API) e o buffer tem
        // exatamente `w * h * 4` bytes para as `h` linhas de 32 bits pedidas. Um HWND que deixou
        // de existir so faz o `PrintWindow` falhar.
        unsafe {
            let screen = GetDC(None);
            if screen.is_invalid() {
                return None;
            }
            let mem = CreateCompatibleDC(Some(screen));
            let bmp = CreateCompatibleBitmap(screen, w, h);
            let mut out = None;
            if !mem.is_invalid() && !bmp.is_invalid() {
                let old = SelectObject(mem, HGDIOBJ(bmp.0));
                let ok = PrintWindow(hwnd, mem, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)).as_bool();
                SelectObject(mem, old);
                if ok {
                    let mut info = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: w,
                            biHeight: -h, // negativo = linhas de cima para baixo
                            biPlanes: 1,
                            biBitCount: 32,
                            biCompression: BI_RGB.0,
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    let mut buf = vec![0u8; w as usize * h as usize * 4];
                    let lines = GetDIBits(
                        screen,
                        bmp,
                        0,
                        h as u32,
                        Some(buf.as_mut_ptr().cast()),
                        &mut info,
                        DIB_RGB_COLORS,
                    );
                    if lines == h {
                        out = Some(buf);
                    }
                }
            }
            if !bmp.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(bmp.0));
            }
            if !mem.is_invalid() {
                let _ = DeleteDC(mem);
            }
            ReleaseDC(None, screen);
            out
        }
    }
}

/// Area util de cada monitor (sem a barra de tarefas), px fisicos do desktop virtual.
pub fn work_areas(_screen: Rect) -> Vec<Rect> {
    use windows::Win32::Graphics::Gdi::{
        EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
    };

    unsafe extern "system" fn each(
        monitor: HMONITOR,
        _hdc: HDC,
        _clip: *mut RECT,
        data: LPARAM,
    ) -> windows::core::BOOL {
        // SAFETY: `data` e o `&mut Vec<Rect>` passado abaixo, vivo durante a enumeracao.
        let out = unsafe { &mut *(data.0 as *mut Vec<Rect>) };
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: `info` e valido e tem `cbSize` preenchido.
        if unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            let r = info.rcWork;
            out.push(Rect::new(
                Vec2::new(r.left as f32, r.top as f32),
                Vec2::new(r.right as f32, r.bottom as f32),
            ));
        }
        true.into()
    }

    let mut out: Vec<Rect> = Vec::new();
    // SAFETY: o callback so usa `data` como o vetor acima, que vive ate o fim da chamada.
    let _ = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(each),
            LPARAM(&mut out as *mut Vec<Rect> as isize),
        )
    };
    out
}

/// O Win32 ja lista as janelas reais.
pub fn helper_status() -> super::HelperStatus {
    super::HelperStatus::NotNeeded
}

/// Nada a instalar no Windows.
pub fn install_helper() -> Result<(), String> {
    Ok(())
}

/// `GetCursorPos` acompanha o ponteiro em qualquer lugar da tela.
pub fn pointer_tracking_live() -> bool {
    true
}

/// Nao e preciso no Windows: o movimento do cursor e lido diretamente.
pub fn input_idle_secs() -> Option<f32> {
    None
}

/// De onde vem a lista de janelas (para o log de inicio e `--list-windows`).
pub fn window_source() -> &'static str {
    "Win32 (EnumWindows)"
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

/// Canto superior esquerdo real da janela (pixels fisicos).
pub fn window_origin(handle: &bevy::window::RawHandleWrapper) -> Option<bevy::math::IVec2> {
    let hwnd = hwnd_of(handle).ok()?;
    let mut r = RECT::default();
    // SAFETY: hwnd e deste processo; `r` vive durante a chamada.
    unsafe { GetWindowRect(hwnd, &mut r) }.ok()?;
    Some(bevy::math::IVec2::new(r.left, r.top))
}

/// Move a janela sem mudar tamanho, ordem Z nem ativacao.
pub fn move_window(handle: &bevy::window::RawHandleWrapper, pos: bevy::math::IVec2) -> bool {
    let Ok(hwnd) = hwnd_of(handle) else {
        return false;
    };
    // SAFETY: hwnd e deste processo; flags preservam tamanho/ordem/foco.
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            pos.x,
            pos.y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
    }
    .is_ok()
}

/// No Windows o click-through usa `hit_test` (winit/`WS_EX_TRANSPARENT`): nada a fazer aqui.
pub fn set_input_region(
    _handle: &bevy::window::RawHandleWrapper,
    _rects: &[(i32, i32, u32, u32)],
) -> bool {
    false
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
