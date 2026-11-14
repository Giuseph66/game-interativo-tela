//! O heroi no ECS: teclado, mundo (apoios e escadas a partir das janelas e do que a visao acha
//! dentro delas), combate com os bichos, coleta, desenho do personagem e interface.
//! As regras estao em [`crate::game::hero`].

use std::collections::HashSet;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::config::{Config, HeroConfig};
use crate::creature::effects::{EffectAssets, spawn_burst};
use crate::creature::glb::PendingModel;
use crate::creature::spider::Mob;
use crate::desktop::Desktop;
use crate::game::hero::{
    Ground, Hero, HeroInput, Loot, Pickup, PickupKind, Platform, SWING_SECS, build_world,
};
use crate::game::{NestField, Progress, Sword};
use crate::hud::{FloatText, HitFx, HudRoot, sword_color};
use crate::navigation::geometry::{Rect, Segment};
use crate::overlay::{Activity, OverlayGeometry, RunOptions};
use crate::platform::{self, DesktopWindow};
use crate::rng::Rng;

/// Na frente dos bichos (z ~ 0), dos ninhos e das teias.
const HERO_Z: f32 = 40.0;
const LOOT_Z: f32 = 30.0;
/// Altura do modelo em unidades proprias (pes em y = 0).
const MODEL_H: f32 = 1.42;
const COLOR_GOOD: Color = Color::srgb(0.55, 1.0, 0.6);
const COLOR_BAD: Color = Color::srgb(1.0, 0.45, 0.35);
const COLOR_GOLD: Color = Color::srgb(1.0, 0.86, 0.3);
const COLOR_INFO: Color = Color::srgb(0.85, 0.92, 1.0);
/// Tempo (s) que o nome do lugar fica acima do heroi depois de pousar.
const LABEL_SECS: f32 = 2.6;

/// Sequencia de teclas programada (`--hero-demo`): testes e demonstracao sem teclado.
#[derive(Debug, Clone, Default)]
pub struct Script {
    steps: Vec<(HeroInput, f32)>,
    at: usize,
    t: f32,
}

impl Script {
    /// `"right:1.5,jump+right:0.4,wait:1,attack"`: teclas unidas por `+`, duracao em segundos
    /// depois de `:` (padrao 0,12). Teclas: left right up down jump run sneak attack wait
    /// (ou a/d/w/s, ctrl = run, shift = sneak, space/j = attack, como no teclado).
    pub fn parse(text: &str) -> Result<Script, String> {
        let mut steps = Vec::new();
        for token in text.split(',').map(str::trim).filter(|t| !t.is_empty()) {
            let (keys, secs) = token.split_once(':').unwrap_or((token, "0.12"));
            let secs: f32 = secs
                .trim()
                .parse()
                .map_err(|_| format!("duracao invalida em {token:?}"))?;
            let mut inp = HeroInput::default();
            for k in keys.split('+').map(|k| k.trim().to_ascii_lowercase()) {
                match k.as_str() {
                    "left" | "a" => inp.left = true,
                    "right" | "d" => inp.right = true,
                    "up" | "w" => inp.up = true,
                    "down" | "s" => inp.down = true,
                    "jump" => inp.jump = true,
                    "run" | "ctrl" => inp.run = true,
                    "sneak" | "shift" => inp.sneak = true,
                    "attack" | "space" | "j" => inp.attack = true,
                    "wait" | "" => {}
                    other => return Err(format!("tecla desconhecida {other:?} em {token:?}")),
                }
            }
            steps.push((inp, secs.clamp(0.0, 600.0)));
        }
        Ok(Script {
            steps,
            at: 0,
            t: 0.0,
        })
    }

    /// Teclas deste quadro; `None` quando a sequencia acabou.
    fn next(&mut self, dt: f32) -> Option<HeroInput> {
        loop {
            let (inp, secs) = *self.steps.get(self.at)?;
            if self.t < secs {
                self.t += dt;
                return Some(inp);
            }
            // um quadro de teclas soltas entre passos (para "apertar de novo" contar)
            self.t = 0.0;
            self.at += 1;
            if self.at < self.steps.len() {
                return Some(HeroInput::default());
            }
        }
    }
}

#[derive(Resource)]
pub struct HeroState {
    pub hero: Hero,
    pub loot: Loot,
    pub enabled: bool,
    pub platforms: Vec<Platform>,
    pub ladders: Vec<Segment>,
    /// (versao do desktop, versao da visao, tamanho da tela) do mundo montado.
    world_key: Option<(u64, u64, UVec2)>,
    /// (apoios, apoios dentro de apps, escadas) do ultimo mundo, para o log.
    world_summary: (usize, usize, usize),
    /// Janela em que o heroi esta e onde ela estava (para ele viajar junto quando ela e movida).
    riding: Option<(u64, Rect)>,
    visited: HashSet<u64>,
    /// Nome do lugar onde pousou por ultimo e por quanto tempo ainda aparece.
    pub label: String,
    label_left: f32,
    /// Bichos ja atingidos pelo golpe de espada em andamento.
    swing_hits: (u32, Vec<Entity>),
    script: Option<Script>,
    /// Alguma janela do SpiderDesk tem o foco do teclado.
    pub has_focus: bool,
    rng: Rng,
}

impl HeroState {
    pub fn new(seed: u64, cfg: &Config, opts: &RunOptions) -> Self {
        let script = opts
            .hero_demo
            .as_deref()
            .and_then(|s| match Script::parse(s) {
                Ok(s) => Some(s),
                Err(e) => {
                    warn!("--hero-demo ignorado: {e}");
                    None
                }
            });
        Self {
            hero: Hero::new(&cfg.hero),
            loot: Loot::new(seed),
            enabled: cfg.hero.enabled && !opts.poc,
            platforms: Vec::new(),
            ladders: Vec::new(),
            world_key: None,
            world_summary: (0, 0, 0),
            riding: None,
            visited: HashSet::new(),
            label: String::new(),
            label_left: 0.0,
            swing_hits: (0, Vec::new()),
            script,
            has_focus: false,
            rng: Rng::new(seed ^ 0xBEEF),
        }
    }

    /// Posicao do heroi para os bichos reagirem (se ele esta em jogo e vivo).
    pub fn threat(&self, c: &HeroConfig) -> Option<Vec2> {
        (self.enabled && c.mobs_notice && self.hero.alive()).then(|| self.hero.center(c))
    }
}

