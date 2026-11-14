//! Camada de plataforma: tudo que depende do sistema operacional fica aqui.
//!
//! * `windows.rs` (somente Windows): Win32 real, unico lugar com `unsafe`.
//! * `fallback.rs` (demais SOs): desktop simulado, permite desenvolver e testar fora do Windows.
//!
//! O resto do programa so enxerga os tipos e funcoes reexportados neste modulo.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::navigation::geometry::Rect;

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(not(windows))]
mod fallback;
#[cfg(not(windows))]
pub use self::fallback::*;

/// Estado de uma janela de topo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    Normal,
    Maximized,
}

/// Janela de aplicativo descoberta no desktop, em pixels fisicos de tela.
#[derive(Debug, Clone, PartialEq)]
pub struct DesktopWindow {
    /// HWND (ou identificador equivalente).
    pub id: u64,
    /// Limites visiveis (`DWMWA_EXTENDED_FRAME_BOUNDS`; `GetWindowRect` como reserva).
    pub rect: Rect,
    /// Retangulo bruto de `GetWindowRect` (inclui bordas invisiveis).
    pub raw_rect: Rect,
    pub state: WindowState,
    pub pid: u32,
    pub title: String,
}

/// Area do monitor em pixels fisicos.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MonitorArea {
    pub rect: Rect,
    /// Fator de escala informado pelo sistema (1.0, 1.25, 1.5 ...).
    pub scale_factor: f32,
}

/// Compara dois conjuntos de janelas ignorando ruido de 1 px e titulos.
/// Ordem importa (e a ordem Z).
pub fn windows_equivalent(a: &[DesktopWindow], b: &[DesktopWindow]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| x.id == y.id && x.state == y.state && x.rect.approx_eq(&y.rect, 1.0))
}

/// Tecla principal de um atalho global.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyKey {
    /// 'A'..='Z' ou '0'..='9' (maiusculo).
    Char(char),
    /// F1..=F12.
    Function(u8),
    Escape,
    Pause,
}

/// Atalho global de encerramento de emergencia, ex.: `ctrl+alt+shift+q`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub key: HotkeyKey,
}

/// "q" -> 'Q'; "7" -> '7'; qualquer outra coisa -> `None`.
fn single_alphanumeric(s: &str) -> Option<char> {
    let mut it = s.chars();
    let c = it.next()?;
    (it.next().is_none() && c.is_ascii_alphanumeric()).then(|| c.to_ascii_uppercase())
}

/// "f5" -> 5 (somente F1..=F12).
fn function_key(s: &str) -> Option<u8> {
    s.strip_prefix('f')?
        .parse::<u8>()
        .ok()
        .filter(|n| (1..=12).contains(n))
}

impl Hotkey {
    /// Interpreta "ctrl+alt+shift+q", "ctrl+shift+f12", "ctrl+alt+esc"...
    pub fn parse(text: &str) -> Result<Hotkey, String> {
        let mut hk = Hotkey {
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
            key: HotkeyKey::Escape,
        };
        let mut key = None;
        for part in text.split('+').map(|p| p.trim().to_ascii_lowercase()) {
            match part.as_str() {
                "" => return Err(format!("atalho invalido: {text:?}")),
                "ctrl" | "control" => hk.ctrl = true,
                "alt" => hk.alt = true,
                "shift" => hk.shift = true,
                "win" | "super" | "meta" => hk.win = true,
                "esc" | "escape" => key = Some(HotkeyKey::Escape),
                "pause" => key = Some(HotkeyKey::Pause),
                other => {
                    if let Some(c) = single_alphanumeric(other) {
                        key = Some(HotkeyKey::Char(c));
                    } else if let Some(n) = function_key(other) {
                        key = Some(HotkeyKey::Function(n));
                    } else {
                        return Err(format!("tecla desconhecida {other:?} em {text:?}"));
                    }
                }
            }
        }
        hk.key = key.ok_or_else(|| format!("atalho sem tecla principal: {text:?}"))?;
        if !(hk.ctrl || hk.alt || hk.shift || hk.win) && matches!(hk.key, HotkeyKey::Char(_)) {
            return Err("atalho de emergencia precisa de ao menos um modificador".to_string());
        }
        Ok(hk)
    }

    /// Codigo de tecla virtual (VK_*) do Windows.
    pub fn vk_code(&self) -> u32 {
        match self.key {
            HotkeyKey::Char(c) => c as u32, // VK de 'A'..'Z' e '0'..'9' = ASCII maiusculo
            HotkeyKey::Function(n) => 0x6F + n as u32, // VK_F1 = 0x70
            HotkeyKey::Escape => 0x1B,
            HotkeyKey::Pause => 0x13,
        }
    }
}

