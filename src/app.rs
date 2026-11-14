//! Montagem do aplicativo Bevy e interface de linha de comando.

use std::path::PathBuf;
use std::process::ExitCode;

use bevy::camera::{OrthographicProjection, ScalingMode};
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::settings::{Backends, RenderCreation, WgpuSettings};
use bevy::render::view::{DebandDither, Tonemapping};
use bevy::winit::{UpdateMode, WinitSettings};

use crate::capture::capture_if_requested;
use crate::config::{BackendChoice, Config};
use crate::creature::effects::{EffectAssets, EffectsActive, update_particles};
use crate::creature::glb::{drive_glb_animation, fit_glb_model, spawn_pending_models};
use crate::creature::procedural::{ProceduralAssets, animate_procedural};
use crate::creature::spider::{
    OverlayCamera, read_click, spawn_death_effect, spider_think, sync_spider_transform,
};
use crate::mobs::{MobCatalog, MobCommand, apply_mob_commands, discover_kinds, spawn_initial_mobs};
use crate::desktop::{Desktop, poll_desktop};
use crate::diagnostics::{Metrics, log_metrics};
use crate::game::Score;
use crate::navigation::geometry::Rect;
use crate::overlay::{OverlayGeometry, OverlayPlugin, OverlaySet, RunOptions, window};
use crate::{assets, panel, platform, poc};

/// Argumentos de linha de comando.
#[derive(Debug, Default, PartialEq)]
pub struct Cli {
    pub config: Option<PathBuf>,
    pub list_windows: bool,
    pub timeout_secs: Option<f32>,
    pub poc: bool,
    pub screenshot: Option<PathBuf>,
    pub auto_kill_secs: Option<f32>,
    pub no_panel: bool,
    pub print_config: bool,
    pub help: bool,
}

pub const HELP: &str = "\
SpiderDesk - uma aranha 3D que vive sobre a area de trabalho do Windows

USO:
    spiderdesk [OPCOES]

OPCOES:
    --config <arquivo>   usa este TOML (padrao: config/default.toml, se existir)
    --poc                prova de conceito: cubo 3D transparente + click-through (sem aranha)
    --no-panel           nao abre o painel flutuante de spawn de mobs
    --list-windows       imprime as janelas detectadas (bordas visiveis) e sai
    --timeout <segs>     encerra sozinho apos N segundos (testes)
    --screenshot <png>   salva um quadro renderizado (RGBA) com estatisticas de alfa e sai
    --auto-kill <segs>   teste: simula um clique na aranha apos N s (e a cada reaparecimento)
    --print-config       imprime a configuracao efetiva e sai
    -h, --help           esta ajuda

ENCERRAMENTO DE EMERGENCIA (independente da aranha):
    Ctrl+Alt+Shift+Q     (configuravel em [overlay].emergency_hotkey)
";

pub fn parse_args<I: IntoIterator<Item = String>>(args: I) -> Result<Cli, String> {
    let mut cli = Cli::default();
    let mut it = args.into_iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--config" => {
                cli.config = Some(PathBuf::from(it.next().ok_or("--config exige um caminho")?));
            }
            "--timeout" => {
                let v: f32 = it
                    .next()
                    .ok_or("--timeout exige um numero de segundos")?
                    .parse()
                    .map_err(|_| "--timeout invalido")?;
                if !(v.is_finite() && v > 0.0) {
                    return Err("--timeout deve ser > 0".into());
                }
                cli.timeout_secs = Some(v);
            }
            "--screenshot" => {
                cli.screenshot = Some(PathBuf::from(
                    it.next().ok_or("--screenshot exige um caminho .png")?,
                ));
            }
            "--auto-kill" => {
                let v: f32 = it
                    .next()
                    .ok_or("--auto-kill exige um numero de segundos")?
                    .parse()
                    .map_err(|_| "--auto-kill invalido")?;
                if !(v.is_finite() && v > 0.0) {
                    return Err("--auto-kill deve ser > 0".into());
                }
                cli.auto_kill_secs = Some(v);
            }
            "--poc" => cli.poc = true,
            "--no-panel" => cli.no_panel = true,
            "--list-windows" => cli.list_windows = true,
            "--print-config" => cli.print_config = true,
            "-h" | "--help" => cli.help = true,
            other => return Err(format!("argumento desconhecido: {other}")),
        }
    }
    Ok(cli)
}