/// Linhas do heroi para o painel (vazio = heroi desligado).
#[derive(Resource, Default, Debug)]
pub struct HeroHud {
    pub line: String,
}

/// Nome curto de uma janela para o heroi dizer onde esta.
fn short_title(title: &str) -> String {
    let t = title.trim();
    let t = if t.is_empty() { "janela sem nome" } else { t };
    let mut s: String = t.chars().take(34).collect();
    if t.chars().count() > 34 {
        s.push_str("...");
    }
    s
}

/// O que e o apoio `p`, em palavras: e assim que o heroi "identifica" as coisas.
pub fn describe(p: &Platform, windows: &[DesktopWindow]) -> String {
    let owner = windows.iter().find(|w| w.id == p.owner);
    let title = owner.map(|w| short_title(&w.title)).unwrap_or_default();
    match p.what {
        Ground::Floor => "Chao da tela".into(),
        Ground::WindowTop => format!("Janela: {title}"),
        Ground::Element if owner.is_some_and(|w| w.title == "top-bar") => "Barra superior".into(),
        Ground::Element => format!("Icone: {}", title.trim_end_matches(".desktop")),
        Ground::Text => format!("Texto em: {title}"),
        Ground::Line => format!("Borda em: {title}"),
    }
}

/// Monta o mundo do heroi (apoios e escadas) quando as janelas ou a visao mudam, e faz o heroi
/// viajar junto com a janela em que esta quando ela e arrastada.
pub fn hero_world(
    cfg: Res<Config>,
    geo: Res<OverlayGeometry>,
    desktop: Res<Desktop>,
    mut state: ResMut<HeroState>,
    mut requested: Local<Option<bool>>,
) {
    let want_images = state.enabled && cfg.hero.inside_apps;
    if *requested != Some(want_images) {
        *requested = Some(want_images);
        platform::request_window_images(want_images);
    }
    if !state.enabled || !geo.ready || desktop.version == 0 {
        return;
    }
    let key = (desktop.version, platform::window_ledges_version(), geo.size);
    if state.world_key == Some(key) {
        return;
    }
    let state = &mut *state;
    state.world_key = Some(key);
    let (platforms, ladders) = build_world(
        &geo.local_rect(),
        &desktop.windows,
        &platform::window_ledges,
        &desktop.graph.threads,
        cfg.hero.min_ledge_px,
    );
    state.platforms = platforms;
    state.ladders = ladders;
    let inside = state
        .platforms
        .iter()
        .filter(|p| p.what.is_inside_app())
        .count();
    let summary = (state.platforms.len(), inside, state.ladders.len());
    if state.world_summary != summary {
        state.world_summary = summary;
        info!(
            "mundo do heroi: {} apoios ({} dentro de aplicativos), {} escadas",
            summary.0, summary.1, summary.2
        );
    }

    // a janela sob os pes foi movida (mesmo tamanho): o heroi vai junto
    let owner = state.hero.ground.map(|g| g.owner).filter(|o| *o != 0);
    let now = owner.and_then(|id| {
        desktop
            .windows
            .iter()
            .find(|w| w.id == id)
            .map(|w| (id, w.rect))
    });
    if let (Some((id, new)), Some((old_id, old))) = (now, state.riding)
        && id == old_id
        && (new.width() - old.width()).abs() < 1.0
        && (new.height() - old.height()).abs() < 1.0
    {
        let delta = new.min - old.min;
        if delta != Vec2::ZERO {
            state.hero.pos += delta;
            if let Some(g) = &mut state.hero.ground {
                g.x0 += delta.x;
                g.x1 += delta.x;
                g.y += delta.y;
            }
        }
    }
    state.riding = now;
}

/// Teclado -> heroi; trata pousos (identifica o lugar, descobertas) e reaparecimento.
#[allow(clippy::too_many_arguments)]
pub fn hero_control(
    cfg: Res<Config>,
    time: Res<Time<Real>>,
    geo: Res<OverlayGeometry>,
    desktop: Res<Desktop>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window>,
    mut state: ResMut<HeroState>,
    mut progress: ResMut<Progress>,
    mut float: MessageWriter<FloatText>,
    mut activity: ResMut<Activity>,
    mut focus: Option<ResMut<bevy::input_focus::InputFocus>>,
) {
    // Espaco e o golpe: um botao do painel com foco (clicado por ultimo) seria "apertado" junto
    // (Espaco/Enter ativam o botao focado), e o ultimo clicado pode ser "Sair"
    if let Some(focus) = focus.as_mut()
        && focus.get().is_some()
    {
        focus.clear();
    }
    if !state.enabled || !geo.ready || state.world_key.is_none() {
        return;
    }
    let c = &cfg.hero;
    let dt = time.delta_secs();
    let state = &mut *state;
    state.has_focus = windows.iter().any(|w| w.focused);
    let any = |list: &[KeyCode]| list.iter().any(|k| keys.pressed(*k));
    let typed = HeroInput {
        left: any(&[KeyCode::KeyA, KeyCode::ArrowLeft]),
        right: any(&[KeyCode::KeyD, KeyCode::ArrowRight]),
        up: any(&[KeyCode::KeyW, KeyCode::ArrowUp]),
        down: any(&[KeyCode::KeyS, KeyCode::ArrowDown]),
        jump: false, // (W/cima pula)
        run: any(&[KeyCode::ControlLeft, KeyCode::ControlRight]),
        sneak: any(&[KeyCode::ShiftLeft, KeyCode::ShiftRight]),
        attack: any(&[KeyCode::Space, KeyCode::KeyJ, KeyCode::KeyF]),
    };
    let scripted = state.script.as_mut().and_then(|s| s.next(dt));
    if scripted.is_none() && state.script.take().is_some() {
        info!("--hero-demo: sequencia terminada");
    }
    let inp = scripted.unwrap_or(typed);

    let was_placed = state.hero.placed;
    let ev = state.hero.step(
        dt,
        inp,
        &state.platforms,
        &state.ladders,
        &geo.local_rect(),
        c,
    );
    // barras do sistema sao paredes/teto
    let panels: Vec<Rect> = desktop
        .windows
        .iter()
        .filter(|w| w.is_panel())
        .map(|w| w.rect)
        .collect();
    state.hero.keep_out_of(&panels, &geo.local_rect(), c);
    state.label_left = (state.label_left - dt).max(0.0);
    // `SPIDERDESK_HERO_TRACE=1`: posicao do heroi no log 2x por segundo (testes de teclado)
    if std::env::var_os("SPIDERDESK_HERO_TRACE").is_some() {
        let (now, hero) = (time.elapsed_secs(), &state.hero);
        if (now * 2.0) as i32 != ((now - dt) * 2.0) as i32 {
            info!(
                "heroi em ({:.0}, {:.0}) foco={} chao={:?}",
                hero.pos.x,
                hero.pos.y,
                state.has_focus,
                hero.ground.map(|g| g.what)
            );
        }
    }
    if ev.respawned {
        if was_placed {
            info!("heroi reapareceu");
        } else {
            info!(
                "heroi em jogo: A/D andam, Ctrl corre, Shift devagar, W pula/escala, S desce, \
                 Espaco golpeia (clique no painel para ele receber o teclado)"
            );
        }
    }
    if let Some(p) = ev.landed {
        let name = describe(&p, &desktop.windows);
        if name != state.label {
            // (com teclas programadas o log conta a historia do teste)
            if state.script.is_some() {
                info!(
                    "heroi pousou em: {name} ({:.0}, {:.0})",
                    state.hero.pos.x, state.hero.pos.y
                );
            } else {
                debug!("heroi pousou em: {name}");
            }
        }
        state.label = name;
        state.label_left = LABEL_SECS;
        // primeira vez em cima desta janela/elemento
        if p.owner != 0 && state.visited.insert(p.owner) && c.xp_per_discovery > 0 {
            info!(
                "heroi descobriu: {} (+{} XP)",
                state.label, c.xp_per_discovery
            );
            if let Some(new) = progress.add_xp(c.xp_per_discovery, &cfg.combat) {
                info!("espada evoluiu: {} (XP {})", new.label(), progress.xp);
            }
            float.write(FloatText {
                at: state.hero.pos - Vec2::new(0.0, c.size_px + 34.0),
                text: format!("Lugar novo! +{} XP", c.xp_per_discovery),
                color: COLOR_GOOD,
                size: 16.0,
            });
        }
    }
    if *activity != Activity::Fast {
        *activity = Activity::Fast;
    }
}

