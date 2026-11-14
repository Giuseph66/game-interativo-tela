# Modelos 3D

Coloque aqui o modelo da aranha como **`spider.glb`**. Se o arquivo não existir (ou falhar ao
carregar), o SpiderDesk usa a aranha procedural feita de primitivas do Bevy.

Convenções do modelo:

| Item | Convenção |
|---|---|
| Frente | +X |
| Cima | +Y |
| Pés | tocam `y = 0` |
| Tamanho | ~3 unidades de envergadura (1 unidade ≈ 28 px × `spider.scale`) |
| Animações | nomes contendo `idle`, `walk`, `run`, `climb`, `hit`, `death` (qualquer prefixo/caixa, ex. `Armature\|Walk`) |

Animações ausentes usam fallback: `run → walk → idle`, `climb → walk → run → idle`,
`hit → idle`, `death → hit → idle`.

Ajustes finos sem recompilar, em `config/default.toml`, seção `[spider]`:
`model`, `scale`, `glb_yaw_deg` (gira o modelo), `glb_scale`.

Para testar o carregador sem um modelo real: `python3 tools/make_test_glb.py /tmp/teste/assets/models/spider.glb`
e depois `BEVY_ASSET_ROOT=/tmp/teste cargo run`.

## Modelos reais testados

Testados no Linux/Vulkan (render + carga + animacao; ver README principal). Cada um tem um preset
em `config/models/` com o `glb_yaw_deg` certo (o auto-fit cuida de escala e posicao):

| Modelo | Preset | yaw | Animacoes no arquivo → usadas |
|---|---|---|---|
| `wolf_spider_rigged_-_rabidosa_rabida.glb` (4,5 MB) | `wolf_spider.toml` | -90 | walking→Walk, running→Run (ignora "walk and turn", "backword"); sem idle/climb/hit/death |
| `walking_ant.glb` (4,6 MB) | `ant.toml` | 0 | `Take 001`→Walk |
| `american_cockroach_walk_cycle.glb` (20 MB) | `cockroach.toml` | 90 | `Modelo Cucaracha`→Walk |
| `grasshopper.glb` (2,1 MB) | `grasshopper.toml` | 180 | `Scene`→Walk |
| `Rat.glb` (0,15 MB) | `rat.toml` | 90 | idle, walk, dead→Death (ignora attack) |

Sem clipe proprio para repouso/golpe/morte, o clipe de andar **congela** (nao anda no lugar) e a
morte usa o giro de 180 graus + encolhimento aplicados na raiz.

Como descobrir o `glb_yaw_deg` de um modelo novo: rode com `--screenshot teste.png` (ou olhe na
tela) e gire de 90 em 90 ate a cabeca apontar na direcao em que ela anda (+X = para a direita).
