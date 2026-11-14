//! Configuracao do aplicativo (TOML via serde).
//!
//! Todos os campos possuem valor padrao: um arquivo parcial (ou ausente) nunca impede
//! o aplicativo de iniciar. Valores absurdos sao corrigidos por [`Config::sanitize`].

use std::path::Path;

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum ConfigError {
    Io(std::io::Error),
    Parse(toml::de::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "erro de leitura: {e}"),
            ConfigError::Parse(e) => write!(f, "TOML invalido: {e}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Backend grafico solicitado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BackendChoice {
    /// DX12 no Windows (unico com transparencia confiavel), automatico nos demais.
    #[default]
    Auto,
    Dx12,
    Vulkan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayConfig {
    /// Limite de FPS enquanto ha movimento.
    pub fps_limit: u32,
    /// Taxa de atualizacao quando a aranha esta parada e o cursor esta longe.
    pub idle_update_hz: f32,
    /// Taxa de atualizacao quando a aranha esta invisivel (morta/aguardando).
    pub hidden_update_hz: f32,
    /// Pixels removidos da altura do overlay (evita o modo "fullscreen otimizado").
    pub bottom_inset_px: u32,
    /// Atalho global de encerramento de emergencia. Ex.: "ctrl+alt+shift+q".
    pub emergency_hotkey: String,
    /// Se o laco principal ficar travado por mais que isso, o processo encerra.
    pub watchdog_secs: f32,
    pub backend: BackendChoice,
    /// Amostras MSAA (1, 2 ou 4). 1 = desligado (menor custo de GPU).
    pub msaa_samples: u32,
    /// Raio, em px, em que o hit-test liga; desliga a `hit_radius * hysteresis`.
    pub hit_hysteresis: f32,
    /// Windows: aplica alfa constante 255 (SetLayeredWindowAttributes) quando o winit liga
    /// WS_EX_LAYERED. Remedio hipotetico caso o overlay suma ao passar o cursor sobre a aranha.
    pub layered_alpha_fix: bool,
    /// Cobre TODOS os monitores com um unico overlay (os mobs andam entre as telas). `false` =
    /// so o monitor principal.
    pub all_monitors: bool,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            fps_limit: 60,
            idle_update_hz: 20.0,
            hidden_update_hz: 4.0,
            bottom_inset_px: 1,
            emergency_hotkey: "ctrl+alt+shift+q".to_string(),
            watchdog_secs: 5.0,
            backend: BackendChoice::Auto,
            msaa_samples: 1,
            hit_hysteresis: 1.1,
            layered_alpha_fix: false,
            all_monitors: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SpiderConfig {
    /// Caminho relativo a `assets/` do modelo GLB (so com `mobs.include_glb`).
    pub model: String,
    /// Escala de exibicao (1.0 = ~64 px de envergadura).
    pub scale: f32,
    /// Raio de clique de referencia em px (34 = hitbox natural de cada bicho; escala todos).
    pub hit_radius_px: f32,
    /// Velocidades em px/s.
    pub walk_speed: f32,
    pub run_speed: f32,
    pub climb_speed: f32,
    /// Inclinacao do dorso em direcao a camera (graus) para dar sensacao 3D.
    pub view_tilt_deg: f32,
    /// Rotacao extra aplicada ao modelo GLB (graus, eixo Y) para alinhar a frente do modelo ao eixo +X.
    pub glb_yaw_deg: f32,
    /// Escala extra aplicada ao modelo GLB (multiplica o auto-fit).
    pub glb_scale: f32,
    /// Mede o GLB depois de carregado e o reescala/centraliza automaticamente (~3 unidades).
    pub glb_auto_fit: bool,
}

impl Default for SpiderConfig {
    fn default() -> Self {
        Self {
            model: "models/spider.glb".to_string(),
            scale: 1.0,
            hit_radius_px: 34.0,
            walk_speed: 90.0,
            run_speed: 260.0,
            climb_speed: 60.0,
            view_tilt_deg: 32.0,
            glb_yaw_deg: 0.0,
            glb_scale: 1.0,
            glb_auto_fit: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BehaviorConfig {
    /// Distancia (px) em que o cursor e percebido.
    pub detect_radius_px: f32,
    /// Chance de reagir cada vez que o cursor entra no raio.
    pub react_probability: f32,
    /// Pesos relativos das reacoes ao cursor.
    pub weight_flee: f32,
    pub weight_freeze: f32,
    pub weight_speedup: f32,
    pub weight_reroute: f32,
    /// Intervalo minimo entre reacoes (s).
    pub react_cooldown_secs: f32,
    pub idle_min_secs: f32,
    pub idle_max_secs: f32,
    /// Chance por segundo de uma micro-pausa durante a caminhada.
    pub pause_chance_per_sec: f32,
    pub flee_duration_secs: f32,
    /// Intervalo ate reaparecer apos a morte.
    pub respawn_delay_secs: f32,
    pub death_anim_secs: f32,
    /// Semente do gerador (0 = aleatoria a cada execucao).
    pub seed: u64,
}

impl Default for BehaviorConfig {
    fn default() -> Self {
        Self {
            detect_radius_px: 140.0,
            react_probability: 0.65,
            weight_flee: 5.0,
            weight_freeze: 2.0,
            weight_speedup: 2.0,
            weight_reroute: 2.0,
            react_cooldown_secs: 2.5,
            idle_min_secs: 0.8,
            idle_max_secs: 3.5,
            pause_chance_per_sec: 0.12,
            flee_duration_secs: 2.2,
            respawn_delay_secs: 15.0,
            death_anim_secs: 1.1,
            seed: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NavigationConfig {
    /// Periodo da consulta de janelas (ms). Nunca a cada frame.
    pub window_poll_ms: u64,
    /// Distancia maxima (px) para ligar segmentos de janelas diferentes.
    pub link_distance_px: f32,
    /// Janelas menores que isso (largura ou altura) sao ignoradas.
    pub min_window_size_px: f32,
    /// Trechos de borda menores que isso sao descartados.
    pub min_segment_px: f32,
    /// Usa o perimetro da tela como caminho.
    pub screen_edges: bool,
    /// Fios de seda ligando a base de cada janela a superficie abaixo (pontes entre janelas).
    pub silk_threads: bool,
    /// Desenha o grafo (gizmos) para depuracao.
    pub debug_draw: bool,
}

impl Default for NavigationConfig {
    fn default() -> Self {
        Self {
            window_poll_ms: 300,
            link_distance_px: 14.0,
            min_window_size_px: 120.0,
            min_segment_px: 24.0,
            screen_edges: true,
            silk_threads: true,
            debug_draw: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MobsConfig {
    /// Quantos mobs aparecem ao iniciar.
    pub initial: u32,
    /// Tipo dos mobs iniciais: aranha, aranha-lobo, formiga, barata, gafanhoto, rato ou
    /// "misto" (sorteia). Vazio + `include_glb`: usa o GLB de `spider.model`.
    pub initial_kind: String,
    /// Limite de mobs simultaneos (protege CPU/GPU).
    pub max: u32,
    /// Mostra o painel flutuante de spawn.
    pub panel: bool,
    /// Tambem lista cada `assets/models/*.glb` como tipo de mob (modelos de terceiros).
    pub include_glb: bool,
}

impl Default for MobsConfig {
    fn default() -> Self {
        Self {
            initial: 1,
            initial_kind: "aranha".to_string(),
            max: 30,
            panel: true,
            include_glb: false,
        }
    }
}

/// Heroi: personagem controlado pelo teclado que anda pela tela e por dentro dos aplicativos.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HeroConfig {
    pub enabled: bool,
    /// Altura do personagem (px).
    pub size_px: f32,
    pub walk_speed: f32,
    pub run_speed: f32,
    /// Andando devagar (Shift): nao cai das pontas.
    pub sneak_speed: f32,
    pub climb_speed: f32,
    /// Velocidade inicial do pulo (px/s) e gravidade (px/s2): altura = v2 / (2 g).
    pub jump_speed: f32,
    pub gravity: f32,
    pub double_jump: bool,
    pub max_hp: u32,
    /// Tempo sem levar dano depois de um golpe (s).
    pub invuln_secs: f32,
    pub respawn_secs: f32,
    /// Alcance e intervalo do golpe de espada (o dano e o da espada atual, como no clique).
    pub attack_reach_px: f32,
    pub attack_cooldown_secs: f32,
    /// O golpe do heroi vale este multiplo do dano da espada atual; o pisao, este outro.
    pub damage_multiplier: f32,
    pub stomp_multiplier: f32,
    /// Impulso para cima ao pisar num bicho (px/s).
    pub stomp_bounce: f32,
    /// Dano de encostar num bicho (elite tira o dobro).
    pub contact_damage: u32,
    /// Anda por dentro dos aplicativos: pede a imagem das janelas a plataforma para achar as
    /// linhas de texto e bordas (no GNOME, pela extensao; as imagens nao saem do computador).
    pub inside_apps: bool,
    /// Trechos menores que isso (px) nao viram apoio.
    pub min_ledge_px: f32,
    /// Moedas espalhadas ao mesmo tempo.
    pub coins: u32,
    pub xp_per_coin: u32,
    pub xp_per_star: u32,
    /// XP por pisar pela primeira vez numa janela/elemento novo.
    pub xp_per_discovery: u32,
    /// Os bichos reagem ao heroi como reagem ao cursor.
    pub mobs_notice: bool,
}

impl Default for HeroConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            size_px: 60.0,
            walk_speed: 230.0,
            run_speed: 430.0,
            sneak_speed: 110.0,
            climb_speed: 210.0,
            jump_speed: 780.0,
            gravity: 2100.0,
            double_jump: true,
            max_hp: 5,
            invuln_secs: 1.6,
            respawn_secs: 3.0,
            attack_reach_px: 62.0,
            attack_cooldown_secs: 0.32,
            damage_multiplier: 2.0,
            stomp_multiplier: 3.0,
            stomp_bounce: 560.0,
            contact_damage: 1,
            inside_apps: true,
            min_ledge_px: 24.0,
            coins: 6,
            xp_per_coin: 2,
            xp_per_star: 12,
            xp_per_discovery: 3,
            mobs_notice: true,
        }
    }
}

/// Teia: com o mouse parado por `idle_secs`, um mob leva o cursor para uma teia.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WebConfig {
    pub enabled: bool,
    /// Segundos com o mouse parado ate um mob vir buscar o cursor.
    pub idle_secs: f32,
    /// "build_first" (tece a teia e depois leva o mouse), "wrap" (leva o mouse e tece por cima)
    /// ou "random" (sorteia a cada vez).
    pub mode: String,
    /// Vida da teia (dano das espadas: madeira 2, ferro/ouro 4, diamante 8).
    pub web_hp: f32,
    pub radius_px: f32,
    /// Distancia aproximada entre o cursor e a teia.
    pub spot_distance_px: f32,
    pub build_secs: f32,
    /// Velocidade do cacador indo ate a teia/cursor e arrastando o cursor (px/s).
    pub hunt_speed: f32,
    pub drag_speed: f32,
    /// Seguranca: a teia se desfaz sozinha depois disso (s).
    pub max_trap_secs: f32,
    /// Espera extra depois que o jogador se solta (s).
    pub cooldown_secs: f32,
    /// Canto da tela onde fica a teia: "top_left", "top_right", "bottom_left" ou "bottom_right".
    pub corner: String,
    /// Comprimento do fio em que o cursor fica pendurado.
    pub rope_px: f32,
    /// Vida do fio: cliques tiram o dano da espada; sacudir o mouse tira `struggle_damage` por 1000 px.
    pub thread_hp: f32,
    pub struggle_damage: f32,
    /// Paradas para brincar (ioio) antes de levar o cursor para a teia, e a duracao de cada uma.
    pub play_stops: u32,
    pub play_secs: f32,
    /// XP por se soltar (cortar o fio ou destruir a teia).
    pub xp_reward: u32,
    /// Depois de arrancada, a teia fica fora por N s; entao uma aranha vai ao canto e tece outra
    /// em `build_secs` (indo a `hunt_speed` px/s).
    pub rebuild_secs: f32,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            idle_secs: 20.0,
            mode: "random".into(),
            web_hp: 12.0,
            radius_px: 70.0,
            spot_distance_px: 260.0,
            build_secs: 4.0,
            hunt_speed: 300.0,
            drag_speed: 180.0,
            max_trap_secs: 30.0,
            cooldown_secs: 20.0,
            corner: "top_left".into(),
            rope_px: 90.0,
            thread_hp: 8.0,
            struggle_damage: 1.0,
            play_stops: 2,
            play_secs: 3.5,
            xp_reward: 15,
            rebuild_secs: 25.0,
        }
    }
}

/// Combate: vida dos mobs, dano das espadas e XP para evoluir (madeira -> ferro -> ouro -> diamante).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CombatConfig {
    pub mob_hp: f32,
    pub damage_wood: f32,
    pub damage_iron: f32,
    pub damage_gold: f32,
    pub damage_diamond: f32,
    pub xp_per_hit: u32,
    pub xp_per_kill: u32,
    /// O ouro tem o dano do ferro, mas rende mais XP.
    pub gold_xp_multiplier: f32,
    /// XP total para desbloquear cada espada.
    pub xp_iron: u32,
    pub xp_gold: u32,
    pub xp_diamond: u32,
    /// Por quantos segundos a barra de vida fica visivel apos um golpe.
    pub health_bar_secs: f32,
}

impl Default for CombatConfig {
    fn default() -> Self {
        Self {
            mob_hp: 10.0,
            damage_wood: 2.0,
            damage_iron: 4.0,
            damage_gold: 4.0,
            damage_diamond: 8.0,
            xp_per_hit: 1,
            xp_per_kill: 10,
            gold_xp_multiplier: 2.0,
            xp_iron: 30,
            xp_gold: 90,
            xp_diamond: 250,
            health_bar_secs: 3.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GameConfig {
    pub points_per_kill: u32,
}

impl Default for GameConfig {
    fn default() -> Self {
        Self {
            points_per_kill: 100,
        }
    }
}

/// Ninhos em janelas paradas, sacudir a janela para derrubar os mobs e mordida dos elites.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NestsConfig {
    pub enabled: bool,
    /// Janela parada por este tempo (s) passa a aceitar ninho.
    pub calm_secs: f32,
    /// Chance por segundo de um mob em repouso numa janela calma construir um ninho.
    pub build_chance_per_sec: f32,
    /// Tempo de construcao (s): o ninho cresce e ainda nao choca.
    pub build_secs: f32,
    pub nest_hp: f32,
    /// Raio do ninho pronto (px); tambem e o raio clicavel.
    pub nest_radius_px: f32,
    pub max_per_window: u32,
    pub max_total: u32,
    /// Intervalo entre filhotes de um ninho pronto (s).
    pub hatch_interval_secs: f32,
    /// Idade (s) em que o ninho passa a chocar elites.
    pub elite_after_secs: f32,
    /// Vida do elite = `combat.mob_hp` vezes isto.
    pub elite_hp_multiplier: f32,
    /// XP extra por destruir um ninho (alem do XP normal de golpe/abate).
    pub xp_per_nest: u32,
    /// Inversoes de sentido para o arrasto contar como sacudida...
    pub shake_reversals: u32,
    /// ...dentro desta janela de tempo (s)...
    pub shake_window_secs: f32,
    /// ...cada uma depois de um trecho de pelo menos isto (px).
    pub shake_min_px: f32,
    /// Intervalo minimo (s) entre duas sacudidas da mesma janela.
    pub shake_cooldown_secs: f32,
    /// Periodo da consulta de janelas (ms) enquanto alguma janela esta se movendo.
    pub shake_poll_ms: u64,
    /// Vida que cada sacudida tira dos ninhos da janela (nunca destroi: sobra 1).
    pub shake_nest_damage: f32,
    /// Dano que o mob derrubado leva ao pousar.
    pub fall_damage: f32,
    /// Tempo atordoado apos o pouso (s).
    pub stun_secs: f32,
    /// Elite morde quando o cursor fica a menos disto (px)...
    pub bite_radius_px: f32,
    /// ...por este tempo seguido (s).
    pub bite_windup_secs: f32,
    /// XP roubado por mordida (a espada ja conquistada nunca e perdida).
    pub bite_xp: u32,
    pub bite_cooldown_secs: f32,
}

impl Default for NestsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            calm_secs: 45.0,
            build_chance_per_sec: 0.15,
            build_secs: 6.0,
            nest_hp: 30.0,
            nest_radius_px: 22.0,
            max_per_window: 1,
            max_total: 4,
            hatch_interval_secs: 20.0,
            elite_after_secs: 120.0,
            elite_hp_multiplier: 2.5,
            xp_per_nest: 25,
            shake_reversals: 3,
            shake_window_secs: 1.2,
            shake_min_px: 60.0,
            shake_cooldown_secs: 2.5,
            shake_poll_ms: 50,
            shake_nest_damage: 8.0,
            fall_damage: 3.0,
            stun_secs: 1.5,
            bite_radius_px: 90.0,
            bite_windup_secs: 1.2,
            bite_xp: 5,
            bite_cooldown_secs: 4.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DiagnosticsConfig {
    /// Intervalo do log de metricas (s). 0 = desligado.
    pub metrics_interval_secs: f32,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            metrics_interval_secs: 30.0,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, Resource)]
#[serde(default)]
pub struct Config {
    pub overlay: OverlayConfig,
    pub spider: SpiderConfig,
    pub behavior: BehaviorConfig,
    pub navigation: NavigationConfig,
    pub mobs: MobsConfig,
    pub combat: CombatConfig,
    pub nests: NestsConfig,
    pub web: WebConfig,
    pub hero: HeroConfig,
    pub game: GameConfig,
    pub diagnostics: DiagnosticsConfig,
}

impl Config {
    pub fn from_toml_str(text: &str) -> Result<Self, ConfigError> {
        let mut cfg: Config = toml::from_str(text).map_err(ConfigError::Parse)?;
        cfg.sanitize();
        Ok(cfg)
    }

    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(ConfigError::Io)?;
        Self::from_toml_str(&text)
    }

    /// Carrega o arquivo; em qualquer falha devolve o padrao e a mensagem de aviso.
    pub fn load_or_default(path: &Path) -> (Self, Option<String>) {
        match Self::load(path) {
            Ok(cfg) => (cfg, None),
            Err(ConfigError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => (
                Self::default(),
                Some(format!("{} nao encontrado; usando padroes", path.display())),
            ),
            Err(e) => (
                Self::default(),
                Some(format!("{}: {e}; usando padroes", path.display())),
            ),
        }
    }

    /// Configuracao efetiva para um monitor com escala `dpi` (1.0 = 100%, 1.5 = 150%...):
    /// tudo que e medido em pixels fisicos cresce junto com a interface do sistema, para a
    /// aranha manter o mesmo tamanho aparente em relacao aos aplicativos.
    pub fn scaled(&self, dpi: f32) -> Config {
        let k = if dpi.is_finite() {
            dpi.clamp(0.5, 4.0)
        } else {
            1.0
        };
        let mut c = self.clone();
        c.spider.scale *= k;
        c.spider.walk_speed *= k;
        c.spider.run_speed *= k;
        c.spider.climb_speed *= k;
        c.behavior.detect_radius_px *= k;
        c.navigation.link_distance_px *= k;
        c.navigation.min_window_size_px *= k;
        c.navigation.min_segment_px *= k;
        c.nests.nest_radius_px *= k;
        c.nests.shake_min_px *= k;
        c.nests.bite_radius_px *= k;
        c.web.radius_px *= k;
        c.web.spot_distance_px *= k;
        c.web.hunt_speed *= k;
        c.web.drag_speed *= k;
        c.web.rope_px *= k;
        c.hero.size_px *= k;
        c.hero.walk_speed *= k;
        c.hero.run_speed *= k;
        c.hero.sneak_speed *= k;
        c.hero.climb_speed *= k;
        c.hero.jump_speed *= k;
        c.hero.gravity *= k;
        c.hero.attack_reach_px *= k;
        c.hero.stomp_bounce *= k;
        c.hero.min_ledge_px *= k;
        c
    }

    /// Mantem valores dentro de faixas seguras.
    pub fn sanitize(&mut self) {
        let h = &mut self.hero;
        h.size_px = h.size_px.clamp(16.0, 200.0);
        h.walk_speed = h.walk_speed.clamp(20.0, 2000.0);
        h.run_speed = h.run_speed.clamp(h.walk_speed, 4000.0);
        h.sneak_speed = h.sneak_speed.clamp(10.0, h.walk_speed);
        h.climb_speed = h.climb_speed.clamp(20.0, 2000.0);
        h.jump_speed = h.jump_speed.clamp(100.0, 4000.0);
        h.gravity = h.gravity.clamp(200.0, 20_000.0);
        h.max_hp = h.max_hp.clamp(1, 20);
        h.invuln_secs = h.invuln_secs.clamp(0.1, 30.0);
        h.respawn_secs = h.respawn_secs.clamp(0.5, 120.0);
        h.attack_reach_px = h.attack_reach_px.clamp(10.0, 600.0);
        h.attack_cooldown_secs = h.attack_cooldown_secs.clamp(0.05, 10.0);
        h.min_ledge_px = h.min_ledge_px.clamp(8.0, 400.0);
        h.damage_multiplier = h.damage_multiplier.clamp(0.1, 100.0);
        h.stomp_multiplier = h.stomp_multiplier.clamp(0.1, 100.0);
        h.coins = h.coins.min(40);
        let w = &mut self.web;
        w.idle_secs = w.idle_secs.clamp(2.0, 3600.0);
        w.rope_px = w.rope_px.clamp(20.0, 400.0);
        w.thread_hp = w.thread_hp.clamp(1.0, 1000.0);
        w.struggle_damage = w.struggle_damage.clamp(0.0, 100.0);
        w.play_stops = w.play_stops.min(6);
        w.play_secs = w.play_secs.clamp(0.5, 20.0);
        w.rebuild_secs = w.rebuild_secs.clamp(1.0, 3600.0);
        let o = &mut self.overlay;
        o.fps_limit = o.fps_limit.clamp(5, 240);
        o.idle_update_hz = o.idle_update_hz.clamp(1.0, 60.0);
        o.hidden_update_hz = o.hidden_update_hz.clamp(1.0, 30.0);
        o.watchdog_secs = o.watchdog_secs.clamp(2.0, 120.0);
        o.bottom_inset_px = o.bottom_inset_px.min(64);
        o.hit_hysteresis = o.hit_hysteresis.clamp(1.0, 3.0);
        o.msaa_samples = match o.msaa_samples {
            0 | 1 => 1,
            2 | 3 => 2,
            _ => 4,
        };

        let s = &mut self.spider;
        s.scale = s.scale.clamp(0.25, 8.0);
        s.hit_radius_px = s.hit_radius_px.clamp(8.0, 400.0);
        s.walk_speed = s.walk_speed.clamp(5.0, 2000.0);
        s.run_speed = s.run_speed.clamp(s.walk_speed, 4000.0);
        s.climb_speed = s.climb_speed.clamp(5.0, 2000.0);
        s.view_tilt_deg = s.view_tilt_deg.clamp(0.0, 80.0);
        s.glb_scale = s.glb_scale.clamp(0.001, 1000.0);

        let b = &mut self.behavior;
        b.detect_radius_px = b.detect_radius_px.clamp(10.0, 2000.0);
        b.react_probability = b.react_probability.clamp(0.0, 1.0);
        b.idle_min_secs = b.idle_min_secs.clamp(0.05, 600.0);
        b.idle_max_secs = b.idle_max_secs.clamp(b.idle_min_secs, 600.0);
        b.pause_chance_per_sec = b.pause_chance_per_sec.clamp(0.0, 5.0);
        b.flee_duration_secs = b.flee_duration_secs.clamp(0.2, 60.0);
        b.respawn_delay_secs = b.respawn_delay_secs.clamp(0.5, 86_400.0);
        b.death_anim_secs = b.death_anim_secs.clamp(0.1, 10.0);
        b.react_cooldown_secs = b.react_cooldown_secs.clamp(0.0, 600.0);
        for w in [
            &mut b.weight_flee,
            &mut b.weight_freeze,
            &mut b.weight_speedup,
            &mut b.weight_reroute,
        ] {
            *w = w.clamp(0.0, 1000.0);
        }

        let k = &mut self.combat;
        k.mob_hp = k.mob_hp.clamp(1.0, 100_000.0);
        for d in [
            &mut k.damage_wood,
            &mut k.damage_iron,
            &mut k.damage_gold,
            &mut k.damage_diamond,
        ] {
            *d = d.clamp(0.1, 100_000.0);
        }
        k.gold_xp_multiplier = k.gold_xp_multiplier.clamp(0.0, 100.0);
        k.xp_gold = k.xp_gold.max(k.xp_iron);
        k.xp_diamond = k.xp_diamond.max(k.xp_gold);
        k.health_bar_secs = k.health_bar_secs.clamp(0.2, 60.0);

        let t = &mut self.nests;
        t.calm_secs = t.calm_secs.clamp(0.0, 86_400.0);
        t.build_chance_per_sec = t.build_chance_per_sec.clamp(0.0, 100.0);
        t.build_secs = t.build_secs.clamp(0.1, 3600.0);
        t.nest_hp = t.nest_hp.clamp(1.0, 100_000.0);
        t.nest_radius_px = t.nest_radius_px.clamp(6.0, 200.0);
        t.max_per_window = t.max_per_window.min(8);
        t.max_total = t.max_total.min(64);
        t.hatch_interval_secs = t.hatch_interval_secs.clamp(1.0, 86_400.0);
        t.elite_after_secs = t.elite_after_secs.clamp(0.0, 86_400.0);
        t.elite_hp_multiplier = t.elite_hp_multiplier.clamp(1.0, 100.0);
        t.shake_reversals = t.shake_reversals.clamp(2, 20);
        t.shake_window_secs = t.shake_window_secs.clamp(0.3, 10.0);
        t.shake_min_px = t.shake_min_px.clamp(5.0, 2000.0);
        t.shake_cooldown_secs = t.shake_cooldown_secs.clamp(0.0, 600.0);
        t.shake_poll_ms = t.shake_poll_ms.clamp(16, 1000);
        t.shake_nest_damage = t.shake_nest_damage.clamp(0.0, 100_000.0);
        t.fall_damage = t.fall_damage.clamp(0.0, 100_000.0);
        t.stun_secs = t.stun_secs.clamp(0.0, 60.0);
        t.bite_radius_px = t.bite_radius_px.clamp(10.0, 2000.0);
        t.bite_windup_secs = t.bite_windup_secs.clamp(0.1, 60.0);
        t.bite_cooldown_secs = t.bite_cooldown_secs.clamp(0.5, 600.0);

        let w = &mut self.web;
        w.idle_secs = w.idle_secs.clamp(1.0, 86_400.0);
        if !matches!(w.mode.as_str(), "build_first" | "wrap" | "random") {
            w.mode = "random".into();
        }
        w.web_hp = w.web_hp.clamp(0.1, 100_000.0);
        w.radius_px = w.radius_px.clamp(20.0, 500.0);
        w.spot_distance_px = w.spot_distance_px.clamp(0.0, 3000.0);
        w.build_secs = w.build_secs.clamp(0.1, 30.0);
        w.hunt_speed = w.hunt_speed.clamp(20.0, 5000.0);
        w.drag_speed = w.drag_speed.clamp(20.0, 5000.0);
        // seguranca: o cursor nunca fica preso por mais de 10 minutos
        w.max_trap_secs = w.max_trap_secs.clamp(3.0, 600.0);
        w.cooldown_secs = w.cooldown_secs.clamp(0.0, 86_400.0);
        self.mobs.max = self.mobs.max.clamp(1, 200);
        self.mobs.initial = self.mobs.initial.min(self.mobs.max);

        let n = &mut self.navigation;
        n.window_poll_ms = n.window_poll_ms.clamp(50, 10_000);
        n.link_distance_px = n.link_distance_px.clamp(1.0, 200.0);
        n.min_window_size_px = n.min_window_size_px.clamp(0.0, 4000.0);
        n.min_segment_px = n.min_segment_px.clamp(1.0, 1000.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_stable_under_sanitize() {
        let mut cfg = Config::default();
        let before = format!("{cfg:?}");
        cfg.sanitize();
        assert_eq!(before, format!("{cfg:?}"));
    }

    #[test]
    fn partial_toml_keeps_defaults() {
        let cfg = Config::from_toml_str("[spider]\nscale = 2.0\n").unwrap();
        assert_eq!(cfg.spider.scale, 2.0);
        assert_eq!(cfg.overlay.fps_limit, 60);
    }

    #[test]
    fn absurd_values_are_clamped() {
        let cfg = Config::from_toml_str(
            "[overlay]\nfps_limit = 100000\nmsaa_samples = 9\n[behavior]\nidle_min_secs = 50.0\nidle_max_secs = 1.0\n",
        )
        .unwrap();
        assert_eq!(cfg.overlay.fps_limit, 240);
        assert_eq!(cfg.overlay.msaa_samples, 4);
        assert!(cfg.behavior.idle_max_secs >= cfg.behavior.idle_min_secs);
    }

    #[test]
    fn invalid_toml_is_error_and_load_or_default_recovers() {
        assert!(Config::from_toml_str("[spider\nscale=").is_err());
        let (cfg, warn) = Config::load_or_default(Path::new("/definitely/not/here.toml"));
        assert!(warn.is_some());
        assert_eq!(cfg.overlay.fps_limit, 60);
    }

    #[test]
    fn dpi_scaling_multiplies_pixel_quantities_only() {
        let base = Config::default();
        let same = base.scaled(1.0);
        assert_eq!(format!("{base:?}"), format!("{same:?}"));

        let s150 = base.scaled(1.5);
        assert!((s150.spider.scale - 1.5).abs() < 1e-6);
        assert!((s150.spider.walk_speed - base.spider.walk_speed * 1.5).abs() < 1e-3);
        assert!(
            (s150.navigation.link_distance_px - base.navigation.link_distance_px * 1.5).abs()
                < 1e-3
        );
        assert!(
            (s150.behavior.detect_radius_px - base.behavior.detect_radius_px * 1.5).abs() < 1e-3
        );
        // grandezas que nao sao pixels ficam intactas
        assert_eq!(
            s150.behavior.respawn_delay_secs,
            base.behavior.respawn_delay_secs
        );
        assert_eq!(s150.overlay.fps_limit, base.overlay.fps_limit);
        // valores invalidos nao quebram
        assert_eq!(base.scaled(f32::NAN).spider.scale, 1.0);
        assert_eq!(base.scaled(0.0).spider.scale, 0.5);
        assert_eq!(base.scaled(100.0).spider.scale, 4.0);
    }

    #[test]
    fn shipped_default_toml_parses_and_matches_defaults() {
        let text = include_str!("../config/default.toml");
        let cfg = Config::from_toml_str(text).expect("config/default.toml deve ser valido");
        let def = Config::default();
        assert_eq!(format!("{cfg:?}"), format!("{def:?}"));
    }
}