pub fn run_from_env() -> ExitCode {
    match parse_args(std::env::args().skip(1)) {
        Ok(cli) => run(cli),
        Err(e) => {
            eprintln!("erro: {e}\n\n{HELP}");
            ExitCode::from(64)
        }
    }
}

pub fn run(cli: Cli) -> ExitCode {
    if cli.help {
        print!("{HELP}");
        return ExitCode::SUCCESS;
    }
    // coordenadas fisicas (DPI) antes de qualquer consulta/janela
    platform::init_process();

    let (cfg, warning) = match &cli.config {
        Some(p) => match Config::load(p) {
            Ok(c) => (c, None),
            Err(e) => {
                eprintln!("erro ao carregar {}: {e}", p.display());
                return ExitCode::from(78);
            }
        },
        None => match assets::find_default_config() {
            Some(p) => Config::load_or_default(&p),
            None => (Config::default(), None),
        },
    };
    if let Some(w) = &warning {
        eprintln!("aviso: {w}");
    }

    if cli.print_config {
        match toml::to_string_pretty(&cfg) {
            Ok(t) => print!("{t}"),
            Err(e) => {
                eprintln!("erro ao serializar configuracao: {e}");
                return ExitCode::FAILURE;
            }
        }
        return ExitCode::SUCCESS;
    }
    if cli.list_windows {
        list_windows();
        return ExitCode::SUCCESS;
    }

    let opts = RunOptions {
        timeout_secs: cli.timeout_secs,
        poc: cli.poc,
        screenshot: cli.screenshot,
        auto_kill_secs: cli.auto_kill_secs,
        no_panel: cli.no_panel,
    };
    let exit = build_app(cfg, opts).run();
    match exit {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(code) => ExitCode::from(code.get()),
    }
}

/// `--list-windows`: diagnostico sem janela/GPU (util para validar a enumeracao no Windows).
fn list_windows() {
    let screen = Rect::from_xywh(0.0, 0.0, 1920.0, 1080.0);
    let windows = platform::list_windows(screen, platform::current_pid());
    println!("{} janelas (da frente para o fundo):", windows.len());
    for w in &windows {
        println!(
            "  hwnd=0x{:X} pid={} {:?} visivel=({:.0},{:.0} {:.0}x{:.0}) bruto=({:.0},{:.0} {:.0}x{:.0}) \"{}\"",
            w.id,
            w.pid,
            w.state,
            w.rect.min.x,
            w.rect.min.y,
            w.rect.width(),
            w.rect.height(),
            w.raw_rect.min.x,
            w.raw_rect.min.y,
            w.raw_rect.width(),
            w.raw_rect.height(),
            w.title
        );
    }
}

fn wgpu_settings(cfg: &Config) -> WgpuSettings {
    let mut s = WgpuSettings::default();
    s.backends = match cfg.overlay.backend {
        BackendChoice::Dx12 => Some(Backends::DX12),
        BackendChoice::Vulkan => Some(Backends::VULKAN),
        // Windows: so DX12 (via DirectComposition) entrega transparencia confiavel
        BackendChoice::Auto if cfg!(windows) => Some(Backends::DX12),
        BackendChoice::Auto => s.backends,
    };
    s
}

