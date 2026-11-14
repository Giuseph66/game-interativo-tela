//! Combate: espadas, dano, XP e evolucao.
//!
//! Evolucao por XP: madeira -> ferro -> ouro -> diamante. Ferro e ouro dao o MESMO dano; o ouro
//! rende mais XP por golpe/abate (acelera a chegada ao diamante).

use bevy::prelude::Resource;

use crate::config::CombatConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Sword {
    Wood,
    Iron,
    Gold,
    Diamond,
}

impl Sword {
    pub const ALL: [Sword; 4] = [Sword::Wood, Sword::Iron, Sword::Gold, Sword::Diamond];

    pub fn label(self) -> &'static str {
        match self {
            Sword::Wood => "Madeira",
            Sword::Iron => "Ferro",
            Sword::Gold => "Ouro",
            Sword::Diamond => "Diamante",
        }
    }

    /// Arquivo do cursor (relativo a `assets/`).
    pub fn cursor_path(self) -> &'static str {
        match self {
            Sword::Wood => "cursors/sword_wood.png",
            Sword::Iron => "cursors/sword_iron.png",
            Sword::Gold => "cursors/sword_gold.png",
            Sword::Diamond => "cursors/sword_diamond.png",
        }
    }

    pub fn damage(self, c: &CombatConfig) -> f32 {
        match self {
            Sword::Wood => c.damage_wood,
            Sword::Iron => c.damage_iron,
            Sword::Gold => c.damage_gold,
            Sword::Diamond => c.damage_diamond,
        }
    }

    /// Multiplicador de XP (o ouro rende mais).
    pub fn xp_multiplier(self, c: &CombatConfig) -> f32 {
        match self {
            Sword::Gold => c.gold_xp_multiplier,
            _ => 1.0,
        }
    }

    /// XP total necessario para desbloquear esta espada.
    pub fn xp_required(self, c: &CombatConfig) -> u32 {
        match self {
            Sword::Wood => 0,
            Sword::Iron => c.xp_iron,
            Sword::Gold => c.xp_gold,
            Sword::Diamond => c.xp_diamond,
        }
    }

    pub fn next(self) -> Option<Sword> {
        match self {
            Sword::Wood => Some(Sword::Iron),
            Sword::Iron => Some(Sword::Gold),
            Sword::Gold => Some(Sword::Diamond),
            Sword::Diamond => None,
        }
    }

    /// Melhor espada desbloqueada com `xp`.
    pub fn for_xp(xp: u32, c: &CombatConfig) -> Sword {
        Sword::ALL
            .iter()
            .rev()
            .copied()
            .find(|s| xp >= s.xp_required(c))
            .unwrap_or(Sword::Wood)
    }
}

/// Progresso do jogador (XP e espada atual).
#[derive(Debug, Clone, Resource)]
pub struct Progress {
    pub xp: u32,
    pub sword: Sword,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            xp: 0,
            sword: Sword::Wood,
        }
    }
}

/// Resultado de um golpe do jogador.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Strike {
    pub damage: f32,
    pub xp_gained: u32,
    pub killed: bool,
    /// Espada nova desbloqueada por este golpe.
    pub upgraded: Option<Sword>,
}

impl Progress {
    /// Soma XP e evolui a espada se passar do limiar. Devolve a espada nova, se houver.
    pub fn add_xp(&mut self, amount: u32, c: &CombatConfig) -> Option<Sword> {
        self.xp = self.xp.saturating_add(amount);
        let best = Sword::for_xp(self.xp, c);
        (best > self.sword).then(|| {
            self.sword = best;
            best
        })
    }

    /// Golpe com a espada atual num alvo com `hp` de vida. Aplica o dano em `hp` e o XP aqui.
    pub fn strike(&mut self, hp: &mut f32, c: &CombatConfig) -> Strike {
        let sword = self.sword;
        let damage = sword.damage(c);
        *hp = (*hp - damage).max(0.0);
        let killed = *hp <= 0.0;
        let base = c.xp_per_hit + if killed { c.xp_per_kill } else { 0 };
        let xp_gained = (base as f32 * sword.xp_multiplier(c)).round() as u32;
        let upgraded = self.add_xp(xp_gained, c);
        Strike {
            damage,
            xp_gained,
            killed,
            upgraded,
        }
    }

