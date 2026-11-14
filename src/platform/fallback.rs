//! Implementacao para SOs que nao sao Windows.
//!
//! Linux: janelas reais (extensao do GNOME Shell em `tools/gnome-extension`, ou a lista do X11
//! para apps XWayland/X11), cursor real movido por um mouse virtual (`/dev/uinput`).
//! `SPIDERDESK_FAKE_WINDOWS=1` troca tudo por um desktop simulado com tres janelas (testes).

use bevy::math::Vec2;

use super::{DesktopWindow, HelperStatus, Hotkey, WindowState};
use crate::navigation::geometry::Rect;

/// Nada a configurar fora do Windows.
pub fn init_process() {}

/// Desktop simulado: tres janelas proporcionais a `screen`, da frente para o fundo.
/// As janelas "se mexem" levemente com o tempo para exercitar o recalculo de caminhos
/// quando `SPIDERDESK_FAKE_DRIFT=1`. Com `SPIDERDESK_FAKE_SHAKE=<segundos>` a janela A e
/// "sacudida" (vai-e-vem rapido por ~1,2 s) a cada tantos segundos, para exercitar a derrubada
/// de mobs sem um desktop real.
fn fake_windows(screen: Rect) -> Vec<DesktopWindow> {
    let (w, h) = (screen.width(), screen.height());
    let shake = std::env::var("SPIDERDESK_FAKE_SHAKE")
        .ok()
        .map_or(0.0, |v| {
            let every = v.parse::<f64>().unwrap_or(10.0).max(3.0);
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs_f64())
                .unwrap_or(0.0)
                % every;
            if t < 1.2 {
                ((t * 3.0 * std::f64::consts::TAU).sin() * 80.0) as f32
            } else {
                0.0
            }
        });
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
            w * 0.50 + drift + shake,
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

/// O desktop simulado foi pedido (`SPIDERDESK_FAKE_WINDOWS`, ou as variaveis que o animam)?
fn fake_requested() -> bool {
    [
        "SPIDERDESK_FAKE_WINDOWS",
        "SPIDERDESK_FAKE_SHAKE",
        "SPIDERDESK_FAKE_DRIFT",
    ]
    .iter()
    .any(|v| std::env::var_os(v).is_some())
}

/// Janelas de aplicativos e elementos do desktop visiveis, da frente para o fundo (ordem Z).
///
/// Linux: a extensao do GNOME (ve tambem os apps Wayland nativos) tem prioridade; sem ela, a
/// lista do servidor X (so apps X11/XWayland). Sem nenhuma das duas, lista vazia: os mobs andam
/// nas bordas da tela, nunca em "caixas invisiveis". Os icones da area de trabalho entram no fim
/// (atras de todas as janelas) quando a lista de janelas e confiavel: sem saber quais janelas
/// cobrem os icones, eles virariam caixas invisiveis.
pub fn list_windows(screen: Rect, own_pid: u32) -> Vec<DesktopWindow> {
    if fake_requested() {
        return fake_windows(screen);
    }
    #[cfg(target_os = "linux")]
    {
        let snap = shell::snapshot();
        let trusted = snap.is_some() || !wayland_session();
        let mut wins: Vec<DesktopWindow> = match snap {
            Some(s) => s.windows.into_iter().filter(|w| w.pid != own_pid).collect(),
            None => x11::list_windows(own_pid),
        };
        let icons = match std::env::var("SPIDERDESK_ICONS").as_deref() {
            Ok("0") => false,
            Ok(_) => true, // forca (testes), mesmo sem saber o que cobre os icones
            Err(_) => trusted,
        };
        if icons {
            wins.extend(icons::list());
        }
        wins
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = own_pid;
        fake_windows(screen)
    }
}

/// A sessao grafica e Wayland? (`WAYLAND_DISPLAY` nao serve: o app o remove para usar XWayland.)
#[cfg(target_os = "linux")]
fn wayland_session() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "wayland")
}

#[cfg(target_os = "linux")]
const HELPER_UUID: &str = "spiderdesk-windows@neurelix";

#[cfg(target_os = "linux")]
fn helper_dir() -> Option<std::path::PathBuf> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            Some(std::path::PathBuf::from(std::env::var_os("HOME")?).join(".local/share"))
        })?;
    Some(data.join("gnome-shell/extensions").join(HELPER_UUID))
}

/// Situacao da extensao do GNOME que informa as janelas reais (so faz falta em GNOME + Wayland).
pub fn helper_status() -> HelperStatus {
    #[cfg(target_os = "linux")]
    {
        let gnome = std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.contains("GNOME"));
        if fake_requested() || !wayland_session() || !gnome {
            HelperStatus::NotNeeded
        } else if shell::snapshot().is_some() {
            HelperStatus::Active
        } else if helper_dir().is_some_and(|d| d.join("extension.js").exists()) {
            HelperStatus::NeedsRelogin
        } else {
            HelperStatus::Missing
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        HelperStatus::NotNeeded
    }
}

/// Instala a extensao do GNOME para o usuario atual (os mesmos passos de
/// `tools/install-gnome-extension.sh`) e a deixa ativada. O GNOME so carrega extensoes novas ao
/// iniciar a sessao, entao o efeito vem depois de sair e entrar de novo.
pub fn install_helper() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        const FILES: [(&str, &str); 2] = [
            (
                "metadata.json",
                include_str!(
                    "../../tools/gnome-extension/spiderdesk-windows@neurelix/metadata.json"
                ),
            ),
            (
                "extension.js",
                include_str!(
                    "../../tools/gnome-extension/spiderdesk-windows@neurelix/extension.js"
                ),
            ),
        ];
        let dir = helper_dir().ok_or("pasta do usuario desconhecida")?;
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        for (name, content) in FILES {
            std::fs::write(dir.join(name), content).map_err(|e| e.to_string())?;
        }
        let run = |cmd: &str, args: &[&str]| {
            Command::new(cmd)
                .args(args)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        };
        if run("gnome-extensions", &["enable", HELPER_UUID]).is_some() {
            return Ok(());
        }
        // o Shell em execucao ainda nao conhece a extensao: marca para o proximo login
        let current = run(
            "gsettings",
            &["get", "org.gnome.shell", "enabled-extensions"],
        )
        .ok_or("gsettings indisponivel")?;
        let wanted = enabled_list_with(&current, HELPER_UUID);
        if wanted != current {
            run(
                "gsettings",
                &["set", "org.gnome.shell", "enabled-extensions", &wanted],
            )
            .ok_or("nao foi possivel ativar a extensao")?;
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(())
    }
}

