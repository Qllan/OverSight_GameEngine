# OverSight 2D

Игровой рантайм для 2D-игр на C++ и C#. Движок написан на Rust и собирается в `oversight.dll`. Игра — обычная программа со своим циклом, как в raylib или MonoGame:

```cpp
#include "oversight.hpp"

int main() {
    ov::Engine engine({"Моя игра", 320, 180, 4.0f});
    while (engine.poll()) {
        float dt = engine.dt();
        engine.begin_frame(ov::Color::Black);
        // логика и отрисовка
        engine.end_frame();
    }
}
```

```csharp
using OverSight;

using var engine = new Engine(new Config("Моя игра", 320, 180, 4f));
while (engine.Poll())
{
    float dt = engine.Dt;
    engine.BeginFrame(Color.Black);
    // логика и отрисовка
    engine.EndFrame();
}
```

Поддерживает вид сверху, вид сбоку (платформер) и изометрию.

## Что умеет

- **Рисование.** Спрайты: область текстуры, поворот, масштаб, отражение, оттенок, прозрачность. Прямоугольники, линии, круги, текст. Встроенный шрифт DejaVu Sans с кириллицей. Слои и глубина: если передать `y` как глубину, получится y-сортировка. Камера с зумом и поворотом. Невидимое не рисуется.
- **Окно.** Логическое разрешение растягивается на окно с чёрными полосами. Есть масштаб только в целое число раз для пиксель-арта и полноэкранный режим.
- **Ввод.** Клавиатура по физическим клавишам (WASD работает на любой раскладке), мышь в координатах экрана и мира, колесо, ввод текста.
- **Анимации.** Нарезка спрайт-листов, анимации по кругу, один раз или туда-обратно.
- **Тайлмапы.** Ортогональные и изометрические, загрузка из [Tiled](https://www.mapeditor.org) (`.tmj`). Слои тайлов, объекты (спавны, триггеры), свойства. Пересчёт координат тайл ↔ мир ↔ экран.
- **Коллизии.** `move_box` двигает прямоугольник по карте со скольжением вдоль стен и возвращает флаги касания: пол, потолок, стены. Тайлы со свойством `solid` непроходимы. Тайлы со свойством `platform` — односторонние платформы. Есть raycast, проверка точки и прямоугольника.
- **Звук.** Эффекты с громкостью и высотой тона, музыка с повтором. Форматы WAV, OGG, MP3.
- **Твины.** 22 кривые плавности (Quad, Cubic, Sine, Expo, Back, Elastic, Bounce с вариантами In, Out, InOut), задержка, повтор и туда-обратно. В C++ `ov::Tween<float>`, `ov::Tween<ov::Vec2>`, `ov::Tween<ov::Color>`, в C# `FloatTween`, `Vec2Tween`, `ColorTween`.
- **Частицы.** Эмиттер выпускает частицы потоком или вспышкой: время жизни, скорость, конус направлений, гравитация, сопротивление, размер и цвет от рождения к смерти, вращение. Частицы — круглые точки или текстура. С одним и тем же `seed` получаются одинаковые частицы. В примере платформера — пыль из-под ног.
- **Отладочный оверлей.** F3 включает панель с FPS, временем кадра, числом отрисовок, камерой и мышью. `debug_rect`, `debug_line`, `debug_circle`, `debug_text` рисуют поверх всего только при включённом оверлее, `debug_tilemap` обводит стены, платформы и объекты карты. Клавишу можно сменить или отключить: `set_debug_key`.

## Сборка под Windows

Нужно поставить:

1. **Rust:** <https://rustup.rs>, установщик `rustup-init.exe`, вариант по умолчанию (MSVC).
2. **Visual Studio 2022** (или Build Tools) с нагрузкой «Разработка классических приложений на C++». В ней есть компилятор и CMake.
3. **.NET 8 SDK** для игр на C#: <https://dotnet.microsoft.com/download/dotnet/8.0>.

Сборка движка, из корня репозитория:

```
cargo build --release -p oversight
```

Получатся `target\release\oversight.dll` и библиотека импорта `target\release\oversight.dll.lib`.

Готовую DLL можно не собирать, а скачать: на GitHub во вкладке Actions откройте последний успешный запуск «OverSight 2D». Там есть артефакт `oversight-windows`: DLL, `.lib`, заголовки и привязка C#.

### Игра на C++

Подключите `bindings/cpp` в свой `CMakeLists.txt`:

```cmake
cmake_minimum_required(VERSION 3.16)
project(my_game CXX)

add_subdirectory(path/to/OverSight_GameEngine/bindings/cpp oversight)
add_executable(my_game main.cpp)
target_link_libraries(my_game PRIVATE oversight)
target_compile_options(my_game PRIVATE /utf-8)   # исходники в UTF-8
oversight_copy_runtime(my_game)                   # кладёт oversight.dll рядом с exe
```

Собрать (из «Developer PowerShell for VS 2022» или любого терминала с CMake):

```
cmake -S . -B build
cmake --build build --config Release
```

Если DLL лежит не в `target\release`, укажите папку: `cmake -S . -B build -DOVERSIGHT_LIB_DIR=C:/path/to/dll`.

Без CMake: добавьте в проект Visual Studio папку `crates/oversight/include` в пути заголовков, `oversight.dll.lib` в компоновщик, а `oversight.dll` положите рядом с exe. Из C подключайте `oversight.h`, из C++ — `oversight.hpp`.

### Игра на C#

```
dotnet new console -n MyGame
cd MyGame
dotnet add reference path/to/OverSight_GameEngine/bindings/csharp/OverSight/OverSight.csproj
```

Привязка сама копирует `oversight.dll` из `target\release` в папку сборки. Если DLL лежит в другом месте, задайте свойство `OverSightLibDir` в своём `.csproj`.

## Примеры

В `samples/` три игры, каждая на C++ и на C#. Карты лежат в `samples/assets`, их можно открыть в Tiled.

| Пример | Управление | Что показывает |
|---|---|---|
| `topdown` | WASD или стрелки | ходьба, стены, y-сортировка, табличка с текстом из свойства объекта |
| `platformer` | A/D, Space | гравитация, прыжок, односторонние платформы, камера, финишный флаг |
| `isometric` | WASD (шаг по клеткам), мышь | изометрическая карта, сортировка по глубине, клетка под курсором |

C++:

```
cargo build --release -p oversight
cmake -S samples/cpp -B build/samples
cmake --build build/samples --config Release
build\samples\Release\topdown.exe
```

C#:

```
cargo build --release -p oversight
dotnet run --project samples/csharp/TopDown -c Release
```

С ключами `--headless 200 --screenshot shot.png` пример проходит заскриптованный забег без окна и сохраняет последний кадр. Так их запускает CI.

## Карты из Tiled

- Сохраняйте карты в формате JSON (`.tmj`). Тайлсеты можно встраивать в карту или держать отдельными файлами `.tsj`.
- Слои тайлов — в CSV или base64 без сжатия. Режим «Бесконечная карта» нужно выключить.
- У тайлсета должна быть одна картинка (коллекции картинок пока не поддерживаются).
- Свойства тайлов: `solid` (bool) — стена, `platform` (bool) — односторонняя платформа. Эти свойства можно повесить и на весь слой.
- Объекты (точки, прямоугольники) читаются с именем, классом и свойствами: `find_object("player")`, `string(obj, "text")`, `number(obj, "hp")`.

## Тесты

```
cargo test -p oversight                    # тесты движка
cmake -S bindings/cpp -B build/bindings && cmake --build build/bindings --config Release
ctest --test-dir build/bindings -C Release # C и C++
dotnet run --project bindings/csharp/Smoke # C#
```

CI (`.github/workflows/oversight.yml`) прогоняет всё это на Windows, а также запускает шесть примеров без окна и проверяет, что C++ и C# рисуют одинаковые кадры.
