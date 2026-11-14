#!/usr/bin/env bash
# Instala a extensao "SpiderDesk windows" do GNOME Shell (so para o usuario atual).
# Necessaria em sessoes Wayland para os mobs andarem nas janelas reais dos aplicativos.
# Remover: gnome-extensions uninstall spiderdesk-windows@neurelix
set -euo pipefail

uuid="spiderdesk-windows@neurelix"
src="$(cd "$(dirname "$0")" && pwd)/gnome-extension/$uuid"
dst="${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/$uuid"

mkdir -p "$dst"
cp "$src/metadata.json" "$src/extension.js" "$dst/"
echo "Extensao copiada para $dst"

if gnome-extensions enable "$uuid" 2>/dev/null; then
    echo "Extensao ativada. Abra o SpiderDesk: os mobs passam a usar as janelas reais."
else
    # o Shell em execucao ainda nao conhece a extensao: deixa marcada para o proximo login
    cur="$(gsettings get org.gnome.shell enabled-extensions)"
    if [[ "$cur" != *"$uuid"* ]]; then
        if [[ "$cur" == *"'"* ]]; then
            new="${cur%]*}, '$uuid']"
        else
            new="['$uuid']"
        fi
        gsettings set org.gnome.shell enabled-extensions "$new"
    fi
    echo "Pronto. O GNOME so carrega extensoes novas ao iniciar a sessao:"
    echo "saia da sessao e entre de novo; a extensao ja estara ativa."
fi
