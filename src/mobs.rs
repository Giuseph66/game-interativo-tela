//! Varios mobs na tela: catalogo de tipos (aranha procedural + cada `.glb` em `assets/models/`),
//! comandos (spawnar, matar todos, remover todos, sair) e criacao das entidades.

use std::path::Path;

use bevy::gltf::Gltf;
use bevy::prelude::*;

use crate::config::Config;
use crate::creature::glb::{GlbKindData, PendingModel};
use crate::creature::spider::{Mob, SpiderSim};
use crate::overlay::EmergencyRes;

/// De onde vem o modelo de um tipo de mob.
#[derive(Debug, Clone, PartialEq)]
pub enum MobSource {
    /// Aranha feita de primitivas (sempre disponivel).
    Procedural,
    /// Caminho relativo a `assets/` (ex.: `models/Rat.glb`).
    Glb(String),
}

#[derive(Debug)]
pub struct MobKind {
    /// Nome exibido no painel.
    pub label: String,
    pub source: MobSource,
    /// Rotacao para a frente do modelo apontar para +X (graus).
    pub yaw_deg: f32,
    /// Multiplicador de escala apos o auto-fit.
    pub scale: f32,
    /// Carregado sob demanda no primeiro spawn deste tipo.
    pub handle: Option<Handle<Gltf>>,
    /// Grafo de animacao montado uma vez por tipo, compartilhado pelas instancias.
    pub glb: Option<GlbKindData>,
    /// O GLB falhou ao carregar (passa a usar a aranha procedural).
    pub failed: bool,
}

impl MobKind {
    pub fn procedural() -> Self {
        Self {
            label: "Aranha procedural".into(),
            source: MobSource::Procedural,
            yaw_deg: 0.0,
            scale: 1.0,
            handle: None,
            glb: None,
            failed: false,
        }
    }

    fn glb(label: String, path: String, yaw_deg: f32, scale: f32) -> Self {
        Self {
            label,
            source: MobSource::Glb(path),
            yaw_deg,
            scale,
            handle: None,
            glb: None,
            failed: false,
        }
    }
}

#[derive(Resource)]
pub struct MobCatalog {
    pub kinds: Vec<MobKind>,
    /// Tipo usado nos mobs iniciais.
    pub default_kind: usize,
    seed_base: u64,
    spawned: u64,
}

impl MobCatalog {
    pub fn new(kinds: Vec<MobKind>, default_kind: usize, seed: u64) -> Self {
        let seed_base = if seed != 0 {
            seed
        } else {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x5EED)
        };
        Self {
            default_kind: default_kind.min(kinds.len().saturating_sub(1)),
            kinds,
            seed_base,
            spawned: 0,
        }
    }

    /// Procura um tipo pelo rotulo (sem diferenciar caixa), pelo nome do arquivo ou pelo indice.
    pub fn find(&self, name: &str) -> Option<usize> {
        let n = name.trim().to_ascii_lowercase();
        if let Ok(i) = n.parse::<usize>() {
            return (i < self.kinds.len()).then_some(i);
        }
        self.kinds.iter().position(|k| {
            k.label.to_ascii_lowercase() == n
                || matches!(&k.source, MobSource::Glb(p) if Path::new(p)
                    .file_stem()
                    .is_some_and(|s| s.to_string_lossy().to_ascii_lowercase() == n))
                || (n == "procedural" && k.source == MobSource::Procedural)
        })
    }

    /// Semente distinta (e reprodutivel com `behavior.seed`) para cada mob criado.
    pub fn next_seed(&mut self) -> u64 {
        self.spawned += 1;
        self.seed_base
            .wrapping_add(self.spawned.wrapping_mul(0x9E37_79B9_7F4A_7C15))
            .max(1)
    }
}

