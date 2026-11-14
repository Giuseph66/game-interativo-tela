//! Varios mobs na tela: catalogo de tipos (bichos procedurais + opcionalmente cada `.glb` em
//! `assets/models/`), comandos (spawnar, matar todos, remover todos, sair) e criacao das entidades.

use std::path::Path;

use bevy::gltf::Gltf;
use bevy::prelude::*;

use crate::config::Config;
use crate::creature::glb::{GlbKindData, PendingModel};
use crate::creature::species::{Profile, Species};
use crate::creature::spider::{Mob, SpiderSim};
use crate::overlay::EmergencyRes;

/// De onde vem o modelo de um tipo de mob.
#[derive(Debug, Clone, PartialEq)]
pub enum MobSource {
    /// Bicho feito de primitivas com animacao procedural (sempre disponivel).
    Procedural(Species),
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
    pub fn procedural(species: Species) -> Self {
        Self {
            label: species.label().into(),
            source: MobSource::Procedural(species),
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
        // rotulo exato primeiro; depois nome do arquivo GLB; por ultimo apelidos da especie
        let by_label = |k: &MobKind| k.label.to_ascii_lowercase() == n;
        let by_file = |k: &MobKind| {
            matches!(&k.source, MobSource::Glb(p) if Path::new(p)
                .file_stem()
                .is_some_and(|s| s.to_string_lossy().to_ascii_lowercase() == n))
        };
        let by_alias = |k: &MobKind| matches!(k.source, MobSource::Procedural(sp) if Species::from_name(&n) == Some(sp));
        self.kinds
            .iter()
            .position(by_label)
            .or_else(|| self.kinds.iter().position(by_file))
            .or_else(|| self.kinds.iter().position(by_alias))
    }

    /// Semente distinta (e reprodutivel com `behavior.seed`) para cada mob criado.
    pub fn next_seed(&mut self) -> u64 {
        self.spawned += 1;
        self.seed_base
            .wrapping_add(self.spawned.wrapping_mul(0x9E37_79B9_7F4A_7C15))
            .max(1)
    }
}

/// Nomes de `mobs.initial_kind` que sorteiam uma especie para cada mob inicial.
pub const MIXED_KIND: [&str; 4] = ["misto", "mix", "aleatorio", "random"];

/// Monta o catalogo: primeiro os bichos procedurais ([`Species::ALL`]); com `mobs.include_glb`,
/// depois cada `assets/models/*.glb` (ordem alfabetica). Yaw/escala do GLB vem do preset de
/// `config/models/*.toml` cujo `spider.model` aponta para o arquivo; se nao houver, do `[spider]`
/// da configuracao (quando e o mesmo modelo) ou 0/1. O tipo padrao e `mobs.initial_kind`.
pub fn discover_kinds(
    asset_root: Option<&Path>,
    presets_dir: Option<&Path>,
    cfg: &Config,
) -> (Vec<MobKind>, usize) {
    let mut kinds: Vec<MobKind> = Species::ALL.into_iter().map(MobKind::procedural).collect();
    let mut default_kind = Species::from_name(&cfg.mobs.initial_kind)
        .map(Species::index)
        .unwrap_or(0);
    if !cfg.mobs.include_glb {
        return (kinds, default_kind);
    }

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
        if cfg.spider.model == rel && Species::from_name(&cfg.mobs.initial_kind).is_none() {
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
    let profile = match k.source {
        MobSource::Procedural(sp) => {
            // cada individuo varia ate 8% de tamanho (o hitbox acompanha)
            let mut p = sp.profile();
            p.size *= 0.92 + 0.16 * ((seed >> 11) % 1000) as f32 / 1000.0;
            p
        }
        MobSource::Glb(_) => Profile::default(),
    };
    Some(
        commands
            .spawn((
                Mob {
                    sim: SpiderSim::with_profile(seed, profile),
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
    let mixed = MIXED_KIND.contains(&cfg.mobs.initial_kind.trim().to_ascii_lowercase().as_str());
    for i in 0..cfg.mobs.initial {
        let k = if mixed {
            let n = Species::ALL.len() as u64;
            ((catalog.seed_base.wrapping_add(i as u64 * 7)) % n) as usize
        } else {
            kind
        };
        spawn_mob(&mut commands, &mut catalog, &asset_server, k);
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
        if mixed {
            "misto"
        } else {
            catalog
                .kinds
                .get(kind)
                .map(|k| k.label.as_str())
                .unwrap_or("?")
        }
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

    const N: usize = Species::ALL.len();

    #[test]
    fn catalog_lists_species_then_glbs_with_presets() {
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
        cfg.mobs.include_glb = true;
        cfg.mobs.initial_kind = String::new();
        cfg.spider.model = "models/ant.glb".into();
        cfg.spider.glb_yaw_deg = 45.0;
        let (kinds, default) = discover_kinds(Some(&assets), Some(&presets), &cfg);

        assert_eq!(kinds.len(), N + 2, "especies + 2 glb (txt ignorado)");
        for (k, s) in kinds.iter().zip(Species::ALL) {
            assert_eq!(k.source, MobSource::Procedural(s));
            assert_eq!(k.label, s.label());
        }
        // ordem alfabetica sem diferenciar caixa: ant, Rat
        assert_eq!(kinds[N].source, MobSource::Glb("models/ant.glb".into()));
        assert_eq!(
            kinds[N].yaw_deg, 45.0,
            "sem preset: usa [spider] quando e o mesmo modelo"
        );
        assert_eq!(kinds[N + 1].label, "rato", "rotulo vem do nome do preset");
        assert_eq!((kinds[N + 1].yaw_deg, kinds[N + 1].scale), (90.0, 1.5));
        assert_eq!(default, N, "spider.model existente vira o tipo padrao");

        // initial_kind com nome de especie tem prioridade
        cfg.mobs.initial_kind = "barata".into();
        let (_, default) = discover_kinds(Some(&assets), Some(&presets), &cfg);
        assert_eq!(default, Species::Barata.index());
    }

    #[test]
    fn glbs_are_ignored_by_default() {
        let assets = temp_dir("assets-default");
        std::fs::write(assets.join("models").join("Rat.glb"), b"x").unwrap();
        let (kinds, default) = discover_kinds(Some(&assets), None, &Config::default());
        assert_eq!(kinds.len(), N);
        assert_eq!(default, Species::Viuva.index());
    }

    #[test]
    fn find_by_label_alias_file_or_index() {
        let mut kinds: Vec<MobKind> = Species::ALL.into_iter().map(MobKind::procedural).collect();
        kinds.push(MobKind::glb(
            "wolf_spider".into(),
            "models/wolf_spider_rigged.glb".into(),
            -90.0,
            1.0,
        ));
        let c = MobCatalog::new(kinds, 0, 1);
        assert_eq!(c.find("RATO"), Some(Species::Rato.index()));
        assert_eq!(c.find("rat"), Some(Species::Rato.index()), "alias");
        assert_eq!(c.find("Aranha-lobo"), Some(Species::AranhaLobo.index()));
        assert_eq!(c.find("procedural"), Some(Species::Viuva.index()));
        assert_eq!(
            c.find("wolf_spider_rigged"),
            Some(N),
            "pelo nome do arquivo"
        );
        assert_eq!(
            c.find("wolf_spider"),
            Some(N),
            "rotulo do GLB vence o alias"
        );
        assert_eq!(c.find("2"), Some(2));
        assert_eq!(c.find("99"), None);
        assert_eq!(c.find("dragao"), None);
    }

    #[test]
    fn seeds_are_distinct_and_reproducible() {
        let mut a = MobCatalog::new(vec![MobKind::procedural(Species::Viuva)], 0, 42);
        let mut b = MobCatalog::new(vec![MobKind::procedural(Species::Viuva)], 0, 42);
        let sa: Vec<u64> = (0..5).map(|_| a.next_seed()).collect();
        let sb: Vec<u64> = (0..5).map(|_| b.next_seed()).collect();
        assert_eq!(sa, sb);
        let mut uniq = sa.clone();
        uniq.dedup();
        assert_eq!(uniq.len(), 5);
        assert!(sa.iter().all(|&s| s != 0));
    }
}
