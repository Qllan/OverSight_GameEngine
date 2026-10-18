using ORST;

/// Small helpers for the menus: boxes are found by their Name
public static class Ui
{
    /// The save the menus write and read
    public const string Slot = "garden";

    public static void Show(Engine engine, string name, bool shown)
    {
        var e = engine.FindNamed(name);
        if (!e.IsNone) engine.Get<UiRect>(e).Hidden = !shown;
    }

    public static bool Clicked(Engine engine, string name)
    {
        var e = engine.FindNamed(name);
        return !e.IsNone && engine.Has<UiButton>(e) && engine.Get<UiButton>(e).Clicked;
    }

    public static void SetText(Engine engine, string name, string text)
    {
        var e = engine.FindNamed(name);
        if (!e.IsNone) engine.Get<UiText>(e).Text = engine.Intern(text);
    }

    public static void Enable(Engine engine, string name, bool on)
    {
        var e = engine.FindNamed(name);
        if (!e.IsNone) engine.Get<UiButton>(e).Interactable = on;
    }
}
