# OverSight 2D SDK

Готовый движок для игр на C, C++ и C#: собирать Rust не нужно.

- `include/` — заголовки: `oversight.h` для C, `oversight.hpp` для C++.
- `bin/oversight.dll`, `lib/oversight.dll.lib` — библиотека для Windows; `lib/liboversight.so` — для Linux.
- `cmake/` — пакет для CMake.
- `nuget/OverSight.Net.<версия>.nupkg` — пакет для C#, библиотека движка уже внутри.
- `new-game` и `templates/` — заготовки новой игры.

## Новая игра за минуту

В папке, где будет игра (PowerShell или cmd):

```
C:\oversight-sdk\new-game MyGame          # игра на C#
C:\oversight-sdk\new-game MyGame cpp      # игра на C++
```

На Linux: `~/oversight-sdk/new-game.sh MyGame [cpp]`.

Получится папка `MyGame` с маленькой игрой «собери монеты», уже настроенной на этот SDK. Запуск C#: `cd MyGame`, затем `dotnet run`. Запуск C++:

```
cd MyGame
cmake -S . -B build
cmake --build build --config Release
.\build\Release\MyGame.exe
```

Картинки, звуки и карты кладите в `MyGame/assets`: при сборке они копируются рядом с игрой. SDK потом лучше не перемещать: игра помнит путь к нему (в `CMakeLists.txt` или `nuget.config`).

## Игра на C#

Нужен .NET 8 SDK. Где `C:/oversight-sdk` — эта папка:

```
dotnet new console -n MyGame
cd MyGame
dotnet add package OverSight.Net --source C:/oversight-sdk/nuget
dotnet run
```

`dotnet add package` один раз запоминает пакет в `MyGame.csproj`. Чтобы следующие `dotnet restore` тоже находили папку, положите рядом с проектом `nuget.config`:

```xml
<configuration>
  <packageSources>
    <add key="oversight" value="C:/oversight-sdk/nuget" />
  </packageSources>
</configuration>
```

## Игра на C++ с CMake

```cmake
cmake_minimum_required(VERSION 3.16)
project(my_game CXX)

find_package(oversight REQUIRED)
add_executable(my_game main.cpp)
target_link_libraries(my_game PRIVATE oversight::oversight)
if(MSVC)
    target_compile_options(my_game PRIVATE /utf-8)   # исходники в UTF-8
endif()
oversight_copy_runtime(my_game)                     # кладёт библиотеку рядом с exe
```

Сборка, где `C:/oversight-sdk` — эта папка:

```
cmake -S . -B build -DCMAKE_PREFIX_PATH=C:/oversight-sdk
cmake --build build --config Release
```

## Без CMake

Добавьте `include` в пути заголовков, `lib/oversight.dll.lib` в компоновщик и положите `bin/oversight.dll` рядом с exe.

Документация и примеры: <https://github.com/Qllan/OverSight_GameEngine/tree/oversight-2d/crates/oversight>.
