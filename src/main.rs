//! Ponto de entrada. Toda a logica esta na biblioteca (`spiderdesk`).
//!
//! O executavel usa o subsistema de console do Windows de proposito: assim `--help`,
//! `--list-windows` e os logs funcionam no terminal. Para uso diario, inicie-o minimizado
//! (atalho "Executar: Minimizada") ou via `start /min`.

fn main() -> std::process::ExitCode {
    spiderdesk::app::run_from_env()
}
