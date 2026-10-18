using ORST;

/// Counts the coins on the screen; the game is won when all are picked up
public class Score : Behaviour
{
    public int Coins;
    public int ToWin = 8;

    public override void Start() => Show();

    public void Add(int coins)
    {
        Coins += coins;
        Log($"Coins: {Coins}/{ToWin}");
        Show();
    }

    void Show()
    {
        Ui.SetText(Engine, "Coins text", string.Format(Engine.Tr("hud.coins"), Coins, ToWin));
        Ui.Show(Engine, "Win", Coins >= ToWin);
    }
}
