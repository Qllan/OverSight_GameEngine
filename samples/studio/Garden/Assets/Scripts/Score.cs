using ORST;

/// Counts the coins; the game is won when all are picked up
public class Score : Behaviour
{
    public int Coins;
    public int ToWin = 8;

    public void Add(int coins)
    {
        Coins += coins;
        Log($"Coins: {Coins}/{ToWin}");
        if (Coins == ToWin) Log("All coins collected. You win!");
    }
}
