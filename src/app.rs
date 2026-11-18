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
use crate::desktop::{Desktop, poll_desktop};
use crate::diagnostics::{Metrics, log_metrics};
use crate::game::{NestField, Score};
use crate::mobs::{MobCatalog, MobCommand, apply_mob_commands, discover_kinds, spawn_initial_mobs};
use crate::navigation::geometry::Rect;
use crate::overlay::{OverlayGeometry, OverlayPlugin, OverlaySet, RunOptions, window};
use crate::{assets, nests, panel, platform, poc};

/// Argumentos de linha de comando.
#[derive(Debug, Default, PartialEq)]
pub struct Cli {
    pub config: Option<PathBuf>,
    /// `--backend`: sobrescreve `[overlay] backend` (diagnostico de GPU).
    pub backend: Option<BackendChoice>,
    pub list_windows: bool,
    pub timeout_secs: Option<f32>,
    pub poc: bool,
    pub screenshot: Option<PathBuf>,
    pub auto_kill_secs: Option<f32>,
    pub no_panel: bool,
    pub panel_selftest: bool,
    pub auto_hit_secs: Option<f32>,
    pub spawn: Vec<String>,
    pub list_mobs: bool,
    pub print_config: bool,
    pub help: bool,
    /// `--ledges entrada.png saida.png`: desenha os apoios que o heroi enxergaria na imagem.
    pub ledges: Option<(PathBuf, PathBuf)>,
    pub hero_demo: Option<String>,
}

pub const HELP: &str = "\
SpiderDesk - uma aranha 3D que vive sobre a area de trabalho do Windows

USO:
    spiderdesk [OPCOES]

OPCOES:
    --config <arquivo>   usa este TOML (padrao: config/default.toml, se existir)
    --poc                prova de conceito: cubo 3D transparente + click-through (sem aranha)
    --backend <b>        auto | dx12 | vulkan (sobrescreve [overlay] backend; diagnostico)
    --no-panel           nao abre o painel flutuante de spawn de mobs
    --spawn <a,b,...>    spawna mobs ao iniciar pelo nome (veja --list-mobs), ex.: rat,ant,rat
    --list-mobs          lista os tipos de mob disponiveis e sai
    --list-windows       imprime as janelas detectadas (bordas visiveis) e sai
    --hero-demo <seq>    teste: teclas programadas para o heroi, ex.: right:2,jump+right:0.5,attack
                         (teclas: left right up down jump run attack wait; `+` junta, `:` segundos)
    --ledges <in> <out>  depuracao: marca numa imagem PNG os apoios que o heroi enxerga e sai
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
            "--backend" => {
                cli.backend = Some(match it.next().as_deref() {
                    Some("auto") => BackendChoice::Auto,
                    Some("dx12") => BackendChoice::Dx12,
                    Some("vulkan") => BackendChoice::Vulkan,
                    _ => return Err("--backend exige auto, dx12 ou vulkan".into()),
                });
            }
            "--panel-selftest" => cli.panel_selftest = true,
            "--auto-hit" => {
                let v: f32 = it
                    .next()
                    .ok_or("--auto-hit exige segundos")?
                    .parse()
                    .map_err(|_| "--auto-hit invalido")?;
                if !(v.is_finite() && v > 0.0) {
                    return Err("--auto-hit deve ser > 0".into());
                }
                cli.auto_hit_secs = Some(v);
            }
            "--list-mobs" => cli.list_mobs = true,
            "--spawn" => {
                let v = it
                    .next()
                    .ok_or("--spawn exige nomes, ex.: --spawn rat,ant,rat")?;
                cli.spawn.extend(
                    v.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty()),
                );
            }
            "--list-windows" => cli.list_windows = true,
            "--hero-demo" => {
                let script = it
                    .next()
                    .ok_or("--hero-demo exige uma sequencia, ex.: right:2,jump+right:0.5,attack")?;
                crate::hero::Script::parse(&script)?;
                cli.hero_demo = Some(script);
            }
            "--ledges" => {
                let msg = "--ledges exige dois caminhos: entrada.png saida.png";
                let input = PathBuf::from(it.next().ok_or(msg)?);
                let output = PathBuf::from(it.next().ok_or(msg)?);
                cli.ledges = Some((input, output));
            }
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
    let mut cfg = cfg;
    if let Some(b) = cli.backend {
        cfg.overlay.backend = b;
    }
    install_panic_log();

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
    if let Some((input, output)) = &cli.ledges {
        return match draw_ledges_file(input, output) {
            Ok(n) => {
                println!("{n} apoios; imagem marcada em {}", output.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("erro: {e}");
                ExitCode::FAILURE
            }
        };
    }
    if cli.list_mobs {
        let root = assets::asset_root(&cfg.spider.model);
        let presets =
            assets::find_default_config().and_then(|p| p.parent().map(|d| d.join("models")));
        let (kinds, default) = discover_kinds(root.as_deref(), presets.as_deref(), &cfg);
        println!("tipos de mob (use o nome em --spawn):");
        for (i, k) in kinds.iter().enumerate() {
            let src = match &k.source {
                crate::mobs::MobSource::Procedural(sp) => {
                    format!("procedural: {}", sp.aliases().join(", "))
                }
                crate::mobs::MobSource::Glb(p) => p.clone(),
            };
            let mark = if i == default { "  (padrao)" } else { "" };
            println!("  {i}: {}  [{src}, yaw {}]{mark}", k.label, k.yaw_deg);
        }
        return ExitCode::SUCCESS;
    }

    let opts = RunOptions {
        timeout_secs: cli.timeout_secs,
        poc: cli.poc,
        screenshot: cli.screenshot,
        auto_kill_secs: cli.auto_kill_secs,
        no_panel: cli.no_panel,
        panel_selftest: cli.panel_selftest,
        auto_hit_secs: cli.auto_hit_secs,
        spawn: cli.spawn,
        hero_demo: cli.hero_demo,
    };
    let exit = build_app(cfg, opts).run();
    platform::shutdown();
    match exit {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(code) => ExitCode::from(code.get()),
    }
}