/// Distancia de um ponto a um retangulo (0 = dentro).
fn rect_distance(r: &Rect, p: Vec2) -> f32 {
    (p - p.clamp(r.min, r.max)).length()
}

/// Espada, pisao e contato com os bichos; golpes em ninhos, no fio e na teia da aranha.
#[allow(clippy::too_many_arguments)]
pub fn hero_combat(
    mut commands: Commands,
    cfg: Res<Config>,
    geo: Res<OverlayGeometry>,
    effects: Option<Res<EffectAssets>>,
    mut state: ResMut<HeroState>,
    mut progress: ResMut<Progress>,
    mut mobs: Query<(Entity, &mut Mob), Without<PendingModel>>,
    mut field: ResMut<NestField>,
    mut heist: Option<ResMut<crate::web::HeistState>>,
    mut fx: MessageWriter<HitFx>,
    mut float: MessageWriter<FloatText>,
) {
    if !state.enabled || !state.hero.alive() {
        return;
    }
    let c = &cfg.hero;
    let state = &mut *state;
    let hero = &mut state.hero;
    let sword = progress.sword;
    let attack = hero.attack_box(c);
    let new_swing = attack.is_some() && state.swing_hits.0 != hero.swing_id;
    if new_swing {
        state.swing_hits = (hero.swing_id, Vec::new());
    }
    let report =
        |strike: crate::game::Strike, at: Vec2, radius: f32, fx: &mut MessageWriter<HitFx>| {
            if let Some(new) = strike.upgraded {
                info!("espada evoluiu: {}", new.label());
            }
            fx.write(HitFx {
                at,
                radius,
                sword,
                strike,
            });
        };

    for (entity, mut mob) in &mut mobs {
        if !mob.sim.hittable() {
            continue;
        }
        let (at, r) = (mob.sim.body_center, mob.sim.hit_radius);
        if let Some(b) = attack
            && rect_distance(&b, at) <= r
            && !state.swing_hits.1.contains(&entity)
        {
            state.swing_hits.1.push(entity);
            let strike = mob
                .sim
                .take_hit_scaled(&mut progress, &cfg, c.damage_multiplier);
            report(strike, at, r, &mut fx);
        } else if hero.try_stomp(at, r, c) {
            let strike = mob
                .sim
                .take_hit_scaled(&mut progress, &cfg, c.stomp_multiplier);
            report(strike, at, r, &mut fx);
        } else if hero.center(c).distance(at) <= hero.radius(c) + r * 0.75
            // bicho que acabou de apanhar (deste golpe ou de outro) esta fugindo, nao atacando
            && !state.swing_hits.1.contains(&entity)
            && mob.sim.hurt_flash <= 0.0
        {
            let damage = c.contact_damage as i32 * if mob.sim.elite { 2 } else { 1 };
            if hero.hurt(damage, at, c) {
                float.write(FloatText {
                    at: hero.pos - Vec2::new(0.0, c.size_px + 20.0),
                    text: format!("-{damage}"),
                    color: COLOR_BAD,
                    size: 20.0,
                });
            }
        }
    }

    // o comeco de cada golpe tambem acerta ninhos e a seda da aranha (fio do cursor / teia)
    if new_swing && let Some(b) = attack {
        let tip = Vec2::new(
            hero.pos.x + hero.facing * c.attack_reach_px * 0.7,
            hero.pos.y - c.size_px * 0.5,
        );
        let nest = [tip, b.center(), hero.center(c)]
            .into_iter()
            .find_map(|p| field.nest_at(p, &cfg.nests));
        if let Some(id) = nest
            && let Some(hit) = field.strike(id, &mut progress, &cfg.combat, &cfg.nests)
        {
            report(hit.strike, hit.at, hit.radius, &mut fx);
        }
        if let Some(heist) = heist.as_mut() {
            heist.hero_swings.push((tip, sword.damage(&cfg.combat)));
        }
    }

    if !hero.alive() {
        info!("heroi derrotado; reaparece em {:.0} s", c.respawn_secs);
        float.write(FloatText {
            at: hero.pos - Vec2::new(0.0, c.size_px + 30.0),
            text: "Derrotado!".into(),
            color: COLOR_BAD,
            size: 24.0,
        });
        if let Some(effects) = effects {
            let w = geo.local_to_world(hero.center(c));
            spawn_burst(&mut commands, &effects, w.extend(HERO_Z), &mut state.rng);
        }
    }
}

