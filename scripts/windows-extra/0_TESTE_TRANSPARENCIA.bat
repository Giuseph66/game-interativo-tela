@echo off
cd /d "%~dp0"
echo Teste de transparencia: deve aparecer so um cubo girando sobre a area de trabalho.
echo Fecha sozinho em 20 segundos. Log em spiderdesk-log.txt.
spiderdesk.exe --poc --timeout 20