/// Encerramento de emergencia, independente da aranha e do laco do jogo:
///
/// * thread propria escuta o atalho global (Windows: `RegisterHotKey` + polling de reserva);
/// * watchdog: se o laco principal parar de bater por `watchdog` segundos (travou), o processo
///   encerra sozinho, para um overlay travado nunca bloquear o desktop;
/// * apos o pedido de saida, se o processo continuar vivo por 3 s, e encerrado a forca.
#[derive(Clone)]
pub struct Emergency {
    requested: Arc<AtomicBool>,
    last_beat_ms: Arc<AtomicU64>,
    armed: Arc<AtomicBool>,
    started: Instant,
}

impl Emergency {
    pub fn start(hotkey: &str, watchdog: Duration) -> Emergency {
        let em = Emergency {
            requested: Arc::new(AtomicBool::new(false)),
            last_beat_ms: Arc::new(AtomicU64::new(0)),
            armed: Arc::new(AtomicBool::new(false)),
            started: Instant::now(),
        };
        match Hotkey::parse(hotkey) {
            Ok(hk) => spawn_hotkey_thread(hk, em.requested.clone()),
            Err(e) => {
                bevy::log::warn!("atalho de emergencia invalido ({e}); usando ctrl+alt+shift+q");
                if let Ok(hk) = Hotkey::parse("ctrl+alt+shift+q") {
                    spawn_hotkey_thread(hk, em.requested.clone());
                }
            }
        }
        let w = em.clone();
        let spawn = std::thread::Builder::new()
            .name("spiderdesk-watchdog".into())
            .spawn(move || w.watchdog_loop(watchdog));
        if let Err(e) = spawn {
            bevy::log::error!("nao foi possivel iniciar o watchdog: {e}");
        }
        em
    }

    /// Chamado a cada atualizacao do laco principal. A primeira chamada arma o watchdog.
    pub fn beat(&self) {
        self.last_beat_ms
            .store(self.started.elapsed().as_millis() as u64, Ordering::Relaxed);
        self.armed.store(true, Ordering::Relaxed);
    }

    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Relaxed)
    }

    /// Pede encerramento (usado tambem por `--timeout`).
    pub fn request_exit(&self) {
        self.requested.store(true, Ordering::Relaxed);
    }

    fn watchdog_loop(&self, limit: Duration) {
        let mut exit_requested_at: Option<Instant> = None;
        loop {
            std::thread::sleep(Duration::from_millis(250));
            if self.is_requested() {
                let t0 = *exit_requested_at.get_or_insert_with(Instant::now);
                if t0.elapsed() > Duration::from_secs(3) {
                    eprintln!("spiderdesk: encerramento forcado (saida normal demorou)");
                    std::process::exit(0);
                }
            }
            if self.armed.load(Ordering::Relaxed) {
                let now = self.started.elapsed().as_millis() as u64;
                let last = self.last_beat_ms.load(Ordering::Relaxed);
                if Duration::from_millis(now.saturating_sub(last)) > limit {
                    eprintln!(
                        "spiderdesk: laco principal travado; encerrando para nao bloquear o desktop"
                    );
                    std::process::exit(2);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: u64, x: f32) -> DesktopWindow {
        let r = Rect::from_xywh(x, 0.0, 100.0, 100.0);
        DesktopWindow {
            id,
            rect: r,
            raw_rect: r,
            state: WindowState::Normal,
            pid: 1,
            title: String::new(),
        }
    }

    #[test]
    fn equivalence_ignores_subpixel_noise_but_not_moves_or_order() {
        let a = vec![win(1, 0.0), win(2, 200.0)];
        let mut b = a.clone();
        b[0].rect = Rect::from_xywh(0.6, 0.0, 100.0, 100.0);
        assert!(windows_equivalent(&a, &b));

        let mut moved = a.clone();
        moved[1] = win(2, 230.0);
        assert!(!windows_equivalent(&a, &moved));

        let swapped = vec![win(2, 200.0), win(1, 0.0)];
        assert!(!windows_equivalent(&a, &swapped));
        assert!(!windows_equivalent(&a, &a[..1]));
    }

    #[test]
    fn hotkey_parsing() {
        let h = Hotkey::parse("ctrl+alt+shift+q").unwrap();
        assert!(h.ctrl && h.alt && h.shift && !h.win);
        assert_eq!(h.key, HotkeyKey::Char('Q'));
        assert_eq!(h.vk_code(), 0x51);
        assert_eq!(Hotkey::parse("Ctrl + F12").unwrap().vk_code(), 0x7B);
        assert_eq!(Hotkey::parse("ctrl+alt+esc").unwrap().vk_code(), 0x1B);
        assert!(Hotkey::parse("q").is_err(), "sem modificador");
        assert!(Hotkey::parse("ctrl+alt").is_err(), "sem tecla");
        assert!(Hotkey::parse("ctrl+banana").is_err());
        assert!(Hotkey::parse("ctrl+f13").is_err());
        assert!(Hotkey::parse("ctrl++q").is_err());
    }
}