/// Monta o catalogo: tipo 0 = aranha procedural; depois cada `assets/models/*.glb` (ordem
/// alfabetica). Yaw/escala vem do preset de `config/models/*.toml` cujo `spider.model` aponta
/// para o arquivo; se nao houver, do `[spider]` da configuracao (quando e o mesmo modelo) ou 0/1.
pub fn discover_kinds(
    asset_root: Option<&Path>,
    presets_dir: Option<&Path>,
    cfg: &Config,
) -> (Vec<MobKind>, usize) {
    let mut kinds = vec![MobKind::procedural()];

    // presets: model -> (rotulo, yaw, escala)
    let mut presets: Vec<(String, String, f32, f32)> = Vec::new();
    if let Some(dir) = presets_dir
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "toml") {
                continue;
            }
            match Config::load(&p) {
                Ok(c) => {
                    let label = p
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    presets.push((
                        c.spider.model.clone(),
                        label,
                        c.spider.glb_yaw_deg,
                        c.spider.glb_scale,
                    ));
                }
                Err(err) => warn!("preset {} ignorado: {err}", p.display()),
            }
        }
    }

    let mut files: Vec<String> = asset_root
        .and_then(|root| std::fs::read_dir(root.join("models")).ok())
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    name.to_ascii_lowercase().ends_with(".glb").then_some(name)
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort_by_key(|f| f.to_ascii_lowercase());

    let mut default_kind = 0;
    for file in files {
        let rel = format!("models/{file}");
        let stem = Path::new(&file)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| file.clone());
        let kind = if let Some((_, label, yaw, scale)) = presets.iter().find(|p| p.0 == rel) {
            MobKind::glb(label.clone(), rel.clone(), *yaw, *scale)
        } else if cfg.spider.model == rel {
            MobKind::glb(
                stem,
                rel.clone(),
                cfg.spider.glb_yaw_deg,
                cfg.spider.glb_scale,
            )
        } else {
            MobKind::glb(stem, rel.clone(), 0.0, 1.0)
        };
        if cfg.spider.model == rel {
            default_kind = kinds.len();
        }
        kinds.push(kind);
    }
    (kinds, default_kind)
}

/// Pedidos vindos do painel (ou de outros sistemas).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MobCommand {
    Spawn(usize),
    KillAll,
    RemoveAll,
    Quit,
}

/// Cria um mob do tipo `kind` (o modelo e anexado por `spawn_pending_models`).
pub fn spawn_mob(
    commands: &mut Commands,
    catalog: &mut MobCatalog,
    asset_server: &AssetServer,
    kind: usize,
) -> Option<Entity> {
    let seed = catalog.next_seed();
    let k = catalog.kinds.get_mut(kind)?;
    if let MobSource::Glb(path) = &k.source
        && k.handle.is_none()
        && !k.failed
    {
        k.handle = Some(asset_server.load(path.clone()));
    }
    Some(
        commands
            .spawn((
                Mob {
                    sim: SpiderSim::new(seed),
                    kind,
                },
                PendingModel,
                Transform::default(),
                Visibility::Hidden,
            ))
            .id(),
    )
}

/// Mobs iniciais (`mobs.initial`, tipo padrao).
pub fn spawn_initial_mobs(
    mut commands: Commands,
    cfg: Res<Config>,
    opts: Res<crate::overlay::RunOptions>,
    asset_server: Res<AssetServer>,
    mut catalog: ResMut<MobCatalog>,
) {
    if opts.poc {
        return;
    }
    let kind = catalog.default_kind;
    for _ in 0..cfg.mobs.initial {
        spawn_mob(&mut commands, &mut catalog, &asset_server, kind);
    }
    let mut extra = 0u32;
    for name in &opts.spawn {
        match catalog.find(name) {
            Some(k) if cfg.mobs.initial + extra < cfg.mobs.max => {
                spawn_mob(&mut commands, &mut catalog, &asset_server, k);
                extra += 1;
            }
            Some(_) => warn!("--spawn: limite de {} mobs atingido", cfg.mobs.max),
            None => warn!("--spawn: tipo \"{name}\" nao existe (veja --list-mobs)"),
        }
    }
    info!(
        "{} tipos de mob disponiveis; {} inicial(is) do tipo \"{}\"",
        catalog.kinds.len(),
        cfg.mobs.initial,
        catalog
            .kinds
            .get(kind)
            .map(|k| k.label.as_str())
            .unwrap_or("?")
    );
}