/// Moedas, estrela (missao) e coracoes.
pub fn hero_loot(
    cfg: Res<Config>,
    time: Res<Time<Real>>,
    geo: Res<OverlayGeometry>,
    desktop: Res<Desktop>,
    mut state: ResMut<HeroState>,
    mut progress: ResMut<Progress>,
    mut float: MessageWriter<FloatText>,
) {
    if !state.enabled || !geo.ready || state.world_key.is_none() {
        return;
    }
    let c = &cfg.hero;
    let state = &mut *state;
    let got = state.loot.update(
        time.delta_secs(),
        &mut state.hero,
        &state.platforms,
        &geo.local_rect(),
        c,
    );
    for item in got {
        let (text, color, xp) = match item.kind {
            PickupKind::Coin => (format!("+{} XP", c.xp_per_coin), COLOR_GOLD, c.xp_per_coin),
            PickupKind::Star => {
                info!(
                    "heroi pegou a estrela em: {} (total {})",
                    describe(&item.on, &desktop.windows),
                    state.loot.stars
                );
                (
                    format!("Estrela! +{} XP", c.xp_per_star),
                    COLOR_GOLD,
                    c.xp_per_star,
                )
            }
            PickupKind::Heart => ("+1 vida".to_string(), COLOR_BAD, 0),
        };
        if xp > 0
            && let Some(new) = progress.add_xp(xp, &cfg.combat)
        {
            info!("espada evoluiu: {} (XP {})", new.label(), progress.xp);
        }
        float.write(FloatText {
            at: item.pos - Vec2::new(0.0, 14.0),
            text,
            color,
            size: if item.kind == PickupKind::Star {
                22.0
            } else {
                16.0
            },
        });
    }
}

// ---------------------------------------------------------------------------------------------
// desenho

#[derive(Resource)]
pub struct HeroAssets {
    sphere: Handle<Mesh>,
    limb: Handle<Mesh>,
    cube: Handle<Mesh>,
    tunic: Handle<StandardMaterial>,
    skin: Handle<StandardMaterial>,
    steel: Handle<StandardMaterial>,
    dark: Handle<StandardMaterial>,
    red: Handle<StandardMaterial>,
    wood: Handle<StandardMaterial>,
    gold: Handle<StandardMaterial>,
    blades: [Handle<StandardMaterial>; 4],
}

#[derive(Component)]
pub struct HeroRoot;

/// Partes articuladas do boneco.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum HeroPart {
    Body,
    LegFront,
    LegBack,
    ArmSword,
    ArmShield,
    Sword,
    Cape,
}

#[derive(Component)]
pub struct HeroBlade;

/// Bolinha que aponta para a estrela da missao.
#[derive(Component)]
pub struct HeroCompass;

#[derive(Component)]
pub struct LootVisual {
    id: u32,
}

pub fn setup_hero(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut matte = |c: Color| {
        materials.add(StandardMaterial {
            base_color: c,
            perceptual_roughness: 0.85,
            ..default()
        })
    };
    let tunic = matte(Color::srgb(0.18, 0.42, 0.85));
    let skin = matte(Color::srgb(0.96, 0.78, 0.62));
    let steel = matte(Color::srgb(0.72, 0.75, 0.80));
    let dark = matte(Color::srgb(0.12, 0.10, 0.12));
    let red = matte(Color::srgb(0.85, 0.16, 0.18));
    let wood = matte(Color::srgb(0.50, 0.33, 0.18));
    let blades = Sword::ALL.map(|s| matte(sword_color(s)));
    let gold = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.83, 0.18),
        unlit: true,
        ..default()
    });
    commands.insert_resource(HeroAssets {
        sphere: meshes.add(Sphere::new(0.5).mesh().uv(20, 12)),
        // membro: capsula de comprimento total 1 ao longo de Y, pendurada na origem
        limb: meshes.add(Capsule3d::new(0.22, 0.56)),
        cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
        tunic,
        skin,
        steel,
        dark,
        red,
        wood,
        gold,
        blades,
    });
}

