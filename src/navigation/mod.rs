//! Navegacao: geometria, grafo de caminhos derivado das janelas, A* e locomocao.
//!
//! Tudo aqui e logica pura (sem Win32, sem ECS) e coberta por testes unitarios.
//!
//! Evolucao futura (linhas internas dos aplicativos, ex.: via UI Automation): novas fontes de
//! superficies podem gerar `Surface`s adicionais em [`window_graph::PathGraph::build`], com
//! dono proprio, sem alterar o A* nem o `Walker`.

pub mod geometry;
pub mod pathfinding;
pub mod walker;
pub mod window_graph;

pub use geometry::{Rect, Segment, Side};
pub use pathfinding::{Leg, Route, choose_destination, plan_route};
pub use walker::{Reattach, Walker};
pub use window_graph::{BuildParams, EdgeId, GraphPoint, PathGraph, SurfaceOwner};
