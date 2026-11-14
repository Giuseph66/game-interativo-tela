//! SpiderDesk: uma aranha 3D que vive sobre a area de trabalho do Windows.
//!
//! A logica (navegacao, comportamento, config, pontuacao) e pura e testavel em qualquer SO;
//! o Win32 fica isolado em [`platform`].

pub mod app;
pub mod assets;
pub mod capture;
pub mod config;
pub mod creature;
pub mod desktop;
pub mod diagnostics;
pub mod game;
pub mod hero;
pub mod hud;
pub mod mobs;
pub mod navigation;
pub mod nests;
pub mod overlay;
pub mod panel;
pub mod platform;
pub mod poc;
pub mod rng;
pub mod web;