/// Monta o boneco (unidades do modelo: pes em y = 0, olhando para +X).
fn spawn_hero_model(commands: &mut Commands, a: &HeroAssets) {
    let piece = |mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>, at: Vec3, scale: Vec3| {
        (
            Mesh3d(mesh.clone()),
            MeshMaterial3d(mat.clone()),
            Transform::from_translation(at).with_scale(scale),
        )
    };
    let pivot =
        |part: HeroPart, at: Vec3| (part, Transform::from_translation(at), Visibility::Inherited);
    let leg = |commands: &mut ChildSpawnerCommands, part: HeroPart, z: f32| {
        commands
            .spawn(pivot(part, Vec3::new(0.0, 0.36, z)))
            .with_children(|p| {
                p.spawn(piece(
                    &a.limb,
                    &a.dark,
                    Vec3::new(0.0, -0.17, 0.0),
                    Vec3::new(0.40, 0.34, 0.40),
                ));
                // bota
                p.spawn(piece(
                    &a.sphere,
                    &a.wood,
                    Vec3::new(0.035, -0.31, 0.0),
                    Vec3::new(0.21, 0.11, 0.15),
                ));
            });
    };
    commands
        .spawn((
            HeroRoot,
            Transform::from_scale(Vec3::ZERO),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            leg(root, HeroPart::LegBack, -0.075);
            leg(root, HeroPart::LegFront, 0.075);
            root.spawn(pivot(HeroPart::Body, Vec3::new(0.0, 0.36, 0.0)))
                .with_children(|body| {
                    // capa (atras), tronco, cinto
                    body.spawn(pivot(HeroPart::Cape, Vec3::new(-0.11, 0.40, 0.0)))
                        .with_children(|p| {
                            p.spawn(piece(
                                &a.cube,
                                &a.red,
                                Vec3::new(0.0, -0.20, 0.0),
                                Vec3::new(0.035, 0.42, 0.30),
                            ));
                        });
                    body.spawn(piece(
                        &a.sphere,
                        &a.tunic,
                        Vec3::new(0.0, 0.20, 0.0),
                        Vec3::new(0.46, 0.50, 0.40),
                    ));
                    body.spawn(piece(
                        &a.sphere,
                        &a.wood,
                        Vec3::new(0.0, 0.06, 0.0),
                        Vec3::new(0.45, 0.10, 0.41),
                    ));
                    // cabeca, elmo, penacho, olhos (um de cada lado)
                    body.spawn(piece(
                        &a.sphere,
                        &a.skin,
                        Vec3::new(0.02, 0.63, 0.0),
                        Vec3::splat(0.50),
                    ));
                    body.spawn(piece(
                        &a.sphere,
                        &a.steel,
                        Vec3::new(0.0, 0.75, 0.0),
                        Vec3::new(0.56, 0.34, 0.56),
                    ));
                    body.spawn(piece(
                        &a.sphere,
                        &a.red,
                        Vec3::new(-0.06, 0.97, 0.0),
                        Vec3::new(0.13, 0.22, 0.10),
                    ));
                    for z in [-0.14, 0.14] {
                        body.spawn(piece(
                            &a.sphere,
                            &a.dark,
                            Vec3::new(0.20, 0.62, z),
                            Vec3::splat(0.09),
                        ));
                    }
                    // braco do escudo (lado de tras)
                    body.spawn(pivot(HeroPart::ArmShield, Vec3::new(0.0, 0.36, -0.24)))
                        .with_children(|p| {
                            p.spawn(piece(
                                &a.limb,
                                &a.tunic,
                                Vec3::new(0.0, -0.13, 0.0),
                                Vec3::new(0.24, 0.27, 0.24),
                            ));
                            p.spawn(piece(
                                &a.sphere,
                                &a.wood,
                                Vec3::new(0.03, -0.22, -0.05),
                                Vec3::new(0.30, 0.30, 0.06),
                            ));
                            p.spawn(piece(
                                &a.sphere,
                                &a.steel,
                                Vec3::new(0.03, -0.22, -0.08),
                                Vec3::new(0.12, 0.12, 0.05),
                            ));
                        });
                    // braco da espada (lado da camera)
                    body.spawn(pivot(HeroPart::ArmSword, Vec3::new(0.0, 0.36, 0.24)))
                        .with_children(|p| {
                            p.spawn(piece(
                                &a.limb,
                                &a.tunic,
                                Vec3::new(0.0, -0.13, 0.0),
                                Vec3::new(0.24, 0.27, 0.24),
                            ));
                            p.spawn(piece(
                                &a.sphere,
                                &a.skin,
                                Vec3::new(0.0, -0.26, 0.0),
                                Vec3::splat(0.10),
                            ));
                            p.spawn(pivot(HeroPart::Sword, Vec3::new(0.0, -0.26, 0.02)))
                                .with_children(|s| {
                                    s.spawn((
                                        HeroBlade,
                                        piece(
                                            &a.cube,
                                            &a.blades[0],
                                            Vec3::new(0.0, 0.30, 0.0),
                                            Vec3::new(0.07, 0.48, 0.025),
                                        ),
                                    ));
                                    s.spawn(piece(
                                        &a.cube,
                                        &a.wood,
                                        Vec3::new(0.0, 0.05, 0.0),
                                        Vec3::new(0.20, 0.04, 0.05),
                                    ));
                                });
                        });
                });
        });
    commands.spawn((
        HeroCompass,
        Mesh3d(a.sphere.clone()),
        MeshMaterial3d(a.gold.clone()),
        Transform::from_scale(Vec3::ZERO),
        Visibility::Hidden,
    ));
}

/// Angulos (rad) das articulacoes para o estado atual. Braco/perna a 0 = pendurado; positivo =
/// para a frente. Pura, para ser testada.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub leg_front: f32,
    pub leg_back: f32,
    pub arm_sword: f32,
    pub arm_shield: f32,
    /// Punho: 0 = lamina para cima (descanso), PI = lamina estendida no sentido do braco.
    pub wrist: f32,
    pub lean: f32,
    pub cape: f32,
    pub bob: f32,
}

pub fn pose(hero: &Hero, c: &HeroConfig, t: f32) -> Pose {
    use std::f32::consts::{PI, TAU};
    let speed = (hero.vel.x.abs() / c.run_speed.max(1.0)).min(1.0);
    let phase = hero.stride * TAU;
    let mut p = Pose {
        leg_front: 0.0,
        leg_back: 0.0,
        arm_sword: 0.25,
        arm_shield: 0.15,
        wrist: -0.75,
        lean: 0.0,
        cape: -0.12 - 0.05 * (t * 2.0).sin(),
        bob: 0.012 * (t * 2.4).sin(),
    };
    if hero.climbing.is_some() {
        let s = phase.sin();
        p.leg_front = 0.5 * s;
        p.leg_back = -0.5 * s;
        p.arm_sword = 2.7 + 0.35 * s;
        p.arm_shield = 2.7 - 0.35 * s;
        p.wrist = -2.2;
        p.bob = 0.0;
    } else if hero.ground.is_some() {
        let moving = (hero.vel.x.abs() / 40.0).min(1.0);
        let swing = phase.sin() * (0.35 + 0.6 * speed) * moving;
        p.leg_front = swing;
        p.leg_back = -swing;
        p.arm_shield = 0.15 + swing * 0.8;
        p.arm_sword = 0.25 - swing * 0.35;
        p.lean = -0.2 * speed;
        p.cape = -0.12 - 0.75 * speed + 0.08 * (t * 11.0).sin() * moving;
        p.bob += 0.03 * (phase * 2.0).sin().abs() * moving;
    } else {
        // no ar: subindo, pernas abertas e braco para cima; caindo, encolhe
        let rising = hero.vel.y < 0.0;
        p.leg_front = if rising { 0.75 } else { 0.3 };
        p.leg_back = if rising { -0.5 } else { -0.2 };
        p.arm_shield = if rising { 2.2 } else { 1.3 };
        p.cape = if rising { -0.5 } else { -2.4 };
        p.lean = -0.08;
    }
    if hero.attack > 0.0 {
        // corte de cima para baixo: braco de quase vertical ate a frente/baixo, lamina estendida
        let k = (1.0 - hero.attack / SWING_SECS).clamp(0.0, 1.0);
        let ease = 1.0 - (1.0 - k) * (1.0 - k);
        p.arm_sword = 2.9 + (0.45 - 2.9) * ease;
        p.wrist = PI;
        p.lean -= 0.12 * ease;
    }
    p
}

