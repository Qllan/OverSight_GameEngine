# Первая игра на OverSight 2D

В этом уроке получится маленькая игра: герой ходит по саду, упирается в стены и читает табличку. Урок написан на C#, а в конце есть таблица тех же функций для C++.

## 1. Что нужно

- Архив SDK `oversight-sdk-<версия>-windows-x64.zip` со страницы [Releases](https://github.com/Qllan/OverSight_GameEngine/releases). Распакуйте его, например, в `C:\oversight-sdk`. Rust и исходники движка не нужны.
- [.NET 8 SDK](https://dotnet.microsoft.com/download/dotnet/8.0).
- [Tiled](https://www.mapeditor.org) — редактор карт, если хотите их править.

## 2. Новая игра

В папке, где будут ваши проекты:

```
C:\oversight-sdk\new-game MyGame
cd MyGame
dotnet run
```

Откроется окно с игрой «собери монеты». Это заготовка, и дальше мы заменим её код в `Program.cs` своим.

## 3. Как устроена игра

Игра — это цикл, который повторяется каждый кадр, 60 раз в секунду:

```csharp
while (engine.Poll())          // false, когда окно закрыли
{
    float dt = engine.Dt;      // сколько секунд прошло с прошлого кадра
    // 1. читаем ввод и двигаем объекты
    engine.BeginFrame(Color.Black);
    // 2. рисуем
    engine.EndFrame();         // показываем кадр
}
```

Скорости задавайте в пикселях в секунду и умножайте на `dt`, тогда игра идёт одинаково на любом компьютере.

Игра рисует в маленький холст 320×180 (`Width`, `Height`), а окно растягивает его в `Scale` раз. Для пиксель-арта это удобно: координаты маленькие, картинка чёткая.

## 4. Карта

Скопируйте из `C:\oversight-sdk\examples\assets` в папку `MyGame\assets` три файла: `topdown.tmj` (карта из Tiled), `tiles.png` (тайлы) и `hero.png` (герой). При сборке всё из `assets` копируется рядом с игрой, поэтому путь к файлам строим от папки программы:

```csharp
static string Asset(string name) => Path.Combine(AppContext.BaseDirectory, "assets", name);

using var map = engine.LoadTilemap(Asset("topdown.tmj"));
```

В цикле, между `BeginFrame` и `EndFrame`, карта рисуется одной строкой: `map.Draw();`.

Камера решает, какая точка мира будет в центре экрана. `engine.LookAt(feet)` держит героя в центре.

## 5. Герой и стены

Тайлы со свойством `solid` в Tiled становятся стенами. `MoveBox` двигает прямоугольник по карте: в стены он не заходит, а скользит вдоль них.

```csharp
var box = new Rect(feet.X - 5, feet.Y - 6, 10, 6);   // «ноги» героя
box = map.MoveBox(box, dir * (70 * dt)).Rect;
```

Со стенами сталкивается только маленький прямоугольник у ног, а спрайт рисуется выше. Поэтому голова героя может зайти за верхушку куста, а сам он в стену не пройдёт.

Направление `dir` собираем из клавиш и стика геймпада. `engine.Axis(Key.A, Key.D)` даёт -1, 0 или 1, а `engine.PadAxis(0, PadAxis.LeftX)` — положение стика первого геймпада.

## 6. Спрайт и анимация

`hero.png` — лист кадров 16×16. Нарезаем его и задаём анимацию ходьбы из кадров 0–3:

```csharp
using var hero = engine.LoadTexture(Asset("hero.png"));
var frames = hero.Sheet(16, 16);
var walk = Animation.Range(0, 4, 8f);   // 4 кадра, 8 в секунду
```

`walk.FrameAt(walkTime)` выбирает кадр по времени, а `engine.DrawFrame(hero, frames, кадр, sprite)` рисует его. В `Sprite` задаются позиция, точка привязки (`Origin`), отражение (`FlipX`) и слой (`Layer`, чем больше, тем выше).

## 7. Табличка и текст на экране

Объекты, расставленные в Tiled (точка появления, табличка), находятся по имени: `map.FindObject("sign")`. Их свойства читаются через `map.String(obj, "text")`.

Текст для игрока рисуйте в экранных координатах, чтобы он не уезжал вместе с камерой:

```csharp
engine.Space = Space.Screen;   // дальше координаты экрана
engine.DrawText("Привет", new Vec2(4, 2), 10, Color.White);
engine.Space = Space.World;    // обратно в мир
```

## 8. Весь код

```csharp
// MyGame: walk around the garden and read the sign.
using OverSight;

// Files from assets/ lie next to the game after the build
static string Asset(string name) => Path.Combine(AppContext.BaseDirectory, "assets", name);

using var engine = new Engine(new Config
{
    Title = "MyGame",
    Width = 320, Height = 180,
    Scale = 4,
    IntegerScale = true,
});
engine.HotReload = true; // save the map in Tiled and the game picks it up

using var map = engine.LoadTilemap(Asset("topdown.tmj"));
using var hero = engine.LoadTexture(Asset("hero.png"));
var frames = hero.Sheet(16, 16);       // 16x16 frames, 4 per row
var walk = Animation.Range(0, 4, 8f);  // frames 0..3, 8 per second

var feet = new Vec2(64, 168);
if (map.FindObject("player") is { } spawn) feet = new Vec2(spawn.Rect.X, spawn.Rect.Y);
var sign = map.FindObject("sign");
float walkTime = 0;
bool left = false;

while (engine.Poll())
{
    float dt = engine.Dt;
    var dir = new Vec2(
        engine.Axis(Key.A, Key.D) + engine.PadAxis(0, PadAxis.LeftX),
        engine.Axis(Key.W, Key.S) + engine.PadAxis(0, PadAxis.LeftY));
    float len = MathF.Sqrt(dir.X * dir.X + dir.Y * dir.Y);
    if (len > 0)
    {
        // The feet are a 10x6 box: it slides along walls instead of going through
        var box = new Rect(feet.X - 5, feet.Y - 6, 10, 6);
        box = map.MoveBox(box, dir * (70 * dt / MathF.Max(len, 1))).Rect;
        feet = new Vec2(box.X + 5, box.Bottom);
        walkTime += dt;
        if (dir.X != 0) left = dir.X < 0;
    }
    else walkTime = 0;

    engine.LookAt(feet);
    engine.BeginFrame(new Color(20, 24, 30));
    map.Draw();
    var s = new Sprite { Pos = feet, Origin = new Vec2(8, 15), FlipX = left, Layer = 5 };
    engine.DrawFrame(hero, frames, len > 0 ? walk.FrameAt(walkTime) : 0, s);

    // Text on the screen, not in the world: it does not move with the camera
    engine.Space = Space.Screen;
    engine.Layer = 10;
    if (sign != null && sign.Rect.Overlaps(new Rect(feet.X - 6, feet.Y - 12, 12, 12)))
    {
        engine.FillRect(new Rect(20, 140, 280, 30), new Color(0, 0, 0, 200));
        engine.DrawText(map.String(sign, "text") ?? "", new Vec2(28, 148), 12, Color.White);
    }
    engine.Space = Space.World;
    engine.Layer = 0;
    engine.EndFrame();
}
```

## 9. Звук

Положите любой файл `.wav`, `.ogg` или `.mp3` в `assets`:

```csharp
using var step = engine.LoadSound(Asset("step.wav"));
step.Play();                                   // эффект
engine.PlayMusic(Asset("music.ogg"), 0.5f);    // музыка по кругу
```

## 10. Отладка

- **F3** показывает FPS, время кадра, камеру и рендер. Пока панель открыта, `engine.DebugTilemap(map)` обводит стены и объекты карты.
- `engine.HotReload = true` уже стоит в коде выше. Поправьте карту в Tiled или картинку в редакторе, сохраните, и запущенная игра подхватит изменения.

## 11. Отдать игру

```
dotnet publish -c Release -r win-x64 --self-contained
```

Папку `bin\Release\net8.0\win-x64\publish` можно заархивировать и отправить. Ставить .NET на компьютер друга не нужно.

## То же на C++

Игра на C++ создаётся командой `new-game MyGame cpp`. Функции те же, только названы в стиле C++:

| C# | C++ |
| --- | --- |
| `engine.Poll()`, `engine.Dt` | `engine.poll()`, `engine.dt()` |
| `engine.BeginFrame(c)`, `engine.EndFrame()` | `engine.begin_frame(c)`, `engine.end_frame()` |
| `engine.LoadTilemap(path)`, `map.Draw()` | `engine.load_tilemap(path)`, `map.draw()` |
| `map.MoveBox(box, delta).Rect` | `map.move_box(box, delta).rect` |
| `map.FindObject("sign")` | `map.find_object("sign")`, возвращает `std::optional` |
| `hero.Sheet(16, 16)`, `Animation.Range(0, 4, 8f)` | `hero.sheet(16, 16)`, `ov::Animation::range(0, 4, 8.0f)` |
| `engine.DrawFrame(t, sheet, f, s)` | `engine.draw_frame(t, sheet, f, s)` |
| `engine.Space = Space.Screen` | `engine.set_space(ov::Space::Screen)` |
| `engine.LookAt(p)` | `engine.look_at(p)` |
| `engine.HotReload = true` | `engine.set_hot_reload(true)` |

Папка `assets` копируется в папку сборки рядом с exe. Запускайте игру из папки проекта или стройте пути от папки exe.

Готовая игра на C++ — это папка `build\Release`: в ней exe, `oversight.dll` и `assets`.
