using ORST;

/// The title screen: play, continue the saved game, settings (language
/// and keys), quit. Changed keys are kept for the next runs.
public class MainMenu : Behaviour
{
    static readonly string[] Keys = { "up", "down", "left", "right" };

    /// The action waiting for a key, 0: none
    private uint waiting;

    public override void Start()
    {
        var language = Engine.GetPref("language");
        if (language != "") Engine.Language = language;
        ShowSettings(false);
        Refresh();
    }

    void ShowSettings(bool shown)
    {
        Ui.Show(Engine, "Settings", shown);
        Ui.Show(Engine, "Buttons", !shown);
        Ui.Show(Engine, "Title", !shown);
    }

    /// What depends on the saves, the language and the keys
    void Refresh()
    {
        Ui.Enable(Engine, "Continue", Engine.SaveExists(Ui.Slot));
        foreach (var name in Keys)
        {
            var bindings = Engine.Bindings(Engine.Action(name));
            var key = bindings.Length == 0 ? "-" : Engine.BindingText(bindings[0]).Split(':')[1];
            Ui.SetText(Engine, "Key " + name, $"{Engine.Tr("keys." + name)}: {key}");
        }
    }

    public override void Update(float dt)
    {
        if (waiting != 0)
        {
            Rebind();
            return;
        }
        if (Ui.Clicked(Engine, "Play")) Engine.LoadScene("Main.scene");
        if (Ui.Clicked(Engine, "Continue")) Engine.LoadGame(Ui.Slot);
        if (Ui.Clicked(Engine, "Settings button")) ShowSettings(true);
        if (Ui.Clicked(Engine, "Quit")) Engine.Quit();
        if (Ui.Clicked(Engine, "Back")) ShowSettings(false);
        if (Ui.Clicked(Engine, "Language"))
        {
            var all = Engine.Languages;
            var next = all[(Array.IndexOf(all, Engine.Language) + 1) % all.Length];
            Engine.Language = next;
            Engine.SetPref("language", next);
            Refresh();
        }
        if (Ui.Clicked(Engine, "Reset keys"))
        {
            Engine.ResetInput();
            Refresh();
        }
        foreach (var name in Keys)
        {
            if (!Ui.Clicked(Engine, "Key " + name)) continue;
            waiting = Engine.Action(name);
            Ui.SetText(Engine, "Key " + name, Engine.Tr("keys.press"));
        }
    }

    /// The next key or gamepad input becomes the action's first binding;
    /// Escape leaves it as it was
    void Rebind()
    {
        if (Engine.CapturedBinding is not { } input || input.Kind == BindingKind.Mouse) return;
        if (input.Kind != BindingKind.Key || input.Code != (int)Key.Escape)
        {
            if (!Engine.SetBinding(waiting, 0, input)) Engine.Bind(waiting, input);
            Engine.KeepInput();
        }
        waiting = 0;
        Refresh();
    }
}
