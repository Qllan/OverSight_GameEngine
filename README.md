# ORST Game Engine

2D-движок для игр на C++ и C# с редактором ORST Studio. Ядро написано на Rust; игра пишется как обычная программа со своим циклом или как скрипты в редакторе, как в Unity. Вид сверху, платформер и изометрия.

![CI](https://github.com/Qllan/OverSight_GameEngine/actions/workflows/orst.yml/badge.svg)

## Что внутри

| Папка | Что это |
|---|---|
| [crates/orst](crates/orst/README.md) | Движок: рендер (GPU и программный), ввод, звук, ECS, сцены, тайлмапы Tiled, физика, свет, частицы. C API `orst.h` |
| [crates/orst-studio](crates/orst-studio/README.md) | Редактор: сцены, инспектор, Play в окне, скрипты на C# и C++, редактор тайлмапов, сборка игры в папку |
| [bindings](bindings) | Привязки для C++ (`orst.hpp`) и C# (`ORST.Net`) |
| [samples](samples) | Примеры: вид сверху, платформер, изометрия на C++ и C#, проект Coins для редактора |
| [crates/orst/docs/first-game.md](crates/orst/docs/first-game.md) | Первая игра шаг за шагом |

## Быстрый старт

Нужны Rust (stable), для C#-скриптов .NET 8 SDK, для C++-скриптов CMake и компилятор C++ (на Windows Visual Studio 2022 с C++).

```
cargo build --release -p orst
dotnet build -c Release bindings/csharp/Scripting
cargo run --release -p orst-studio -- samples/studio/Coins
```

Готовый редактор для Windows (`orst-studio-windows-x64`) и SDK движка лежат в артефактах CI: Actions → ORST → последний зелёный запуск.

## Тесты

```
cargo test -p orst
cargo test -p orst-studio
dotnet run --project bindings/csharp/Smoke -c Release
```

## Visual Novel Engine

В репозитории также лежит более ранний проект: движок визуальных новелл на Rust и Python с визуальной логикой из блоков (`src/`, `python/`, `examples/`, `tests/`). Его описание: [docs/VN_ENGINE.md](docs/VN_ENGINE.md).

## Лицензия

ORST Game Engine (`crates/`, `bindings/`, `samples/`) распространяется под двумя лицензиями на выбор: [MIT](LICENSE-MIT) или [Apache-2.0](LICENSE-APACHE). Игры на движке можно продавать и не открывать их код. Visual Novel Engine распространяется под [MIT](LICENSE).
