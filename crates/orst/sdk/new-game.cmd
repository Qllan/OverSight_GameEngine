@echo off
rem Makes a new game from a template of this SDK: new-game MyGame [cs|cpp]
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0new-game.ps1" %*
