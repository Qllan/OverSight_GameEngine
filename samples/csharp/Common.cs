// Shared by the samples: command line, scripted input for headless runs.
using OverSight;

static class Sample
{
    public sealed class Options
    {
        public string Assets = Path.Combine(AppContext.BaseDirectory, "assets");
        /// <summary>--headless N: no window, run N frames with scripted input</summary>
        public int Frames;
        /// <summary>--screenshot file.png: save the last frame</summary>
        public string? Screenshot;
    }

    public static Options Parse(string[] args)
    {
        var o = new Options();
        for (int i = 0; i + 1 < args.Length; i++)
        {
            switch (args[i])
            {
                case "--assets": o.Assets = args[++i]; break;
                case "--headless": o.Frames = int.Parse(args[++i]); break;
                case "--screenshot": o.Screenshot = args[++i]; break;
            }
        }
        return o;
    }

    public static Config Window(string title, Options o) => new(title, 320, 180, 4f)
    {
        IntegerScale = true,
        Headless = o.Frames > 0,
    };

    /// <summary>Key presses at given frames, for headless runs</summary>
    public sealed class Script
    {
        private readonly (int Frame, Key Key, bool Down)[] steps;
        private int frame;

        public Script(params (int, Key, bool)[] steps) { this.steps = steps; }

        /// <summary>One frame of the loop: true while the game should go on</summary>
        public bool Next(Engine engine, Options o)
        {
            if (o.Frames > 0)
            {
                if (frame >= o.Frames) return false;
                foreach (var s in steps)
                    if (s.Frame == frame) engine.InjectKey(s.Key, s.Down);
            }
            frame++;
            return engine.Poll();
        }
    }

    public static int Finish(Engine engine, Options o, string name)
    {
        if (o.Screenshot != null) engine.SaveScreenshot(o.Screenshot);
        if (o.Frames > 0) Console.WriteLine($"{name}: {o.Frames} frames ok");
        return 0;
    }

    /// <summary>Camera centred on target but kept inside the map</summary>
    public static Vec2 Follow(Vec2 target, float mapW, float mapH, float viewW = 320, float viewH = 180)
    {
        float x = mapW <= viewW ? mapW / 2 : Math.Clamp(target.X, viewW / 2, mapW - viewW / 2);
        float y = mapH <= viewH ? mapH / 2 : Math.Clamp(target.Y, viewH / 2, mapH - viewH / 2);
        return new Vec2(MathF.Floor(x), MathF.Floor(y));
    }
}