/// `--list-windows`: diagnostico sem janela/GPU (util para validar a enumeracao no Windows).
/// Le um PNG, encontra os apoios (o que o heroi enxerga dentro de um aplicativo) e salva a imagem
/// com eles marcados: verde = borda continua, laranja = topo de texto.
fn draw_ledges_file(input: &std::path::Path, output: &std::path::Path) -> Result<usize, String> {
    use crate::navigation::vision;
    let img = image::open(input).map_err(|e| e.to_string())?.to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut rgba = img.into_raw();
    let luma = vision::luma_from_rgba(&rgba);
    let t0 = std::time::Instant::now();
    let ledges = vision::find_ledges(&luma, w, h, &vision::VisionParams::default());
    println!(
        "{w}x{h}: analise em {:.1} ms",
        t0.elapsed().as_secs_f32() * 1000.0
    );
    vision::draw_ledges(&mut rgba, w, h, &ledges);
    image::save_buffer(output, &rgba, w as u32, h as u32, image::ColorType::Rgba8)
        .map_err(|e| e.to_string())?;
    Ok(ledges.len())
}

fn list_windows() {
    let screen = Rect::from_xywh(0.0, 0.0, 1920.0, 1080.0);
    let windows = platform::list_windows(screen, platform::current_pid());
    println!("fonte: {}", platform::window_source());
    for (i, w) in platform::work_areas(screen).iter().enumerate() {
        println!(
            "area util {i}: ({:.0},{:.0} {:.0}x{:.0})",
            w.min.x,
            w.min.y,
            w.width(),
            w.height()
        );
    }
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

/// Panico tambem vai para o log (no Windows, o `spiderdesk-log.txt`): o console fecha junto com
/// o processo e a mensagem padrao (stderr) se perderia.
fn install_panic_log() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        error!(
            "PANICO na thread {}: {info}",
            thread.name().unwrap_or("<sem nome>")
        );
        previous(info);
    }));
}