    /// Fracao (0..1) do caminho ate a proxima espada; `None` no diamante.
    pub fn progress_to_next(&self, c: &CombatConfig) -> Option<(Sword, f32)> {
        let next = self.sword.next()?;
        let lo = self.sword.xp_required(c) as f32;
        let hi = next.xp_required(c) as f32;
        Some((
            next,
            ((self.xp as f32 - lo) / (hi - lo).max(1.0)).clamp(0.0, 1.0),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> CombatConfig {
        CombatConfig::default()
    }

    #[test]
    fn damage_order_wood_lt_iron_eq_gold_lt_diamond() {
        let c = cfg();
        assert!(Sword::Wood.damage(&c) < Sword::Iron.damage(&c));
        assert_eq!(Sword::Iron.damage(&c), Sword::Gold.damage(&c));
        assert!(Sword::Gold.damage(&c) < Sword::Diamond.damage(&c));
        // ouro rende mais XP
        assert!(Sword::Gold.xp_multiplier(&c) > Sword::Iron.xp_multiplier(&c));
    }

    #[test]
    fn sword_follows_xp_thresholds_in_order() {
        let c = cfg();
        assert_eq!(Sword::for_xp(0, &c), Sword::Wood);
        assert_eq!(Sword::for_xp(c.xp_iron - 1, &c), Sword::Wood);
        assert_eq!(Sword::for_xp(c.xp_iron, &c), Sword::Iron);
        assert_eq!(Sword::for_xp(c.xp_gold, &c), Sword::Gold);
        assert_eq!(Sword::for_xp(c.xp_diamond, &c), Sword::Diamond);
        assert_eq!(Sword::for_xp(u32::MAX, &c), Sword::Diamond);
    }

    #[test]
    fn strike_damages_kills_and_awards_xp() {
        let c = cfg();
        let mut p = Progress::default();
        let mut hp = c.mob_hp;
        let mut hits = 0;
        loop {
            let s = p.strike(&mut hp, &c);
            hits += 1;
            assert!(hp >= 0.0);
            if s.killed {
                assert_eq!(s.xp_gained, c.xp_per_hit + c.xp_per_kill);
                break;
            }
            assert_eq!(s.xp_gained, c.xp_per_hit);
            assert!(hits < 100);
        }
        // espada de madeira precisa de varios golpes
        assert_eq!(hits, (c.mob_hp / c.damage_wood).ceil() as i32);
    }

    #[test]
    fn evolution_reports_each_upgrade_once_and_gold_speeds_up_xp() {
        let c = cfg();
        let mut p = Progress::default();
        let mut upgrades = Vec::new();
        let mut kills_with = std::collections::HashMap::new();
        while p.sword != Sword::Diamond {
            let mut hp = c.mob_hp;
            let sword = p.sword;
            loop {
                let s = p.strike(&mut hp, &c);
                if let Some(u) = s.upgraded {
                    upgrades.push(u);
                }
                if s.killed {
                    *kills_with.entry(sword).or_insert(0u32) += 1;
                    break;
                }
            }
        }
        assert_eq!(upgrades, vec![Sword::Iron, Sword::Gold, Sword::Diamond]);
        // golpe com ouro rende o dobro do ferro
        let mut a = Progress {
            xp: c.xp_iron,
            sword: Sword::Iron,
        };
        let mut b = Progress {
            xp: c.xp_gold,
            sword: Sword::Gold,
        };
        let (mut ha, mut hb) = (1000.0, 1000.0);
        assert!(b.strike(&mut hb, &c).xp_gained > a.strike(&mut ha, &c).xp_gained);
    }

    #[test]
    fn progress_fraction() {
        let c = cfg();
        let p = Progress {
            xp: 0,
            sword: Sword::Wood,
        };
        assert_eq!(p.progress_to_next(&c), Some((Sword::Iron, 0.0)));
        let p = Progress {
            xp: c.xp_diamond,
            sword: Sword::Diamond,
        };
        assert_eq!(p.progress_to_next(&c), None);
    }
}