#[allow(clippy::too_many_arguments)]
pub fn apply_mob_commands(
    mut commands: Commands,
    cfg: Res<Config>,
    asset_server: Res<AssetServer>,
    em: Option<Res<EmergencyRes>>,
    mut catalog: ResMut<MobCatalog>,
    mut reader: MessageReader<MobCommand>,
    mut mobs: Query<(Entity, &mut Mob)>,
    mut exit: MessageWriter<AppExit>,
) {
    for cmd in reader.read() {
        match *cmd {
            MobCommand::Spawn(kind) => {
                let count = mobs.iter().count() as u32;
                if count >= cfg.mobs.max {
                    warn!("limite de {} mobs atingido (mobs.max)", cfg.mobs.max);
                    continue;
                }
                let label = catalog
                    .kinds
                    .get(kind)
                    .map(|k| k.label.clone())
                    .unwrap_or_default();
                if spawn_mob(&mut commands, &mut catalog, &asset_server, kind).is_some() {
                    info!("spawn: {label} ({} na tela)", count + 1);
                }
            }
            MobCommand::KillAll => {
                for (_, mut m) in &mut mobs {
                    if m.sim.visible && m.sim.brain.state().is_alive_free() {
                        m.sim.clicked = true;
                    }
                }
            }
            MobCommand::RemoveAll => {
                for (e, _) in &mobs {
                    commands.entity(e).despawn();
                }
                info!("todos os mobs removidos");
            }
            MobCommand::Quit => {
                info!("saida pedida pelo painel");
                if let Some(em) = &em {
                    em.0.request_exit();
                }
                exit.write(AppExit::Success);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("spiderdesk-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("models")).unwrap();
        d
    }

    #[test]
    fn catalog_lists_procedural_then_glbs_with_presets() {
        let assets = temp_dir("assets");
        for f in ["Rat.glb", "ant.glb", "notes.txt"] {
            std::fs::write(assets.join("models").join(f), b"x").unwrap();
        }
        let presets = temp_dir("presets");
        std::fs::write(
            presets.join("rato.toml"),
            "[spider]\nmodel = \"models/Rat.glb\"\nglb_yaw_deg = 90.0\nglb_scale = 1.5\n",
        )
        .unwrap();
        std::fs::write(presets.join("quebrado.toml"), "[spider\n").unwrap();

        let mut cfg = Config::default();
        cfg.spider.model = "models/ant.glb".into();
        cfg.spider.glb_yaw_deg = 45.0;
        let (kinds, default) = discover_kinds(Some(&assets), Some(&presets), &cfg);

        assert_eq!(kinds.len(), 3, "procedural + 2 glb (txt ignorado)");
        assert_eq!(kinds[0].source, MobSource::Procedural);
        // ordem alfabetica sem diferenciar caixa: ant, Rat
        assert_eq!(kinds[1].source, MobSource::Glb("models/ant.glb".into()));
        assert_eq!(
            kinds[1].yaw_deg, 45.0,
            "sem preset: usa [spider] quando e o mesmo modelo"
        );
        assert_eq!(kinds[2].label, "rato", "rotulo vem do nome do preset");
        assert_eq!((kinds[2].yaw_deg, kinds[2].scale), (90.0, 1.5));
        assert_eq!(default, 1, "spider.model existente vira o tipo padrao");
    }

    #[test]
    fn catalog_without_assets_has_only_procedural() {
        let (kinds, default) = discover_kinds(None, None, &Config::default());
        assert_eq!(kinds.len(), 1);
        assert_eq!(default, 0);
    }

    #[test]
    fn find_by_label_file_or_index() {
        let kinds = vec![
            MobKind::procedural(),
            MobKind::glb("rat".into(), "models/Rat.glb".into(), 90.0, 1.0),
            MobKind::glb(
                "wolf_spider".into(),
                "models/wolf_spider_rigged.glb".into(),
                -90.0,
                1.0,
            ),
        ];
        let c = MobCatalog::new(kinds, 0, 1);
        assert_eq!(c.find("RAT"), Some(1));
        assert_eq!(
            c.find("wolf_spider_rigged"),
            Some(2),
            "pelo nome do arquivo"
        );
        assert_eq!(c.find("procedural"), Some(0));
        assert_eq!(c.find("2"), Some(2));
        assert_eq!(c.find("9"), None);
        assert_eq!(c.find("dragao"), None);
    }

    #[test]
    fn seeds_are_distinct_and_reproducible() {
        let mut a = MobCatalog::new(vec![MobKind::procedural()], 0, 42);
        let mut b = MobCatalog::new(vec![MobKind::procedural()], 0, 42);
        let sa: Vec<u64> = (0..5).map(|_| a.next_seed()).collect();
        let sb: Vec<u64> = (0..5).map(|_| b.next_seed()).collect();
        assert_eq!(sa, sb);
        let mut uniq = sa.clone();
        uniq.dedup();
        assert_eq!(uniq.len(), 5);
        assert!(sa.iter().all(|&s| s != 0));
    }
}