/// Windows: grava tambem o log em `spiderdesk-log.txt` ao lado do executavel (ou na pasta
/// temporaria, se ali nao der para escrever). Assim quem testa so precisa mandar esse arquivo,
/// mesmo abrindo o `.exe` com duplo clique. Nos outros sistemas o terminal basta.
fn log_file_layer(_app: &mut App) -> Option<bevy::log::BoxedLayer> {
    #[cfg(windows)]
    {
        use bevy::log::tracing_subscriber::{Layer, fmt};
        const NAME: &str = "spiderdesk-log.txt";
        let next_to_exe = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join(NAME)));
        for path in next_to_exe
            .into_iter()
            .chain(std::iter::once(std::env::temp_dir().join(NAME)))
        {
            if let Ok(file) = std::fs::File::create(&path) {
                eprintln!("log tambem em {}", path.display());
                return Some(
                    fmt::layer()
                        .with_ansi(false)
                        .with_writer(std::sync::Mutex::new(file))
                        .boxed(),
                );
            }
        }
        None
    }
    #[cfg(not(windows))]
    {
        None
    }
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
                filter: "info,wgpu=error,naga=warn,bevy_winit=warn,bevy_render=info,bevy_render::view=warn".into(),
                custom_layer: log_file_layer,
                ..default()
            }),
    );

    {
        use crate::platform::breadcrumbs::{
            self, EXTRACTED, MAIN_FINISHED, MAIN_STARTED, RENDERED,
        };
        app.add_systems(First, || breadcrumbs::bump(&MAIN_STARTED));
        app.add_systems(Last, || breadcrumbs::bump(&MAIN_FINISHED));
        if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render_app.add_systems(bevy::render::ExtractSchedule, || {
                breadcrumbs::bump(&EXTRACTED)
            });
            render_app.add_systems(bevy::render::Render, || breadcrumbs::bump(&RENDERED));
        }
    }
    {
        let backend = cfg.overlay.backend;
        let poc = opts.poc;
        app.add_systems(Startup, move || {
            info!(
                "SpiderDesk {} em {}/{}; backend pedido: {:?}; modo PoC: {}",
                env!("CARGO_PKG_VERSION"),
                std::env::consts::OS,
                std::env::consts::ARCH,
                backend,
                poc
            );
        });
    }
    let seed = cfg.behavior.seed;
    let heist = crate::web::HeistState::new(seed, &cfg);
    let hero = crate::hero::HeroState::new(seed, &cfg, &opts);
    let presets_dir =
        assets::find_default_config().and_then(|p| p.parent().map(|d| d.join("models")));
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
        .add_message::<crate::hud::HitFx>()
        .add_message::<crate::hud::FloatText>()
        .insert_resource(NestField::new(seed))
        .insert_resource(heist)
        .init_resource::<crate::web::HeistHud>()
        .init_resource::<panel::PanelNotice>()
        .insert_resource(hero)
        .init_resource::<crate::hero::HeroHud>()
        .init_resource::<crate::game::Progress>()
        .add_systems(
            Startup,
            (
                setup_scene,
                spawn_initial_mobs,
                panel::spawn_panel,
                crate::hud::setup_hud,
            )
                .chain(),
        )
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
                crate::hud::spawn_hit_fx,
                crate::hud::animate_fx,
                crate::hud::update_health_bars,
                crate::hud::update_sword_cursor,
            )
                .chain()
                .in_set(OverlaySet::Game),
        )
        // ninhos e sacudida: os golpes em ninho sao decididos antes dos golpes em mob; o resto
        // roda depois da simulacao dos mobs e antes da animacao dos textos da interface
        .add_systems(
            Startup,
            (
                nests::setup_nests,
                crate::web::setup_web,
                crate::hero::setup_hero,
            ),
        )
        .add_systems(
            Update,
            (nests::apply_hatchlings, nests::nest_clicks)
                .chain()
                .after(spawn_pending_models)
                .before(read_click)
                .in_set(OverlaySet::Game),
        )
        .add_systems(
            Update,
            (
                nests::nest_update,
                nests::sync_nest_visuals,
                nests::update_nest_bars,
                // heroi: mundo, teclado, combate, itens, desenho
                (
                    crate::hero::hero_world,
                    crate::hero::hero_control,
                    crate::hero::hero_combat,
                    crate::hero::hero_loot,
                    crate::hero::sync_hero_visuals,
                    crate::hero::hero_hud,
                )
                    .chain(),
                // sequestro do mouse: fio, teia do canto e cursor real arrastado
                crate::web::heist_update,
                crate::web::sync_web_visuals,
                crate::web::sync_bridge_threads,
                crate::hud::spawn_float_text,
            )
                .chain()
                .after(spider_think)
                .before(crate::hud::animate_fx)
                .in_set(OverlaySet::Game),
        )
        .add_systems(
            Update,
            (
                panel::fit_panel,
                panel::update_status,
                panel::button_colors,
                panel::panel_selftest,
            )
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
