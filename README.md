# SpiderDesk

Uma pequena aranha 3D que **vive sobre a área de trabalho do Windows**: anda pelas bordas das
janelas reais, escala as laterais, contorna cantos, foge do cursor e morre se você clicar nela
(e reaparece depois). Escrito em **Rust** com **Bevy 0.20** (wgpu), sem Electron/WebView.

> **Estado honesto deste protótipo.** O código foi desenvolvido e testado em **Linux**. Tudo que
> é lógica (navegação, comportamento, configuração, pontuação) tem testes automatizados que
> passam. O backend Win32 **compila e passa no clippy para o alvo Windows**, mas **nunca rodou em
> um Windows real**: transparência verdadeira, click-through seletivo e os 10 cenários nativos
> estão **pendentes** — veja [docs/TESTES_WINDOWS.md](docs/TESTES_WINDOWS.md) e a seção
> [Resultados dos testes](#resultados-dos-testes).

## Início rápido

```powershell
# Windows 10/11, Rust >= 1.97
cargo run --release -- --poc     # Etapa 1: cubo 3D transparente (valide a transparencia primeiro)
cargo run --release              # a aranha
```

Encerramento de emergência (independe da aranha e do mouse): **Ctrl+Alt+Shift+Q**
(configurável) — ou `taskkill /IM spiderdesk.exe /F`.

Linha de comando:

| Opção | O que faz |
|---|---|
| `--poc` | Prova de conceito: cubo 3D transparente com click-through, sem aranha |
| `--list-windows` | Lista as janelas detectadas (bordas visíveis × brutas) e sai — não precisa de GPU |
| `--screenshot arq.png` | Salva um quadro renderizado (RGBA) + estatística de alfa e sai |
| `--auto-kill N` | Teste: simula um clique na aranha após N s (e a cada reaparecimento) |
| `--timeout N` | Encerra sozinho após N segundos |
| `--config arq.toml` | Usa outro arquivo de configuração |
| `--print-config` | Mostra a configuração efetiva |

### Compilar para Windows a partir do Linux

```bash
rustup target add x86_64-pc-windows-gnu      # + pacote mingw-w64
scripts/package-windows.sh                   # gera dist/spiderdesk-windows/ (exe + config + assets)
```
O linker do alvo está em [.cargo/config.toml](.cargo/config.toml).

### Desenvolvimento em Linux

`cargo run` funciona (usa um desktop simulado em [src/platform/fallback.rs](src/platform/fallback.rs)),
útil para depurar render, animação e comportamento. `cargo test`, `cargo clippy` e `cargo fmt`
rodam em qualquer SO.

## Como funciona

```
src/
├── app.rs            monta o App Bevy + CLI
├── config.rs         TOML (serde) com padrões, validação e escala por DPI
├── desktop.rs        consulta periódica das janelas → reconstrói o grafo só se algo mudou
├── overlay/          janela transparente, click-through seletivo, emergência, ritmo do laço
├── platform/         TUDO que depende do SO (Win32 `unsafe` isolado em windows.rs)
├── navigation/       geometria, grafo de caminhos, A*, locomoção (lógica pura)
├── creature/         comportamento (máquina de estados), animação, aranha procedural, GLB, efeitos
├── game/             pontuação
├── capture.rs        --screenshot       diagnostics.rs  métricas       poc.rs  modo PoC
config/models/        presets (yaw) para os GLBs de animais testados
tools/make_test_glb.py  gera um GLB sintético de teste
tests/                cenários de desktop + simulação de 10 min simulados, sem GPU
```

### Overlay transparente

* Janela sem bordas, `transparent`, `AlwaysOnTop`, `skip_taskbar`, não-focável; cobre o monitor
  principal **menos 1 px de altura** (evita o modo "fullscreen otimizado" do Windows).
* **O Bevy 0.20 só lê o tipo de swapchain do DX12 por variável de ambiente.** O padrão
  (`DxgiFromHwnd`) **não suporta transparência**; por isso `platform::prepare_graphics_env()`
  define `WGPU_DX12_PRESENTATION_SYSTEM=Visual` (DirectComposition) e o backend DX12 é forçado
  no Windows. Alfa: `PreMultiplied`, `ClearColor = NONE`, sem tonemapping/HDR.
* Estilos nativos: `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` (sem roubar foco, fora do Alt+Tab);
  "sempre no topo" é reafirmado a cada 2 s.

### Click-through seletivo

A janela **nasce** com `hit_test = false` (cliques atravessam). A cada quadro o cursor global
(`GetCursorPos`) é comparado com o círculo clicável da aranha (já em pixels de tela):

* dentro do raio → `hit_test = true` (a janela recebe o clique; a aranha morre);
* fora de `raio × 1,1` → volta a `false` (histerese). **Atenção:** enquanto ligado, a janela inteira
  captura o mouse, então cliques no anel entre `raio` e `raio × 1,1` (~3 px) são engolidos sem
  efeito; por isso a histerese é pequena. O círculo clicável é só aproximação do modelo;
* nunca desliga com o botão pressionado (estado **físico** via `GetAsyncKeyState`, trava de 4 s);
* cursor desconhecido → click-through.

O winit recalcula o *ex-style* inteiro a cada troca de `hit_test` (verificado em
`winit-0.30.13/.../window_state.rs`: `SetWindowLongW(GWL_EXSTYLE, ...)`), o que apagaria
`WS_EX_NOACTIVATE|WS_EX_TOOLWINDOW`; por isso `platform::ensure_overlay_styles` devolve esses bits
**a cada quadro** (leitura-modificação-escrita, preserva `TRANSPARENT/LAYERED`).

Não se usa `WM_NCHITTEST/HTTRANSPARENT` (não atravessa para outros processos).

### Segurança (nunca bloquear o computador)

Thread própria (`platform::Emergency`): `RegisterHotKey` + polling de reserva para o atalho;
**watchdog** que encerra o processo se o laço principal ficar > `watchdog_secs` sem bater;
saída forçada 3 s depois de qualquer pedido de encerramento que não complete.

### Janelas e caminhos

* Enumeração (`EnumWindows`) a cada `window_poll_ms` (300 ms), **nunca por quadro**; filtra
  invisíveis, minimizadas, *cloaked* (DWM), `TOOLWINDOW`, janelas com dono, sem título, shell
  (barra de tarefas/desktop) e o próprio processo. Limites visíveis via
  `DWMWA_EXTENDED_FRAME_BOUNDS` (reserva: `GetWindowRect`). O processo é DPI *per-monitor v2*;
  tudo é em **pixels físicos**.
* `PathGraph`: bordas visíveis das janelas (recortadas pelas janelas **na frente** na ordem Z),
  perímetro da tela, cantos compartilhados (contornar cantos) e *links* entre bordas de janelas
  a `< link_distance_px` (pontos de encontro). Reconstruído **só quando o conjunto muda**.
* A* sobre o grafo; destinos sorteados por comprimento de aresta; fuga escolhe o destino mais
  distante do cursor.
* Janela movida/redimensionada: a aranha **viaja junto** (mapeamento proporcional ao retângulo);
  a rota é revalidada; se a superfície sumiu (minimizada), **cai** até a de baixo.

### Comportamento

Máquina de estados `Idle · Walking · Climbing · Fleeing · Attacking (reservado) · Hit · Dead ·
Respawning`. Velocidade com variação suave e micro-pausas; ao cursor **entrar** no raio de
detecção (gatilho por entrada + *cooldown* + probabilidade) ela foge, congela, acelera ou muda
de rota. Clique: *Hit* → *Dead* (animação + explosão) → pontuação → some → reaparece.

### Render e animação

Câmera ortográfica 1 unidade = 1 px físico; o corpo é inclinado em direção à câmera
(`view_tilt_deg`) e a orientação acompanha a **normal** da superfície (escalando parede, pendurada
no teto) com `slerp` suave. Sem `spider.glb`, usa-se uma aranha procedural com marcha de
tetrápodes e IK de duas juntas (pés sem "patinar"). Com GLB
(`assets/models/spider.glb` ou `spider.model`): **auto-fit** de escala/posição (mede os ossos de modelos
com esqueleto), animações por nome com seleção que evita variantes ("walking backward", "turn left"),
clipe único genérico vira `walk`, clipes ausentes usam *fallback* e repouso/golpe/morte sem clipe
**congelam** o andar em vez de andar no lugar. Veja [assets/models/README.md](assets/models/README.md)
e os presets de `config/models/`.

## Configuração

[config/default.toml](config/default.toml) documenta tudo (todos os campos são opcionais).
Principais: `overlay.fps_limit`, `overlay.emergency_hotkey`, `spider.scale/speeds/model`,
`behavior.detect_radius_px/respawn_delay_secs/...`, `navigation.window_poll_ms/link_distance_px`,
`diagnostics.metrics_interval_secs`. Valores em px escalam automaticamente com o DPI do monitor.

## Desempenho

* Laço dirigido por tempo (`reactive_low_power`): `fps_limit` (60) em movimento,
  `idle_update_hz` (20) parada com cursor longe, `hidden_update_hz` (4) morta/invisível
  (a lógica usa o tempo **real** decorrido, então o intervalo de respawn vale mesmo a 4 Hz;
  há teste de regressão para isso).
* Sem consulta de janelas por quadro; grafo reconstruído só em mudança; sem sombras;
  MSAA desligado por padrão; sem alocações por quadro no caminho quente.
* Métricas no log a cada `metrics_interval_secs`: FPS, tempo de quadro, atividade, consultas,
  reconstruções (tempo máx.), nós/arestas, memória. Medidas feitas **só no Linux** (consulta de
  janelas ≈ 10–18 µs, reconstrução do grafo ≈ 60 µs, ~265 MiB residentes com Vulkan); **não há
  medição de CPU/GPU no Windows ainda**.
* Custo conhecido: a janela cobre o monitor inteiro (superfície transparente de tela cheia).
  Alternativa futura: janela pequena que segue a aranha.

## Resultados dos testes

### Executados (Linux, fora do Windows)

| Verificação | Resultado |
|---|---|
| `cargo test` | **115 testes passam** (100 unitários + 7 cenários de desktop + 7 simulações + 1 de configuração) |
| `cargo fmt --check`, `cargo clippy --all-targets` (Linux) e `--target x86_64-pc-windows-gnu` | `Finished`, **0 avisos** nos dois alvos, sobre o código final |
| `cargo check` e `cargo build --release --target x86_64-pc-windows-gnu` | OK (gera `spiderdesk.exe`, ~64 MB) — a única validação de compilação do módulo Win32 |
| **Wine** (somente *smoke*, **não** é validação do Windows) | `spiderdesk.exe --help` e `--print-config` OK; `--list-windows` executa `EnumWindows`/DPI sem falhar (0 janelas, esperado num prefixo vazio) |
| Execução real no Linux (Wayland/XWayland, Vulkan) | janela dimensionada, aranha posicionada e animada, saída limpa por `--timeout`; 5/5 capturas com aranha |
| Transparência **do render** (`--screenshot`) | fundo com alfa 0 em ~99,9 % dos pixels, aranha/cubo com alfa 255, **sem pixels parciais** |
| GLB sintético (`tools/make_test_glb.py`) e GLB corrompido | carga + fallback de clipes OK; corrompido → erro registrado e aranha procedural assume |
| **5 animais reais** (lobo-aranha rigado, formiga, barata, gafanhoto, rato) | todos carregam, renderizam texturizados, com `AnimationPlayer` ligado, escala/posição automáticas e orientação correta com o preset de `config/models/` (veja [assets/models/README.md](assets/models/README.md)) |
| Ciclo morte→respawn no **app real** (`--auto-kill`, lobo-aranha) | clique → pontos com bônus de sequência (+100, +125, +150) → atividade `Hidden` (4 Hz) → reaparece no tempo configurado |
| Simulação de 10 min simulados | aranha sempre sobre uma superfície, sem NaN, dentro da tela |

Cobertura dos cenários obrigatórios **no nível da lógica** (automatizada): 5 (mover janela →
rebuild + aranha acompanha), 6 (minimizar → caminhos somem → queda), 7 (maximizada),
8 (grafo invariante à escala DPI 100/125/150 %), 9 (mudança de ordem Z detectada e oclusão
recalculada), morte/respawn e fuga do cursor.

### **Pendentes — exigem Windows real** (nenhum foi executado)

Os 10 cenários da especificação *como experiência nativa*: 1 Chrome, 2 VS Code, 3 clique fora
chegando ao app certo, 4 clique matando a aranha com o clique **não** vazando, 5/6/7 com janelas
reais, 8 DPI real, 9 Alt+Tab real, 10 encerramento — mais **transparência real sobre o DWM** e
**click-through real**. Roteiro passo a passo em [docs/TESTES_WINDOWS.md](docs/TESTES_WINDOWS.md).

## Limitações conhecidas

* **Risco nº 1 (não validado):** ao alternar o click-through o winit liga `WS_EX_LAYERED` via
  `SetWindowLongW` e **nunca chama** `SetLayeredWindowAttributes`/`UpdateLayeredWindow` (conferido no
  código-fonte do winit 0.30.13); o Win32 documenta que uma janela *layered* assim pode não ser
  exibida. Se o overlay sumir/piscar ao passar o cursor sobre a aranha: 1) ligue
  `overlay.layered_alpha_fix = true` (aplica alfa constante 255, sem mudar a aparência — remédio
  hipotético, também não testado); 2) plano B: `SetWindowRgn` ou janela pequena que segue a aranha.
* Só o **monitor principal** é coberto; janelas em outros monitores são ignoradas.
* Jogos em tela cheia exclusiva e janelas elevadas (UAC) ficam acima do overlay.
* O watchdog pode encerrar o app ao retomar de suspensão (ajuste `watchdog_secs`).
* Em Linux/X11 o `hit_test` não é suportado pelo winit (só afeta o desenvolvimento).
* A pontuação não é persistida; só o monitor principal e uma aranha.
* Uma captura (`--screenshot`) veio vazia uma única vez, durante compilações em paralelo com a
  máquina a load average ~20; 5 repetições seguidas e outras capturas posteriores saíram normais.
  Não consegui reproduzir (provável atraso de quadros sob carga), mas fica registrado.

## Evolução futura

* **Linhas internas** (painéis, separadores, barras de ferramentas): novas fontes de `Surface`
  em `PathGraph::build`, via **Windows UI Automation** (nem todo app expõe a geometria);
  sem visão computacional.
* Estado `Attacking`, persistência de pontos, vários monitores, janela que segue a aranha,
  ícone de bandeja.
