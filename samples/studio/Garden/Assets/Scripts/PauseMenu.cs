using ORST;

/// Esc (or Start on a gamepad) pauses the game: resume, save, load the
/// save, or go back to the title screen
public class PauseMenu : Behaviour
{
    public static bool Paused;

    private float statusTime;

    public override void Start() => Pause(false);

    void Pause(bool on)
    {
        Paused = on;
        Ui.Show(Engine, "Pause", on);
        Ui.Enable(Engine, "Load", Engine.SaveExists(Ui.Slot));
        Ui.SetText(Engine, "Pause status", "");
    }

    public override void Update(float dt)
    {
        if (Engine.ActionPressed(Engine.Action("pause"))) Pause(!Paused);
        if (!Paused) return;
        if (statusTime > 0 && (statusTime -= dt) <= 0) Ui.SetText(Engine, "Pause status", "");
        if (Ui.Clicked(Engine, "Resume")) Pause(false);
        if (Ui.Clicked(Engine, "Save"))
        {
            // A loaded game starts unpaused: PauseMenu.Start hides the menu
            Engine.SaveGame(Ui.Slot);
            Ui.Enable(Engine, "Load", true);
            Ui.SetText(Engine, "Pause status", Engine.Tr("pause.saved"));
            statusTime = 2;
        }
        if (Ui.Clicked(Engine, "Load")) Engine.LoadGame(Ui.Slot);
        if (Ui.Clicked(Engine, "To menu")) Engine.LoadScene("Menu.scene");
    }
}