/// Lista de extensoes do gsettings (`['a', 'b']` ou `@as []`) com `uuid` incluido.
#[cfg(target_os = "linux")]
fn enabled_list_with(current: &str, uuid: &str) -> String {
    let current = current.trim();
    if current.contains(uuid) {
        current.to_string()
    } else if let (true, Some(head)) = (current.contains('\''), current.strip_suffix(']')) {
        format!("{head}, '{uuid}']")
    } else {
        format!("['{uuid}']")
    }
}

/// De onde vem a lista de janelas (para o log de inicio e `--list-windows`).
pub fn window_source() -> &'static str {
    if fake_requested() {
        return "desktop simulado (SPIDERDESK_FAKE_WINDOWS)";
    }
    #[cfg(target_os = "linux")]
    {
        if shell::snapshot().is_some() {
            "extensao do GNOME Shell (todas as janelas)"
        } else {
            "servidor X (so apps X11/XWayland; instale tools/gnome-extension para ver os demais)"
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        "desktop simulado"
    }
}

/// Cursor global. Linux: `QueryPointer` do X (exato, mas em sessao Wayland so atualiza com o
/// ponteiro sobre uma janela X, como o overlay capturando) ou a extensao do GNOME (sempre
/// atualiza, a ~10 Hz); vale a fonte que mudou por ultimo. Senao `None` (o chamador usa o
/// cursor da janela do Bevy).
pub fn cursor_position() -> Option<Vec2> {
    #[cfg(target_os = "linux")]
    {
        use std::sync::Mutex;
        use std::time::Instant;
        static LAST_X: Mutex<Option<(Vec2, Instant)>> = Mutex::new(None);
        let x = x11::cursor_position();
        let Some(shell) = shell::snapshot().and_then(|s| s.pointer) else {
            return x;
        };
        let Some(x) = x else { return Some(shell) };
        let mut last = LAST_X.lock().ok()?;
        let now = Instant::now();
        let changed_at = match *last {
            Some((p, at)) if p == x => at,
            _ => {
                *last = Some((x, now));
                now
            }
        };
        // X mexeu agora ha pouco: e a leitura exata; parado ha tempo: pode estar velho
        if now.duration_since(changed_at).as_millis() < 300 {
            Some(x)
        } else {
            Some(shell)
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Move o cursor REAL para `global` (px do desktop virtual). Linux: mouse virtual absoluto em
/// `/dev/uinput` (funciona em X11 e Wayland; precisa de permissao de escrita no dispositivo).
pub fn set_cursor_pos(global: Vec2) -> bool {
    #[cfg(target_os = "linux")]
    {
        uinput::warp(global)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = global;
        false
    }
}

/// O cursor real pode ser movido? No Linux, a primeira chamada cria o mouse virtual (o
/// compositor leva ~1 s para reconhece-lo, entao chame bem antes de precisar).
pub fn can_move_cursor() -> bool {
    #[cfg(target_os = "linux")]
    {
        uinput::available()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Liga/desliga o pedido das imagens das janelas (para o heroi andar por dentro dos apps).
/// Linux: a extensao do GNOME salva as 3 janelas da frente numa pasta privada em memoria e uma
/// thread aqui procura os apoios em cada imagem.
pub fn request_window_images(on: bool) {
    #[cfg(target_os = "linux")]
    {
        captures::request(on);
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = on;
    }
}

/// Apoios vistos por dentro da janela `id` (da ultima imagem analisada), se houver.
pub fn window_ledges(id: u64) -> Option<crate::game::hero::WindowLedges> {
    #[cfg(target_os = "linux")]
    {
        captures::ledges(id)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = id;
        None
    }
}

/// Muda sempre que alguma imagem de janela e reanalisada (ou some).
pub fn window_ledges_version() -> u64 {
    #[cfg(target_os = "linux")]
    {
        captures::version()
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// Chamado na saida normal do app: desfaz o que ficou pedido a plataforma.
pub fn shutdown() {
    #[cfg(target_os = "linux")]
    {
        captures::request(false);
        if !fake_requested() {
            shell::goodbye();
        }
    }
}

/// Area util de cada tela (sem barra superior, barra de tarefas ou dock fixo), px do desktop.
/// Linux: o que o gerenciador de janelas publica (`_GTK_WORKAREAS_D<n>` por tela, ou
/// `_NET_WORKAREA`). Desktop simulado: `SPIDERDESK_FAKE_TOPBAR=<px>` simula uma barra superior.
pub fn work_areas(screen: Rect) -> Vec<Rect> {
    if fake_requested() {
        let bar = std::env::var("SPIDERDESK_FAKE_TOPBAR")
            .ok()
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.0)
            .clamp(0.0, screen.height() * 0.5);
        return if bar > 0.0 {
            vec![Rect::new(screen.min + Vec2::new(0.0, bar), screen.max)]
        } else {
            Vec::new()
        };
    }
    #[cfg(target_os = "linux")]
    {
        let _ = screen;
        x11::work_areas()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = screen;
        Vec::new()
    }
}

/// A posicao do ponteiro e acompanhada em tempo real em qualquer lugar da tela? No Linux so
/// com a extensao do GNOME (em Wayland o X nao ve o ponteiro sobre apps nativos).
pub fn pointer_tracking_live() -> bool {
    #[cfg(target_os = "linux")]
    {
        fake_requested()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|t| t == "x11")
            || shell::snapshot().is_some_and(|s| s.pointer.is_some())
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Segundos desde a ultima entrada do usuario (mouse OU teclado), segundo o sistema. Linux:
/// `org.gnome.Mutter.IdleMonitor`, consultado 1x por segundo numa thread. `None` se indisponivel.
pub fn input_idle_secs() -> Option<f32> {
    #[cfg(target_os = "linux")]
    {
        idle::secs()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

/// Canto superior esquerdo real da janela no desktop (X11: `TranslateCoordinates` para a raiz).
pub fn window_origin(handle: &bevy::window::RawHandleWrapper) -> Option<bevy::math::IVec2> {
    #[cfg(target_os = "linux")]
    {
        x11::window_origin(handle)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = handle;
        None
    }
}

/// Pede para mover a janela para `pos` (X11: `ConfigureWindow`). O gerenciador de janelas pode
/// recusar; quem chama confere depois com [`window_origin`].
pub fn move_window(handle: &bevy::window::RawHandleWrapper, pos: bevy::math::IVec2) -> bool {
    #[cfg(target_os = "linux")]
    {
        x11::move_window(handle, pos)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (handle, pos);
        false
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

/// Imagens das janelas (salvas pela extensao do GNOME como `cap-<id>.png`) -> apoios para o heroi.
///
/// `SPIDERDESK_FAKE_CAPTURE=id:arquivo.png[,id:arquivo.png]` usa imagens fixas (testes e
/// demonstracao com o desktop simulado, sem extensao).
#[cfg(target_os = "linux")]
mod captures {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, SystemTime};

    use crate::game::hero::WindowLedges;
    use crate::navigation::vision::{VisionParams, find_ledges};

    static REQUESTED: AtomicBool = AtomicBool::new(false);
    static VERSION: AtomicU64 = AtomicU64::new(0);

    struct Seen {
        stamp: SystemTime,
        ledges: WindowLedges,
    }

    fn cache() -> &'static Mutex<HashMap<u64, Seen>> {
        static CACHE: OnceLock<Mutex<HashMap<u64, Seen>>> = OnceLock::new();
        CACHE.get_or_init(Default::default)
    }

    pub fn requested() -> bool {
        REQUESTED.load(Ordering::Relaxed)
    }

    pub fn version() -> u64 {
        VERSION.load(Ordering::Relaxed)
    }

    pub fn ledges(id: u64) -> Option<WindowLedges> {
        cache().lock().ok()?.get(&id).map(|s| s.ledges.clone())
    }

    fn analyze(path: &Path) -> Option<WindowLedges> {
        let img = image::open(path).ok()?.into_luma8();
        let (w, h) = (img.width(), img.height());
        let ledges = find_ledges(
            img.as_raw(),
            w as usize,
            h as usize,
            &VisionParams::default(),
        );
        Some(WindowLedges {
            ledges,
            image_size: (w, h),
        })
    }

    /// `cap-123.png` -> 123
    pub fn capture_id(name: &str) -> Option<u64> {
        name.strip_prefix("cap-")?
            .strip_suffix(".png")?
            .parse()
            .ok()
    }

    /// Arquivos a analisar agora: (id da janela, caminho).
    fn sources() -> Vec<(u64, PathBuf)> {
        if let Ok(fake) = std::env::var("SPIDERDESK_FAKE_CAPTURE") {
            return fake
                .split(',')
                .filter_map(|part| {
                    let (id, path) = part.split_once(':')?;
                    Some((id.trim().parse().ok()?, PathBuf::from(path.trim())))
                })
                .collect();
        }
        let Some(dir) = super::shell::dir() else {
            return Vec::new();
        };
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter_map(|e| Some((capture_id(&e.file_name().to_string_lossy())?, e.path())))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn refresh() {
        let files = if requested() { sources() } else { Vec::new() };
        let mut changed = false;
        // analisa fora do cadeado: decodificar um PNG grande leva dezenas de ms
        let mut fresh: Vec<(u64, Seen)> = Vec::new();
        for (id, path) in &files {
            let Ok(stamp) = std::fs::metadata(path).and_then(|m| m.modified()) else {
                continue;
            };
            let known = cache()
                .lock()
                .is_ok_and(|c| c.get(id).is_some_and(|s| s.stamp == stamp));
            if !known && let Some(ledges) = analyze(path) {
                fresh.push((*id, Seen { stamp, ledges }));
            }
        }
        if let Ok(mut c) = cache().lock() {
            let before = c.len();
            c.retain(|id, _| files.iter().any(|(f, _)| f == id));
            changed |= c.len() != before || !fresh.is_empty();
            c.extend(fresh);
        }
        if changed {
            VERSION.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn request(on: bool) {
        REQUESTED.store(on, Ordering::Relaxed);
        static STARTED: OnceLock<()> = OnceLock::new();
        if on {
            STARTED.get_or_init(|| {
                let _ = std::thread::Builder::new()
                    .name("spiderdesk-vision".into())
                    .spawn(|| {
                        loop {
                            refresh();
                            std::thread::sleep(Duration::from_millis(250));
                        }
                    });
            });
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn capture_file_names() {
            assert_eq!(super::capture_id("cap-3565270378.png"), Some(3565270378));
            assert_eq!(super::capture_id("cap-12.tmp"), None);
            assert_eq!(super::capture_id("windows.txt"), None);
        }
    }
}

/// Icones da area de trabalho (extensao "Desktop Icons NG" do GNOME) como elementos onde os mobs
/// andam. A posicao de cada icone fica nos metadados do arquivo (`gio info`); uma thread relê a
/// cada poucos segundos.
#[cfg(target_os = "linux")]
mod icons {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;

    use super::super::ELEMENT_ID_BASE;
    use super::{DesktopWindow, Rect, WindowState};

    static CACHE: Mutex<Vec<DesktopWindow>> = Mutex::new(Vec::new());

    fn run(cmd: &str, args: &[&str]) -> Option<String> {
        let out = Command::new(cmd).args(args).output().ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// (tamanho do icone, largura da celula da grade) para a opcao `icon-size` da extensao.
    pub fn metrics(size: &str) -> (f32, f32) {
        let (icon, desired) = match size {
            "tiny" => (36.0, 70.0),
            "small" => (48.0, 90.0),
            "large" => (96.0, 130.0),
            _ => (64.0, 120.0),
        };
        // a grade estica as celulas para preencher o monitor (calculado para 1920 px de largura)
        let columns = (1920.0 / (desired + 8.0_f32)).floor().max(1.0);
        (icon, 1920.0 / columns)
    }

    /// Saida de `gio info -a metadata::nautilus-icon-position ARQUIVOS...` -> um elemento por
    /// icone com posicao salva (`x,y` = canto da celula, px do desktop).
    pub fn parse(text: &str, size: &str) -> Vec<DesktopWindow> {
        let (icon, cell) = metrics(size);
        let mut out = Vec::new();
        let mut uri = "";
        for line in text.lines() {
            if let Some(u) = line.strip_prefix("uri: ") {
                uri = u.trim();
                continue;
            }
            let Some(pos) = line
                .trim()
                .strip_prefix("metadata::nautilus-icon-position:")
            else {
                continue;
            };
            let mut n = pos
                .trim()
                .split(',')
                .filter_map(|v| v.trim().parse::<f32>().ok());
            let (Some(x), Some(y)) = (n.next(), n.next()) else {
                continue;
            };
            // imagem + rotulo, centrados na celula; vizinhos de linha ficam a um passo de distancia
            let w = cell - 12.0;
            let rect = Rect::from_xywh(x + (cell - w) * 0.5, y + 4.0, w, icon + 28.0);
            let mut h = DefaultHasher::new();
            uri.hash(&mut h);
            out.push(DesktopWindow {
                id: ELEMENT_ID_BASE + (h.finish() & 0xFFFF_FFFF),
                rect,
                raw_rect: rect,
                state: WindowState::Normal,
                pid: 0,
                title: percent_decode(uri.rsplit('/').next().unwrap_or("")),
            });
        }
        out
    }

    /// "Snapmaker%20(C%C3%B3pia).desktop" -> "Snapmaker (Cópia).desktop"
    pub fn percent_decode(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            let hex = (bytes[i] == b'%')
                .then(|| s.get(i + 1..i + 3))
                .flatten()
                .and_then(|h| u8::from_str_radix(h, 16).ok());
            match hex {
                Some(b) => {
                    out.push(b);
                    i += 3;
                }
                None => {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
        }
        String::from_utf8_lossy(&out).into_owned()
    }

    fn refresh() -> Vec<DesktopWindow> {
        let shown = run(
            "gsettings",
            &["get", "org.gnome.shell", "enabled-extensions"],
        )
        .is_some_and(|l| l.contains("ding@") || l.contains("desktop-icons"));
        if !shown {
            return Vec::new();
        }
        let size = run(
            "gsettings",
            &["get", "org.gnome.shell.extensions.ding", "icon-size"],
        )
        .map(|s| s.trim().trim_matches('\'').to_string())
        .unwrap_or_default();
        let dir = run("xdg-user-dir", &["DESKTOP"])
            .map(|d| PathBuf::from(d.trim()))
            .filter(|d| d.is_dir())
            .or_else(|| Some(PathBuf::from(std::env::var_os("HOME")?).join("Desktop")));
        let Some(dir) = dir else { return Vec::new() };
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| !n.to_string_lossy().starts_with('.'))
            })
            .take(300)
            .collect();
        if files.is_empty() {
            return Vec::new();
        }
        Command::new("gio")
            .args(["info", "-a", "metadata::nautilus-icon-position"])
            .args(&files)
            .output()
            .ok()
            .map(|o| parse(&String::from_utf8_lossy(&o.stdout), &size))
            .unwrap_or_default()
    }

    pub fn list() -> Vec<DesktopWindow> {
        static STARTED: OnceLock<()> = OnceLock::new();
        STARTED.get_or_init(|| {
            if let Ok(mut c) = CACHE.lock() {
                *c = refresh(); // ja na primeira consulta
            }
            let _ = std::thread::Builder::new()
                .name("spiderdesk-icons".into())
                .spawn(|| {
                    loop {
                        std::thread::sleep(Duration::from_secs(4));
                        let fresh = refresh();
                        if let Ok(mut c) = CACHE.lock() {
                            *c = fresh;
                        }
                    }
                });
        });
        CACHE.lock().map(|c| c.clone()).unwrap_or_default()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn icons_become_small_elements_next_to_each_other() {
            let text = "uri: file:///home/u/Desktop/Creality-V7.desktop\n\
                        caminho local: /home/u/Desktop/Creality-V7.desktop\n\
                        atributos:\n  metadata::nautilus-icon-position: 103,913\n\
                        uri: file:///home/u/Desktop/sem-posicao.txt\natributos:\n\
                        uri: file:///home/u/Desktop/Antigravity.desktop\n\
                        atributos:\n  metadata::nautilus-icon-position: 204,913\n";
            let v = parse(text, "small");
            assert_eq!(v.len(), 2, "arquivo sem posicao salva fica de fora");
            assert!(v.iter().all(|w| w.is_element()));
            assert_eq!(v[0].title, "Creality-V7.desktop");
            assert_eq!(
                percent_decode("Orca%20(C%C3%B3pia).desktop"),
                "Orca (Cópia).desktop"
            );
            assert_eq!(percent_decode("100%"), "100%");
            assert_ne!(v[0].id, v[1].id);
            let (a, b) = (v[0].rect, v[1].rect);
            assert!((a.width() - 89.0).abs() < 1.0 && (a.height() - 76.0).abs() < 0.1);
            assert!(a.min.x > 103.0 && a.max.x < 204.0 && a.min.y == 917.0);
            let gap = b.min.x - a.max.x;
            assert!(
                gap > 0.0 && gap <= 14.0,
                "vizinhos ao alcance de um passo: {gap}"
            );
        }
    }
}

/// Tempo ocioso do usuario pelo monitor de ociosidade do Mutter (GNOME), via `gdbus`.
#[cfg(target_os = "linux")]
mod idle {
    use std::process::Command;
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    /// (ocioso em s, quando foi lido)
    static LAST: Mutex<Option<(f32, Instant)>> = Mutex::new(None);

    /// "(uint64 28536,)" -> 28.536
    pub fn parse(reply: &str) -> Option<f32> {
        let digits: String = reply
            .split("uint64")
            .nth(1)?
            .chars()
            .skip_while(|c| !c.is_ascii_digit())
            .take_while(|c| c.is_ascii_digit())
            .collect();
        Some(digits.parse::<u64>().ok()? as f32 / 1000.0)
    }

    fn query() -> Option<f32> {
        let out = Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--timeout",
                "2",
                "--dest",
                "org.gnome.Mutter.IdleMonitor",
                "--object-path",
                "/org/gnome/Mutter/IdleMonitor/Core",
                "--method",
                "org.gnome.Mutter.IdleMonitor.GetIdletime",
            ])
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| parse(&String::from_utf8_lossy(&out.stdout)))
            .flatten()
    }

    pub fn secs() -> Option<f32> {
        static STARTED: OnceLock<()> = OnceLock::new();
        STARTED.get_or_init(|| {
            let _ = std::thread::Builder::new()
                .name("spiderdesk-idle".into())
                .spawn(|| {
                    let mut failures = 0;
                    // sem o servico (nao e GNOME): desiste depois de algumas tentativas
                    while failures < 5 {
                        match query() {
                            Some(v) => {
                                failures = 0;
                                if let Ok(mut last) = LAST.lock() {
                                    *last = Some((v, Instant::now()));
                                }
                            }
                            None => failures += 1,
                        }
                        std::thread::sleep(Duration::from_millis(1000));
                    }
                });
        });
        let (idle, at) = (*LAST.lock().ok()?)?;
        let age = at.elapsed();
        // leitura velha demais nao vale (thread parou)
        (age < Duration::from_secs(5)).then_some(idle + age.as_secs_f32())
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn parses_the_gdbus_reply() {
            assert_eq!(super::parse("(uint64 28536,)\n"), Some(28.536));
            assert_eq!(super::parse("(uint64 0,)"), Some(0.0));
            assert_eq!(super::parse("Erro: nada"), None);
        }
    }
}

/// Mouse virtual absoluto (`/dev/uinput`): unica forma de mover o ponteiro em Wayland.
#[cfg(target_os = "linux")]
mod uinput {
    use std::fs::{File, OpenOptions};
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::sync::{Mutex, OnceLock};

    use bevy::math::Vec2;

    // <linux/uinput.h> / <linux/input-event-codes.h>
    const UI_DEV_CREATE: libc::c_ulong = 0x5501;
    const UI_DEV_SETUP: libc::c_ulong = 0x405c_5503;
    const UI_ABS_SETUP: libc::c_ulong = 0x401c_5504;
    const UI_SET_EVBIT: libc::c_ulong = 0x4004_5564;
    const UI_SET_KEYBIT: libc::c_ulong = 0x4004_5565;
    const UI_SET_ABSBIT: libc::c_ulong = 0x4004_5567;
    const EV_SYN: u16 = 0;
    const EV_KEY: libc::c_int = 1;
    const EV_ABS: u16 = 3;
    const ABS_X: u16 = 0;
    const ABS_Y: u16 = 1;
    const BTN_LEFT: libc::c_int = 0x110;

    struct Pointer {
        file: File,
        size: (i32, i32),
    }

    fn create() -> Option<Pointer> {
        // o compositor mapeia o eixo absoluto para o desktop inteiro (todas as telas)
        let (w, h) = super::x11::root_size()?;
        let file = OpenOptions::new().write(true).open("/dev/uinput").ok()?;
        let fd = file.as_raw_fd();
        let mut name = [0 as libc::c_char; 80];
        for (dst, src) in name.iter_mut().zip(b"SpiderDesk virtual pointer") {
            *dst = *src as libc::c_char;
        }
        let setup = libc::uinput_setup {
            id: libc::input_id {
                bustype: 0x06, // BUS_VIRTUAL
                vendor: 0x5344,
                product: 0x0001,
                version: 1,
            },
            name,
            ff_effects_max: 0,
        };
        // SAFETY: `fd` e um descritor valido de /dev/uinput; cada ioctl recebe um inteiro ou um
        // ponteiro para uma struct do tamanho que o kernel espera, viva durante a chamada.
        let ok = unsafe {
            let mut ok = libc::ioctl(fd, UI_SET_EVBIT, EV_KEY) >= 0
                && libc::ioctl(fd, UI_SET_KEYBIT, BTN_LEFT) >= 0
                && libc::ioctl(fd, UI_SET_EVBIT, EV_ABS as libc::c_int) >= 0;
            for (code, max) in [(ABS_X, w - 1), (ABS_Y, h - 1)] {
                let abs = libc::uinput_abs_setup {
                    code,
                    absinfo: libc::input_absinfo {
                        value: 0,
                        minimum: 0,
                        maximum: max,
                        fuzz: 0,
                        flat: 0,
                        resolution: 0,
                    },
                };
                ok = ok
                    && libc::ioctl(fd, UI_SET_ABSBIT, code as libc::c_int) >= 0
                    && libc::ioctl(fd, UI_ABS_SETUP, &abs) >= 0;
            }
            ok && libc::ioctl(fd, UI_DEV_SETUP, &setup) >= 0 && libc::ioctl(fd, UI_DEV_CREATE) >= 0
        };
        ok.then_some(Pointer { file, size: (w, h) })
    }

    fn device() -> &'static Mutex<Option<Pointer>> {
        static DEV: OnceLock<Mutex<Option<Pointer>>> = OnceLock::new();
        DEV.get_or_init(|| {
            let dev = create();
            if dev.is_none() {
                bevy::log::warn!(
                    "sem acesso a /dev/uinput: a aranha nao consegue mover o cursor real \
                     (sequestro do mouse desligado)"
                );
            }
            Mutex::new(dev)
        })
    }

    pub fn available() -> bool {
        device().lock().is_ok_and(|d| d.is_some())
    }

    /// `struct input_event`: timeval zerado (o kernel carimba) + tipo + codigo + valor.
    fn event(buf: &mut Vec<u8>, kind: u16, code: u16, value: i32) {
        buf.extend(std::iter::repeat_n(0u8, size_of::<libc::timeval>()));
        buf.extend(kind.to_ne_bytes());
        buf.extend(code.to_ne_bytes());
        buf.extend(value.to_ne_bytes());
    }

    pub fn warp(global: Vec2) -> bool {
        let Ok(mut guard) = device().lock() else {
            return false;
        };
        let Some(dev) = guard.as_mut() else {
            return false;
        };
        let x = (global.x.round() as i32).clamp(0, dev.size.0 - 1);
        let y = (global.y.round() as i32).clamp(0, dev.size.1 - 1);
        let mut buf = Vec::with_capacity(72);
        event(&mut buf, EV_ABS, ABS_X, x);
        event(&mut buf, EV_ABS, ABS_Y, y);
        event(&mut buf, EV_SYN, 0, 0);
        dev.file.write_all(&buf).is_ok()
    }
}

/// Janelas e ponteiro publicados pela extensao do GNOME Shell (`tools/gnome-extension`).
///
/// Em Wayland nenhum app comum pode perguntar onde estao as janelas dos outros; a extensao roda
/// dentro do Shell e escreve um arquivo de texto em `$XDG_RUNTIME_DIR/spiderdesk/windows.txt`
/// enquanto o SpiderDesk estiver vivo (arquivo `alive` na mesma pasta).
#[cfg(target_os = "linux")]
mod shell {
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::time::{Duration, Instant, SystemTime};

    use bevy::math::Vec2;

    use super::{DesktopWindow, Rect, WindowState};

    #[derive(Clone, Default)]
    pub struct Snapshot {
        pub windows: Vec<DesktopWindow>,
        pub pointer: Option<Vec2>,
    }

    struct Cache {
        read_at: Instant,
        beat_at: Option<Instant>,
        snap: Option<Snapshot>,
    }

    pub fn dir() -> Option<PathBuf> {
        Some(PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR")?).join("spiderdesk"))
    }

    /// Avisa a extensao que o app saiu (ela apaga as imagens na hora, sem esperar o batimento
    /// envelhecer).
    pub fn goodbye() {
        if let Some(dir) = dir() {
            let _ = std::fs::remove_file(dir.join("alive"));
        }
    }

    /// Formato: uma linha por item, da frente para o fundo.
    /// `pointer X Y` | `win ID X Y W H PID MAX titulo ate o fim da linha` |
    /// `elem N X Y W H nome` (barra superior e outros elementos do Shell).
    pub fn parse(text: &str) -> Snapshot {
        let mut snap = Snapshot::default();
        for line in text.lines() {
            let mut it = line.splitn(9, ' ');
            match it.next() {
                Some("pointer") => {
                    let mut n = it.filter_map(|v| v.parse::<f32>().ok());
                    if let (Some(x), Some(y)) = (n.next(), n.next()) {
                        snap.pointer = Some(Vec2::new(x, y));
                    }
                }
                Some("elem") => {
                    let mut num = || it.next().and_then(|v| v.parse::<f64>().ok());
                    let (Some(n), Some(x), Some(y), Some(w), Some(h)) =
                        (num(), num(), num(), num(), num())
                    else {
                        continue;
                    };
                    let rect = Rect::from_xywh(x as f32, y as f32, w as f32, h as f32);
                    snap.windows.push(DesktopWindow {
                        id: super::super::ELEMENT_ID_BASE + (1 << 36) + n as u64,
                        rect,
                        raw_rect: rect,
                        state: WindowState::Normal,
                        pid: 0,
                        title: it.collect::<Vec<_>>().join(" "),
                    });
                }
                Some("win") => {
                    let mut num = || it.next().and_then(|v| v.parse::<f64>().ok());
                    let (Some(id), Some(x), Some(y), Some(w), Some(h), Some(pid), Some(max)) =
                        (num(), num(), num(), num(), num(), num(), num())
                    else {
                        continue;
                    };
                    let rect = Rect::from_xywh(x as f32, y as f32, w as f32, h as f32);
                    snap.windows.push(DesktopWindow {
                        id: id as u64,
                        rect,
                        raw_rect: rect,
                        state: if max != 0.0 {
                            WindowState::Maximized
                        } else {
                            WindowState::Normal
                        },
                        pid: pid as u32,
                        title: it.next().unwrap_or("").to_string(),
                    });
                }
                _ => {}
            }
        }
        snap
    }

    /// Estado mais recente da extensao; `None` se ela nao esta instalada/ativa.
    pub fn snapshot() -> Option<Snapshot> {
        static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
        let mut guard = CACHE.lock().ok()?;
        let now = Instant::now();
        let cache = guard.get_or_insert_with(|| Cache {
            read_at: now - Duration::from_secs(1),
            beat_at: None,
            snap: None,
        });
        if now.duration_since(cache.read_at) < Duration::from_millis(40) {
            return cache.snap.clone();
        }
        cache.read_at = now;
        let dir = dir()?;
        // batimento: a extensao so trabalha enquanto este arquivo for recente
        if cache
            .beat_at
            .is_none_or(|t| now.duration_since(t) > Duration::from_secs(1))
        {
            cache.beat_at = Some(now);
            let _ = std::fs::create_dir_all(&dir);
            // (o pid deixa a extensao pular as nossas janelas; capture=1 pede as imagens)
            let beat = format!(
                "pid={} capture={}\n",
                std::process::id(),
                super::captures::requested() as u8
            );
            let _ = std::fs::write(dir.join("alive"), beat);
        }
        let path = dir.join("windows.txt");
        let fresh = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| SystemTime::now().duration_since(m).ok())
            .is_some_and(|age| age < Duration::from_secs(3));
        cache.snap = fresh
            .then(|| std::fs::read_to_string(&path).ok())
            .flatten()
            .map(|t| parse(&t));
        cache.snap.clone()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn parses_pointer_and_windows_in_z_order_and_skips_garbage() {
            let mut s = parse(
                "pointer 812 430\nelem 1 0 0 1920 32 top-bar\n\
                 win 77 100 50 800 600 4321 0 Editor de texto - notas.txt\n\
                 lixo\nwin 9 0 0 1920 1080 12 1 \nwin quebrado\n",
            );
            assert_eq!(s.pointer, Some(Vec2::new(812.0, 430.0)));
            assert_eq!(s.windows.len(), 3);
            assert!(s.windows[0].is_element() && s.windows[0].title == "top-bar");
            assert_eq!(s.windows[0].rect, Rect::from_xywh(0.0, 0.0, 1920.0, 32.0));
            s.windows.remove(0);
            assert_eq!(s.windows[0].id, 77);
            assert_eq!(s.windows[0].pid, 4321);
            assert_eq!(s.windows[0].title, "Editor de texto - notas.txt");
            assert_eq!(
                s.windows[0].rect,
                Rect::from_xywh(100.0, 50.0, 800.0, 600.0)
            );
            assert_eq!(s.windows[0].state, WindowState::Normal);
            assert_eq!(s.windows[1].state, WindowState::Maximized);
        }
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
        AtomEnum, ClientMessageEvent, ClipOrdering, ConfigureWindowAux, ConnectionExt as _,
        EventMask, MapState, Rectangle, StackMode,
    };

    use super::{DesktopWindow, Rect, WindowState};
    use x11rb::rust_connection::RustConnection;

    struct X {
        conn: RustConnection,
        root: u32,
        net_wm_state: u32,
        net_wm_state_above: u32,
        size: (i32, i32),
        client_list: u32,
        wm_pid: u32,
        hidden: u32,
        max_vert: u32,
        wm_desktop: u32,
        current_desktop: u32,
        window_type: u32,
        type_desktop: u32,
        type_dock: u32,
        net_wm_name: u32,
        frame_extents: u32,
    }

    /// Conexao propria com o servidor X (o X permite alterar janelas de outra conexao).
    fn x() -> &'static Mutex<Option<X>> {
        static X11: OnceLock<Mutex<Option<X>>> = OnceLock::new();
        X11.get_or_init(|| {
            let conn = std::env::var_os("DISPLAY")
                .and_then(|_| RustConnection::connect(None).ok())
                .map(|(conn, screen)| {
                    let scr = conn.setup().roots.get(screen);
                    let root = scr.map(|s| s.root).unwrap_or(0);
                    let size = scr.map_or((0, 0), |s| {
                        (s.width_in_pixels as i32, s.height_in_pixels as i32)
                    });
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
                        size,
                        client_list: atom(b"_NET_CLIENT_LIST_STACKING"),
                        wm_pid: atom(b"_NET_WM_PID"),
                        hidden: atom(b"_NET_WM_STATE_HIDDEN"),
                        max_vert: atom(b"_NET_WM_STATE_MAXIMIZED_VERT"),
                        wm_desktop: atom(b"_NET_WM_DESKTOP"),
                        current_desktop: atom(b"_NET_CURRENT_DESKTOP"),
                        window_type: atom(b"_NET_WM_WINDOW_TYPE"),
                        type_desktop: atom(b"_NET_WM_WINDOW_TYPE_DESKTOP"),
                        type_dock: atom(b"_NET_WM_WINDOW_TYPE_DOCK"),
                        net_wm_name: atom(b"_NET_WM_NAME"),
                        frame_extents: atom(b"_NET_FRAME_EXTENTS"),
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

    /// Areas uteis da area de trabalho atual: `_GTK_WORKAREAS_D<n>` (uma por tela, publicada pelo
    /// Mutter) ou, sem ela, `_NET_WORKAREA` (uma so, para o desktop todo).
    pub fn work_areas() -> Vec<Rect> {
        let Ok(guard) = x().lock() else {
            return Vec::new();
        };
        let Some(x) = guard.as_ref() else {
            return Vec::new();
        };
        let atom = |name: &str| {
            x.conn
                .intern_atom(true, name.as_bytes())
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|r| r.atom)
                .unwrap_or(0)
        };
        let desk = x
            .prop32(x.root, x.current_desktop)
            .first()
            .copied()
            .unwrap_or(0) as usize;
        let rects = |v: &[u32]| -> Vec<Rect> {
            v.as_chunks::<4>()
                .0
                .iter()
                .map(|c| Rect::from_xywh(c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32))
                .filter(|r| r.width() > 0.0 && r.height() > 0.0)
                .collect()
        };
        let per_screen = rects(&x.prop32(x.root, atom(&format!("_GTK_WORKAREAS_D{desk}"))));
        if !per_screen.is_empty() {
            return per_screen;
        }
        let all = x.prop32(x.root, atom("_NET_WORKAREA"));
        rects(all.get(desk * 4..desk * 4 + 4).unwrap_or(&[]))
    }

    /// Tamanho da janela raiz = desktop virtual inteiro (todas as telas).
    pub fn root_size() -> Option<(i32, i32)> {
        let guard = x().lock().ok()?;
        let size = guard.as_ref()?.size;
        (size.0 > 0 && size.1 > 0).then_some(size)
    }

    impl X {
        /// Propriedade de 32 bits (lista de atomos/cardinais); vazia se nao existir.
        fn prop32(&self, window: u32, atom: u32) -> Vec<u32> {
            if atom == 0 {
                return Vec::new();
            }
            self.conn
                .get_property(false, window, atom, AtomEnum::ANY, 0, 4096)
                .ok()
                .and_then(|c| c.reply().ok())
                .and_then(|r| r.value32().map(|v| v.collect()))
                .unwrap_or_default()
        }
    }

    /// Janelas X11 gerenciadas (apps X11/XWayland), da frente para o fundo.
    pub fn list_windows(own_pid: u32) -> Vec<DesktopWindow> {
        let Ok(guard) = x().lock() else {
            return Vec::new();
        };
        let Some(x) = guard.as_ref() else {
            return Vec::new();
        };
        let current = x.prop32(x.root, x.current_desktop).first().copied();
        let mut out = Vec::new();
        // a propriedade vem do fundo para a frente
        for id in x.prop32(x.root, x.client_list).into_iter().rev() {
            let pid = x.prop32(id, x.wm_pid).first().copied().unwrap_or(0);
            if pid == own_pid {
                continue;
            }
            let state = x.prop32(id, x.net_wm_state);
            let kind = x.prop32(id, x.window_type);
            if state.contains(&x.hidden)
                || kind.contains(&x.type_desktop)
                || kind.contains(&x.type_dock)
            {
                continue;
            }
            // outra area de trabalho (0xFFFFFFFF = fixada em todas)
            if let (Some(d), Some(cur)) = (x.prop32(id, x.wm_desktop).first(), current)
                && *d != cur
                && *d != u32::MAX
            {
                continue;
            }
            let mapped = x
                .conn
                .get_window_attributes(id)
                .ok()
                .and_then(|c| c.reply().ok())
                .is_some_and(|a| a.map_state == MapState::VIEWABLE);
            let geo = x.conn.get_geometry(id).ok().and_then(|c| c.reply().ok());
            let at = x
                .conn
                .translate_coordinates(id, x.root, 0, 0)
                .ok()
                .and_then(|c| c.reply().ok());
            let (true, Some(geo), Some(at)) = (mapped, geo, at) else {
                continue;
            };
            // moldura desenhada pelo gerenciador: esquerda, direita, topo, base
            let fe = x.prop32(id, x.frame_extents);
            let ext = |i: usize| fe.get(i).copied().unwrap_or(0).min(200) as f32;
            let rect = Rect::from_xywh(
                at.dst_x as f32 - ext(0),
                at.dst_y as f32 - ext(2),
                geo.width as f32 + ext(0) + ext(1),
                geo.height as f32 + ext(2) + ext(3),
            );
            let title = x
                .conn
                .get_property(false, id, x.net_wm_name, AtomEnum::ANY, 0, 256)
                .ok()
                .and_then(|c| c.reply().ok())
                .map(|r| String::from_utf8_lossy(&r.value).into_owned())
                .unwrap_or_default();
            out.push(DesktopWindow {
                id: id as u64,
                rect,
                raw_rect: rect,
                state: if state.contains(&x.max_vert) {
                    WindowState::Maximized
                } else {
                    WindowState::Normal
                },
                pid,
                title,
            });
        }
        out
    }

    fn window_id(handle: &bevy::window::RawHandleWrapper) -> Option<u32> {
        match handle.get_window_handle() {
            RawWindowHandle::Xlib(h) => Some(h.window as u32),
            RawWindowHandle::Xcb(h) => Some(h.window.get()),
            _ => None,
        }
    }

    pub fn window_origin(handle: &bevy::window::RawHandleWrapper) -> Option<bevy::math::IVec2> {
        let window = window_id(handle)?;
        let guard = x().lock().ok()?;
        let x = guard.as_ref()?;
        let r = x
            .conn
            .translate_coordinates(window, x.root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        Some(bevy::math::IVec2::new(r.dst_x as i32, r.dst_y as i32))
    }

    pub fn move_window(handle: &bevy::window::RawHandleWrapper, pos: bevy::math::IVec2) -> bool {
        let Some(window) = window_id(handle) else {
            return false;
        };
        let Ok(guard) = x().lock() else { return false };
        let Some(x) = guard.as_ref() else {
            return false;
        };
        x.conn
            .configure_window(window, &ConfigureWindowAux::new().x(pos.x).y(pos.y))
            .is_ok()
            && x.conn.flush().is_ok()
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

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::enabled_list_with;

    #[test]
    fn adds_the_helper_to_the_gsettings_extension_list_once() {
        assert_eq!(enabled_list_with("@as []", "x@y"), "['x@y']");
        assert_eq!(enabled_list_with("['a@b']", "x@y"), "['a@b', 'x@y']");
        assert_eq!(
            enabled_list_with("['a@b', 'x@y']\n", "x@y"),
            "['a@b', 'x@y']"
        );
    }
}
