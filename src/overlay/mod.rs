//! Overlay transparente: janela sem bordas, sempre no topo, com click-through seletivo.
//!
//! Principios de seguranca (prioridade absoluta: nunca bloquear o uso normal do computador):
//! 1. a janela nasce em click-through total (`hit_test = false`);
//! 2. so captura cliques enquanto o cursor esta sobre a aranha (+ histerese);
//! 3. atalho global de emergencia e watchdog rodam em threads independentes (`platform::Emergency`);
//! 4. a janela nao ativa, nao rouba foco e nao aparece no Alt+Tab.

pub mod input;
pub mod window;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::winit::{UpdateMode, WinitSettings};

use crate::config::Config;
use crate::platform::Emergency;

pub use input::{CursorState, HitCircle, HitTargets};
pub use window::OverlayGeometry;

/// Nivel de atividade da cena; define a taxa de atualizacao (economia de CPU/GPU).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Activity {
    /// Animacao em andamento: usa `fps_limit`.
    #[default]
    Fast,
    /// Aranha parada e cursor longe: `idle_update_hz`.
    Calm,
    /// Nada visivel: `hidden_update_hz`.
    Hidden,
}

#[derive(Resource, Clone)]
pub struct EmergencyRes(pub Emergency);

/// Configuracao como escrita pelo usuario (100% DPI). `Config` e a versao efetiva, escalada
/// pelo fator de escala do monitor.
#[derive(Resource, Clone)]
pub struct BaseConfig(pub Config);

/// Opcoes de execucao vindas da linha de comando.
#[derive(Resource, Debug, Clone, Default)]
pub struct RunOptions {
    /// Encerra sozinho apos N segundos (testes automatizados).
    pub timeout_secs: Option<f32>,
    /// Modo prova de conceito: cubo 3D em vez da aranha.
    pub poc: bool,
    /// Nao abre o painel flutuante.
    pub no_panel: bool,
    /// Teste: aciona os botoes do painel automaticamente.
    pub panel_selftest: bool,
    /// Mobs extras a spawnar ao iniciar (rotulos do catalogo ou indices).
    pub spawn: Vec<String>,
    /// Teste: simula um clique na aranha apos N segundos (e a cada reaparecimento).
    pub auto_kill_secs: Option<f32>,
    /// Salva um quadro renderizado (RGBA) neste arquivo e encerra.
    pub screenshot: Option<std::path::PathBuf>,
}

/// Ordem de execucao por quadro: `Input` (janela, cursor) -> `Game` (aranha etc.) -> `Output`
/// (click-through com as regioes recem-calculadas e ritmo do laco).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum OverlaySet {
    Input,
    Game,
    Output,
}

pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OverlayGeometry>()
            .init_resource::<CursorState>()
            .init_resource::<HitTargets>()
            .init_resource::<Activity>()
            .init_resource::<RunOptions>()
            .configure_sets(
                Update,
                (OverlaySet::Input, OverlaySet::Game, OverlaySet::Output).chain(),
            )
            .add_systems(Startup, start_emergency)
            .add_systems(
                Update,
                (
                    window::fit_overlay_window,
                    apply_dpi,
                    heartbeat_and_exit,
                    input::update_cursor,
                )
                    .chain()
                    .in_set(OverlaySet::Input),
            )
            .add_systems(
                Update,
                (input::apply_click_through, adjust_update_rate)
                    .chain()
                    .in_set(OverlaySet::Output),
            );
    }
}

/// Recalcula a configuracao efetiva quando o fator de escala do monitor muda.
fn apply_dpi(
    geo: Res<OverlayGeometry>,
    base: Res<BaseConfig>,
    mut cfg: ResMut<Config>,
    mut applied: Local<Option<f32>>,
) {
    if !geo.ready || *applied == Some(geo.monitor_scale) {
        return;
    }
    *cfg = base.0.scaled(geo.monitor_scale);
    *applied = Some(geo.monitor_scale);
    info!("escala DPI do monitor: {:.0}%", geo.monitor_scale * 100.0);
}

fn start_emergency(mut commands: Commands, cfg: Res<Config>) {
    let em = Emergency::start(
        &cfg.overlay.emergency_hotkey,
        std::time::Duration::from_secs_f32(cfg.overlay.watchdog_secs),
    );
    info!(
        "encerramento de emergencia: {} (watchdog {:.0}s)",
        cfg.overlay.emergency_hotkey, cfg.overlay.watchdog_secs
    );
    commands.insert_resource(EmergencyRes(em));
}

/// Bate o coracao do watchdog, trata pedidos de saida (atalho global / `--timeout`) e
/// reafirma "sempre no topo" periodicamente.
#[allow(clippy::too_many_arguments)]
fn heartbeat_and_exit(
    cfg: Res<Config>,
    em: Option<Res<EmergencyRes>>,
    opts: Res<RunOptions>,
    time: Res<Time<Real>>,
    windows: Query<Option<&bevy::window::RawHandleWrapper>, With<PrimaryWindow>>,
    all_windows: Query<(&bevy::window::RawHandleWrapper, &Window)>,
    mut exit: MessageWriter<AppExit>,
    mut last_topmost: Local<f32>,
) {
    let Some(em) = em else { return };
    em.0.beat();

    let elapsed = time.elapsed_secs();
    if em.0.is_requested() {
        info!("encerramento solicitado");
        exit.write(AppExit::Success);
        return;
    }
    if let Some(limit) = opts.timeout_secs
        && elapsed >= limit
    {
        info!("--timeout atingido ({limit:.1}s); encerrando");
        em.0.request_exit();
        exit.write(AppExit::Success);
        return;
    }

    // o winit apaga NOACTIVATE/TOOLWINDOW ao alternar o click-through: devolve todo quadro
    if let Ok(Some(handle)) = windows.single() {
        crate::platform::ensure_overlay_styles(handle, cfg.overlay.layered_alpha_fix);
    }

    // overlay e painel sempre no topo: reafirma a cada 1 s (outros programas "sempre no topo"
    // podem passar por cima; no Linux/X11 o pedido inicial do winit chega com a janela oculta e o
    // gerenciador de janelas o ignora)
    if elapsed - *last_topmost >= 1.0 {
        *last_topmost = elapsed;
        for (handle, window) in &all_windows {
            if window.visible
                && let Err(e) = crate::platform::keep_on_top(handle)
            {
                debug!("keep_on_top: {e}");
            }
        }
    }
}

/// Ajusta o ritmo do laco de eventos conforme a atividade.
fn adjust_update_rate(
    cfg: Res<Config>,
    activity: Res<Activity>,
    mut settings: ResMut<WinitSettings>,
) {
    let hz = match *activity {
        Activity::Fast => cfg.overlay.fps_limit as f32,
        Activity::Calm => cfg.overlay.idle_update_hz,
        Activity::Hidden => cfg.overlay.hidden_update_hz,
    };
    let mode =
        UpdateMode::reactive_low_power(std::time::Duration::from_secs_f32(1.0 / hz.max(1.0)));
    // so escreve quando muda, para nao marcar o recurso como alterado a cada frame
    if settings.unfocused_mode != mode || settings.focused_mode != mode {
        settings.unfocused_mode = mode;
        settings.focused_mode = mode;
    }
}
