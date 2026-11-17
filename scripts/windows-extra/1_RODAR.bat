@echo off
cd /d "%~dp0"
echo SpiderDesk rodando. Para sair: Ctrl+Alt+Shift+Q (ou botao "Sair" no painel).
echo O log vai para spiderdesk-log.txt nesta pasta.
spiderdesk.exe --spawn aranha,lobo,formiga,barata,gafanhoto,rato %*