/// Monta o aplicativo completo (sem executar).
pub fn build_app(cfg: Config, opts: RunOptions) -> App {
    platform::prepare_graphics_env();
    let asset_root = assets::asset_root(&cfg.spider.model)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "assets".to_string());

    let mut app = App::new();
    app.add_plugins(
        DefaultPlugins
            .set(window::window_plugin(&cfg))
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(Box::new(wgpu_settings(&cfg))),
                ..default()
            })
            .set(AssetPlugin {
                file_path: asset_root.clone(),
                ..default()
            })
            .set(LogPlugin {
                filter: "info,wgpu=error,naga=warn,bevy_winit=warn,bevy_render=warn".into(),
                ..default()
            }),
    );

    let seed = cfg.behavior.seed;
    let presets_dir = assets::find_default_config()
        .and_then(|p| p.parent().map(|d| d.join("models")));
    let (kinds, default_kind) = discover_kinds(
        Some(std::path::Path::new(&asset_root)),
        presets_dir.as_deref(),
        &cfg,
    );
    let catalog = MobCatalog::new(kinds, default_kind, seed);
    let poc_mode = opts.poc;
    let fps = cfg.overlay.fps_limit as f32;

    app.insert_resource(ClearColor(Color::NONE))
        .insert_resource(GlobalAmbientLight {
            color: Color::WHITE,
            brightness: 650.0,
            ..default()
        })
        .insert_resource(WinitSettings {
            focused_mode: UpdateMode::reactive_low_power(std::time::Duration::from_secs_f32(
                1.0 / fps,
            )),
            unfocused_mode: UpdateMode::reactive_low_power(std::time::Duration::from_secs_f32(
                1.0 / fps,
            )),
        })
        .insert_resource(crate::overlay::BaseConfig(cfg.clone()))
        .insert_resource(cfg)
        .insert_resource(opts)
        .insert_resource(Desktop::default())
        .insert_resource(catalog)
        .insert_resource(Score::default())
        .insert_resource(EffectsActive::default())
        .insert_resource(Metrics::default())
        .add_plugins(OverlayPlugin)
        .add_message::<MobCommand>()
        .add_systems(Startup, (setup_scene, spawn_initial_mobs, panel::spawn_panel).chain())
        .add_systems(
            Update,
            (
                update_camera_projection,
                update_particles,
                log_metrics,
                capture_if_requested,
            )
                .in_set(OverlaySet::Game),
        );

    if poc_mode {
        app.add_systems(Startup, poc::setup_poc)
            .add_systems(Update, poc::update_poc.in_set(OverlaySet::Game));
    } else {
        app.add_systems(
            Update,
            (
                poll_desktop,
                apply_mob_commands,
                spawn_pending_models,
                read_click,
                spider_think,
                sync_spider_transform,
                spawn_death_effect,
                animate_procedural,
                fit_glb_model,
                drive_glb_animation,
            )
                .chain()
                .in_set(OverlaySet::Game),
        )
        .add_systems(
            Update,
            (panel::fit_panel, panel::update_status, panel::button_colors)
                .in_set(OverlaySet::Game),
        );
    }
    app
}

fn setup_scene(
    mut commands: Commands,
    cfg: Res<Config>,
    opts: Res<RunOptions>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // camera ortografica: 1 unidade de mundo = 1 pixel fisico (tamanho ajustado quando a janela fica pronta)
    let msaa = match cfg.overlay.msaa_samples {
        4 => Msaa::Sample4,
        2 => Msaa::Sample2,
        _ => Msaa::Off,
    };
    commands.spawn((
        OverlayCamera,
        Camera3d::default(),
        Camera {
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        Tonemapping::None,
        DebandDither::Disabled,
        msaa,
        ortho_projection(Vec2::new(1280.0, 720.0)),
        Transform::from_xyz(0.0, 0.0, 1000.0),
    ));

    // luz principal de cima/esquerda/frente; sem sombras (custo de GPU)
    commands.spawn((
        DirectionalLight {
            illuminance: 11_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::from_xyz(-0.4, 0.8, 1.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    commands.insert_resource(EffectAssets::new(&mut meshes, &mut materials));
    commands.insert_resource(ProceduralAssets::new(&mut meshes, &mut materials));
    if opts.poc {
        info!("modo PoC: cubo 3D (sem mobs)");
    }
}

fn ortho_projection(size: Vec2) -> Projection {
    Projection::Orthographic(OrthographicProjection {
        near: 0.0,
        far: 2000.0,
        scaling_mode: ScalingMode::Fixed {
            width: size.x,
            height: size.y,
        },
        ..OrthographicProjection::default_3d()
    })
}

/// Mantem a projecao 1:1 com os pixels fisicos do overlay.
fn update_camera_projection(
    geo: Res<OverlayGeometry>,
    mut cameras: Query<&mut Projection, With<OverlayCamera>>,
) {
    if !geo.is_changed() {
        return;
    }
    for mut p in &mut cameras {
        *p = ortho_projection(geo.size_f32());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_flags_and_values() {
        let cli = parse_args(args(&["--poc", "--timeout", "12.5", "--config", "a.toml"])).unwrap();
        assert!(cli.poc);
        assert_eq!(cli.timeout_secs, Some(12.5));
        assert_eq!(cli.config, Some(PathBuf::from("a.toml")));
        assert!(!cli.list_windows);
        assert_eq!(parse_args(args(&[])).unwrap(), Cli::default());
    }

    #[test]
    fn rejects_bad_arguments() {
        assert!(parse_args(args(&["--nope"])).is_err());
        assert!(parse_args(args(&["--timeout"])).is_err());
        assert!(parse_args(args(&["--timeout", "abc"])).is_err());
        assert!(parse_args(args(&["--timeout", "-3"])).is_err());
        assert!(parse_args(args(&["--timeout", "NaN"])).is_err());
        assert!(parse_args(args(&["--config"])).is_err());
    }
}