/// Posiciona e anima o boneco, a bussola e os itens.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn sync_hero_visuals(
    mut commands: Commands,
    cfg: Res<Config>,
    geo: Res<OverlayGeometry>,
    time: Res<Time<Real>>,
    state: Res<HeroState>,
    progress: Res<Progress>,
    assets: Option<Res<HeroAssets>>,
    mut roots: Query<(&mut Transform, &mut Visibility), With<HeroRoot>>,
    mut parts: Query<(&HeroPart, &mut Transform), Without<HeroRoot>>,
    mut blades: Query<&mut MeshMaterial3d<StandardMaterial>, With<HeroBlade>>,
    mut compass: Query<
        (&mut Transform, &mut Visibility),
        (With<HeroCompass>, Without<HeroRoot>, Without<HeroPart>),
    >,
    mut loot: Query<
        (Entity, &LootVisual, &mut Transform),
        (Without<HeroRoot>, Without<HeroPart>, Without<HeroCompass>),
    >,
    mut known: Local<Vec<u32>>,
) {
    let Some(a) = assets else { return };
    if !geo.ready {
        return;
    }
    let c = &cfg.hero;
    let t = time.elapsed_secs();
    let hero = &state.hero;
    if roots.is_empty() {
        if state.enabled {
            spawn_hero_model(&mut commands, &a);
        }
        return;
    }
    let show = |vis: &mut Visibility, on: bool| {
        let want = if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
        }
    };

    for (mut tf, mut vis) in &mut roots {
        let dying = hero.dead_for.map(|d| (d / 0.45).min(1.0));
        // pisca enquanto invulneravel
        let blink = hero.invuln > 0.0 && hero.alive() && (t * 16.0) as i32 % 2 == 0;
        let on = state.enabled && hero.placed && dying.is_none_or(|d| d < 1.0) && !blink;
        show(&mut vis, on);
        let unit = c.size_px / MODEL_H;
        let squash = hero.squash;
        let shrink = 1.0 - dying.unwrap_or(0.0);
        let w = geo.local_to_world(hero.pos);
        tf.translation = Vec3::new(w.x, w.y, HERO_Z);
        // espelhado para a esquerda (o Bevy inverte as faces sozinho)
        tf.scale = Vec3::new(
            hero.facing * unit * (1.0 + 0.14 * squash) * shrink,
            unit * (1.0 - 0.2 * squash) * shrink,
            unit * shrink,
        );
        tf.rotation = Quat::from_rotation_z(dying.unwrap_or(0.0) * 9.0);
    }

    let p = pose(hero, c, t);
    for (part, mut tf) in &mut parts {
        let angle = match part {
            HeroPart::Body => {
                tf.translation.y = 0.36 + p.bob;
                p.lean
            }
            HeroPart::LegFront => p.leg_front,
            HeroPart::LegBack => p.leg_back,
            HeroPart::ArmSword => p.arm_sword,
            HeroPart::ArmShield => p.arm_shield,
            HeroPart::Sword => p.wrist,
            HeroPart::Cape => p.cape,
        };
        tf.rotation = Quat::from_rotation_z(angle);
    }
    let blade = &a.blades[Sword::ALL
        .iter()
        .position(|s| *s == progress.sword)
        .unwrap_or(0)];
    for mut m in &mut blades {
        if m.0 != *blade {
            m.0 = blade.clone();
        }
    }

    // bussola: bolinha dourada do lado do heroi apontando para a estrela da missao
    let star = state.loot.star().map(|s| s.pos);
    for (mut tf, mut vis) in &mut compass {
        let center = hero.center(c);
        let far = star.filter(|s| s.distance(center) > c.size_px * 3.0);
        show(&mut vis, state.enabled && hero.alive() && far.is_some());
        if let Some(s) = far {
            let dir = (s - center).normalize_or_zero();
            let at = geo.local_to_world(center + dir * c.size_px * (1.0 + 0.08 * (t * 5.0).sin()));
            tf.translation = at.extend(HERO_Z + 5.0);
            tf.scale = Vec3::splat(c.size_px * 0.16);
        }
    }

    // itens
    known.clear();
    for (entity, visual, mut tf) in &mut loot {
        let item = state.loot.items.iter().find(|i| i.id == visual.id);
        let Some(item) = item.filter(|_| state.enabled) else {
            commands.entity(entity).despawn();
            continue;
        };
        known.push(item.id);
        let bobbing = Vec2::new(0.0, 3.0 * (t * 3.0 + item.id as f32).sin());
        let w = geo.local_to_world(item.pos + bobbing);
        tf.translation = w.extend(LOOT_Z);
        let grow = (item.age / 0.25).min(1.0);
        match item.kind {
            PickupKind::Coin => {
                tf.rotation = Quat::from_rotation_y(t * 4.0 + item.id as f32);
                tf.scale = Vec3::splat(grow);
            }
            PickupKind::Star => {
                tf.rotation = Quat::from_rotation_z(t * 1.5);
                tf.scale = Vec3::splat(grow * (1.0 + 0.16 * (t * 6.0).sin()));
            }
            PickupKind::Heart => tf.scale = Vec3::splat(grow * (1.0 + 0.1 * (t * 8.0).sin())),
        }
    }
    if !state.enabled {
        return;
    }
    for item in state.loot.items.iter().filter(|i| !known.contains(&i.id)) {
        spawn_loot_visual(&mut commands, &a, item);
    }
}

fn spawn_loot_visual(commands: &mut Commands, a: &HeroAssets, item: &Pickup) {
    let piece =
        |mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>, at: Vec3, scale: Vec3, rot: f32| {
            (
                Mesh3d(mesh.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(at)
                    .with_scale(scale)
                    .with_rotation(Quat::from_rotation_z(rot)),
            )
        };
    commands
        .spawn((
            LootVisual { id: item.id },
            Transform::from_scale(Vec3::ZERO),
            Visibility::Inherited,
        ))
        .with_children(|p| match item.kind {
            PickupKind::Coin => {
                p.spawn(piece(
                    &a.sphere,
                    &a.gold,
                    Vec3::ZERO,
                    Vec3::new(15.0, 15.0, 4.0),
                    0.0,
                ));
                p.spawn(piece(
                    &a.sphere,
                    &a.wood,
                    Vec3::new(0.0, 0.0, 0.0),
                    Vec3::new(8.0, 8.0, 5.0),
                    0.0,
                ));
            }
            PickupKind::Star => {
                // dois quadrados girados = estrela de 8 pontas
                for rot in [0.0, std::f32::consts::FRAC_PI_4] {
                    p.spawn(piece(
                        &a.cube,
                        &a.gold,
                        Vec3::ZERO,
                        Vec3::new(24.0, 24.0, 5.0),
                        rot,
                    ));
                }
                p.spawn(piece(
                    &a.sphere,
                    &a.skin,
                    Vec3::new(0.0, 0.0, 3.0),
                    Vec3::splat(11.0),
                    0.0,
                ));
            }
            PickupKind::Heart => {
                for x in [-4.5, 4.5] {
                    p.spawn(piece(
                        &a.sphere,
                        &a.red,
                        Vec3::new(x, 3.0, 0.0),
                        Vec3::splat(11.0),
                        0.0,
                    ));
                }
                p.spawn(piece(
                    &a.cube,
                    &a.red,
                    Vec3::new(0.0, -2.0, 0.0),
                    Vec3::new(11.0, 11.0, 8.0),
                    std::f32::consts::FRAC_PI_4,
                ));
            }
        });
}

// ---------------------------------------------------------------------------------------------
// interface

#[derive(Component)]
pub struct HeroHearts;

#[derive(Component)]
pub struct HeartPip(u32);

#[derive(Component)]
pub struct HeroLabel;

const PIP: f32 = 9.0;

/// Texto do heroi no painel.
pub fn hud_text(
    state: &HeroState,
    c: &HeroConfig,
    star: Option<&str>,
    blind: Option<&str>,
) -> String {
    if !state.enabled {
        return String::new();
    }
    let hero = &state.hero;
    let mut s = match hero.dead_for {
        Some(d) => format!(
            "Heroi: derrotado ({:.0}s)",
            (c.respawn_secs - d).max(0.0).ceil()
        ),
        None => format!(
            "Heroi: vida {}/{}\nMoedas {}  Estrelas {}",
            hero.hp, c.max_hp, state.loot.coins, state.loot.stars
        ),
    };
    if !state.label.is_empty() {
        s.push_str(&format!("\nEm: {}", state.label));
    }
    if let Some(star) = star {
        s.push_str(&format!("\nMissao, estrela em:\n{star}"));
    }
    if let Some(why) = blind {
        s.push_str(&format!("\n{why}"));
    }
    if !state.has_focus && state.script.is_none() {
        s.push_str("\n>> clique aqui p/ usar o teclado <<");
    }
    s
}

/// Coracoes e nome do lugar acima do heroi; linha do painel.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn hero_hud(
    mut commands: Commands,
    cfg: Res<Config>,
    desktop: Res<Desktop>,
    state: Res<HeroState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    root: Query<Entity, With<HudRoot>>,
    mut hearts: Query<(&mut Node, &mut Visibility), (With<HeroHearts>, Without<HeroLabel>)>,
    mut pips: Query<(&HeartPip, &mut BackgroundColor)>,
    mut labels: Query<
        (&mut Node, &mut Visibility, &mut Text, &mut TextColor),
        (With<HeroLabel>, Without<HeroHearts>),
    >,
    mut hud: ResMut<HeroHud>,
    time: Res<Time<Real>>,
    mut helper: Local<(f32, Option<platform::HelperStatus>)>,
) {
    let c = &cfg.hero;
    // por que ele nao enxerga dentro dos apps (conferido a cada 2 s, nao a cada quadro)
    let now = time.elapsed_secs();
    if helper.1.is_none() || now - helper.0 > 2.0 {
        *helper = (now, Some(platform::helper_status()));
    }
    let blind = match helper.1 {
        Some(platform::HelperStatus::Missing) if c.inside_apps => {
            Some("Nao enxerga dentro dos apps:\nclique 'Andar nas janelas' e\nsaia/entre na sessao")
        }
        _ => None,
    };
    let star = state.loot.star().map(|s| describe(&s.on, &desktop.windows));
    let line = hud_text(&state, c, star.as_deref(), blind);
    if hud.line != line {
        hud.line = line;
    }
    let Ok(root) = root.single() else { return };
    let scale = windows
        .single()
        .map(|w| w.scale_factor())
        .unwrap_or(1.0)
        .max(0.1);
    let hero = &state.hero;
    let show = state.enabled && hero.alive();
    let width = c.max_hp as f32 * (PIP + 2.0);

    if hearts.is_empty() {
        if !state.enabled {
            return;
        }
        let bar = commands
            .spawn((
                HeroHearts,
                Node {
                    position_type: PositionType::Absolute,
                    column_gap: px(2),
                    ..default()
                },
                Visibility::Hidden,
            ))
            .with_children(|p| {
                for i in 0..c.max_hp {
                    p.spawn((
                        HeartPip(i),
                        Node {
                            width: px(PIP),
                            height: px(PIP),
                            border_radius: BorderRadius::all(px(3)),
                            ..default()
                        },
                        BackgroundColor(Color::NONE),
                    ));
                }
            })
            .id();
        let label = commands
            .spawn((
                HeroLabel,
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(COLOR_INFO),
                TextShadow::default(),
                Visibility::Hidden,
            ))
            .id();
        commands.entity(root).add_children(&[bar, label]);
        return;
    }

    let set = |vis: &mut Visibility, on: bool| {
        let want = if on {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *vis != want {
            *vis = want;
        }
    };
    for (mut node, mut vis) in &mut hearts {
        set(&mut vis, show);
        node.left = px((hero.pos.x - width * scale * 0.5) / scale);
        node.top = px((hero.pos.y - c.size_px - 18.0 * scale) / scale);
    }
    for (pip, mut bg) in &mut pips {
        let want = if (pip.0 as i32) < hero.hp {
            Color::srgb(0.95, 0.2, 0.25)
        } else {
            Color::srgba(0.1, 0.1, 0.12, 0.8)
        };
        if bg.0 != want {
            bg.0 = want;
        }
    }
    for (mut node, mut vis, mut text, mut color) in &mut labels {
        let on = show && state.label_left > 0.0 && !state.label.is_empty();
        set(&mut vis, on);
        if !on {
            continue;
        }
        if **text != state.label {
            **text = state.label.clone();
        }
        // (largura estimada: ~7 px por letra a 13 px)
        let half = state.label.chars().count() as f32 * 3.6;
        node.left = px((hero.pos.x / scale - half).max(2.0));
        node.top = px((hero.pos.y - c.size_px - 38.0 * scale) / scale);
        color.0 = COLOR_INFO.with_alpha((state.label_left / 0.5).min(1.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::{ELEMENT_ID_BASE, WindowState};

    fn win(id: u64, title: &str) -> DesktopWindow {
        let r = Rect::from_xywh(0.0, 0.0, 800.0, 600.0);
        DesktopWindow {
            id,
            rect: r,
            raw_rect: r,
            state: WindowState::Normal,
            pid: 1,
            title: title.into(),
        }
    }

    fn at(owner: u64, what: Ground) -> Platform {
        Platform {
            x0: 0.0,
            x1: 100.0,
            y: 50.0,
            owner,
            what,
            solid: false,
        }
    }

    #[test]
    fn hero_names_what_he_is_standing_on() {
        let ws = [
            win(
                7,
                "main.rs - Game-interativo - Visual Studio Code com um titulo enorme",
            ),
            win(ELEMENT_ID_BASE + 1, "Creality-V7.desktop"),
            win(ELEMENT_ID_BASE + 2, "top-bar"),
        ];
        assert_eq!(describe(&at(0, Ground::Floor), &ws), "Chao da tela");
        assert_eq!(
            describe(&at(7, Ground::Text), &ws),
            "Texto em: main.rs - Game-interativo - Visual..."
        );
        assert!(describe(&at(7, Ground::Line), &ws).starts_with("Borda em: main.rs"));
        assert!(describe(&at(7, Ground::WindowTop), &ws).starts_with("Janela: main.rs"));
        assert_eq!(
            describe(&at(ELEMENT_ID_BASE + 1, Ground::Element), &ws),
            "Icone: Creality-V7"
        );
        assert_eq!(
            describe(&at(ELEMENT_ID_BASE + 2, Ground::Element), &ws),
            "Barra superior"
        );
        assert_eq!(describe(&at(99, Ground::WindowTop), &ws), "Janela: ");
    }

    #[test]
    fn demo_script_plays_keys_with_a_released_frame_between_steps() {
        let mut s = Script::parse("right+run:0.2, jump, wait:0.1,attack:0.05").unwrap();
        let first = s.next(0.1).unwrap();
        assert!(first.right && first.run && !first.jump);
        let keys = Script::parse("d+ctrl+shift+space")
            .unwrap()
            .next(0.0)
            .unwrap();
        assert!(keys.right && keys.run && keys.sneak && keys.attack && !keys.jump);
        assert!(s.next(0.1).unwrap().right);
        assert_eq!(
            s.next(0.1),
            Some(HeroInput::default()),
            "teclas soltas entre passos"
        );
        assert!(s.next(0.1).unwrap().jump);
        let mut seen_attack = false;
        let mut frames = 0;
        while let Some(i) = s.next(0.05) {
            seen_attack |= i.attack;
            frames += 1;
            assert!(frames < 50);
        }
        assert!(seen_attack);
        assert!(Script::parse("banana:1").is_err());
        assert!(Script::parse("right:x").is_err());
        assert!(Script::parse("").unwrap().next(0.1).is_none());
    }

    #[test]
    fn pose_swings_limbs_when_running_and_slashes_from_above() {
        let c = HeroConfig::default();
        let mut h = Hero::new(&c);
        h.placed = true;
        h.ground = Some(at(0, Ground::Floor));
        let still = pose(&h, &c, 0.0);
        assert_eq!((still.leg_front, still.leg_back), (0.0, 0.0));
        h.vel.x = c.run_speed;
        h.stride = 0.25; // auge do passo
        let run = pose(&h, &c, 0.0);
        assert!(run.leg_front > 0.8 && run.leg_back < -0.8);
        assert!(
            run.lean < -0.1 && run.cape < still.cape,
            "inclina e a capa voa"
        );

        h.attack = SWING_SECS;
        let start = pose(&h, &c, 0.0);
        h.attack = 0.001;
        let end = pose(&h, &c, 0.0);
        assert!(
            start.arm_sword > 2.5 && end.arm_sword < 0.7,
            "de cima para baixo"
        );
        assert_eq!(start.wrist, std::f32::consts::PI);

        h.ground = None;
        h.climbing = Some(10.0);
        assert!(
            pose(&h, &c, 0.0).arm_shield > 2.0,
            "bracos para cima na escada"
        );
    }

    #[test]
    fn panel_text_shows_health_place_mission_and_focus_hint() {
        let cfg = Config::default();
        let mut s = HeroState::new(1, &cfg, &RunOptions::default());
        s.hero.placed = true;
        s.label = "Texto em: notas.txt".into();
        let t = hud_text(&s, &cfg.hero, Some("Borda em: Chrome"), None);
        assert!(
            t.starts_with("Heroi: vida 5/5\nMoedas 0  Estrelas 0"),
            "{t}"
        );
        assert!(t.contains("\nEm: Texto em: notas.txt"));
        assert!(t.contains("Missao, estrela em:\nBorda em: Chrome"));
        assert!(t.contains("clique aqui"));
        s.has_focus = true;
        assert!(!hud_text(&s, &cfg.hero, None, None).contains("clique aqui"));
        s.hero.dead_for = Some(1.2);
        assert!(hud_text(&s, &cfg.hero, None, None).starts_with("Heroi: derrotado (2s)"));
        s.enabled = false;
        assert_eq!(hud_text(&s, &cfg.hero, None, None), "");
    }
}
