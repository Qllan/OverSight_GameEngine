using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Runtime.Loader;
using System.Text;

namespace ORST.Scripting;

/// <summary>
/// What ORST Studio calls, through function pointers it gets from .NET.
/// The scripts' DllImport of "orst" resolves to the studio itself, which
/// exports the C API, so they work on the engine the editor shows.
/// Every entry point catches what scripts throw and reports it: an
/// exception must not cross into the studio.
/// </summary>
public static unsafe class Host
{
    private static delegate* unmanaged<uint, byte*, nuint, void> log;
    private static IntPtr handle;
    private static bool resolverSet;
    private static ScriptContext? context;
    private static readonly List<Script> scripts = new();
    private static bool playing;
    private static bool firstTick;
    private static IntPtr error;

    /// <summary>The engine scripts work on; a new wrapper per load, so
    /// none keeps a type of an unloaded script alive</summary>
    internal static Engine? Engine { get; private set; }

    /// <summary>The studio's engine and its Console; again for each
    /// project opened, dropping the scripts of the one before</summary>
    [UnmanagedCallersOnly]
    public static int Attach(IntPtr engine, IntPtr logFn)
    {
        try
        {
            UnloadScripts();
            handle = engine;
            log = (delegate* unmanaged<uint, byte*, nuint, void>)logFn;
            if (!resolverSet)
            {
                NativeLibrary.SetDllImportResolver(typeof(Engine).Assembly, (name, _, _) =>
                    name == "orst" ? NativeLibrary.GetMainProgramHandle() : IntPtr.Zero);
                resolverSet = true;
            }
            Console.SetOut(new ConsoleWriter(0));
            Console.SetError(new ConsoleWriter(2));
            Engine = new Engine(handle);
            return 0;
        }
        catch (Exception x) { return Fail(x); }
    }

    /// <summary>Load the compiled scripts (unloading the ones before) and
    /// define their components; the number of Behaviours, -1 on error</summary>
    [UnmanagedCallersOnly]
    public static int Load(byte* path)
    {
        try
        {
            if (!UnloadScripts()) Log(1, "The previous scripts are still held in memory");
            var file = Marshal.PtrToStringUTF8((IntPtr)path)!;
            context = new ScriptContext(Path.GetDirectoryName(Path.GetFullPath(file))!);
            var assembly = context.LoadFile(file);
            Engine = new Engine(handle);
            var types = assembly.GetTypes()
                .Where(t => t.IsSubclassOf(typeof(Behaviour)) && !t.IsAbstract && !t.IsGenericTypeDefinition)
                .OrderBy(t => t.FullName, StringComparer.Ordinal);
            foreach (var t in types) scripts.Add(Script.Define(Engine, t));
            return scripts.Count;
        }
        catch (Exception x) { return Fail(x); }
    }

    /// <summary>Drop the scripts; 1 when .NET still holds their assembly</summary>
    [UnmanagedCallersOnly]
    public static int Unload()
    {
        try { return UnloadScripts() ? 0 : 1; }
        catch (Exception x) { return Fail(x); }
    }

    /// <summary>Play: instances are made as entities are met</summary>
    [UnmanagedCallersOnly]
    public static int Start()
    {
        playing = true;
        firstTick = true;
        foreach (var s in scripts) s.Live.Clear();
        return 0;
    }

    /// <summary>One tick: Start for entities new to a script, then Update
    /// for all; the number of exceptions scripts threw</summary>
    [UnmanagedCallersOnly]
    public static int Tick(float dt)
    {
        if (!playing || Engine is not { } engine) return 0;
        int thrown = 0;
        try
        {
            var met = new List<(Script, Entity[])>();
            foreach (var s in scripts)
            {
                var entities = engine.Query(s.Id);
                var ids = new HashSet<ulong>(entities.Select(e => e.Id));
                foreach (var gone in s.Live.Keys.Where(k => !ids.Contains(k)).ToArray()) s.Live.Remove(gone);
                met.Add((s, entities));
            }
            foreach (var (s, entities) in met)
                foreach (var e in entities)
                {
                    if (s.Live.ContainsKey(e.Id)) continue;
                    var b = (Behaviour)Activator.CreateInstance(s.Type, nonPublic: true)!;
                    b.Entity = e;
                    s.Live[e.Id] = b;
                    if (!Run(s, b, x => x.Start())) thrown++;
                }
            // Contacts are the engine's from the tick before; none yet on the first
            if (!firstTick)
                foreach (var c in engine.Contacts())
                {
                    if (c.State == ContactState.Stay) continue;
                    foreach (var me in new[] { c.A, c.B })
                        foreach (var (s, _) in met)
                        {
                            if (!s.Live.TryGetValue(me.Id, out var b)) continue;
                            var other = c.Other(me);
                            var ok = c.State == ContactState.Begin
                                ? Run(s, b, x => x.OnContactBegin(other, c))
                                : Run(s, b, x => x.OnContactEnd(other, c));
                            if (!ok) thrown++;
                        }
                }
            firstTick = false;
            foreach (var (s, entities) in met)
                foreach (var e in entities)
                    if (s.Live.TryGetValue(e.Id, out var b) && !Run(s, b, x => x.Update(dt))) thrown++;
        }
        catch (Exception x)
        {
            Fail(x);
            thrown++;
        }
        return thrown;
    }

    /// <summary>Stop: the instances go</summary>
    [UnmanagedCallersOnly]
    public static void Stop()
    {
        playing = false;
        foreach (var s in scripts) s.Live.Clear();
    }

    /// <summary>The last error, UTF-8, valid until the next one</summary>
    [UnmanagedCallersOnly]
    public static IntPtr LastError() => error;

    /// <summary>Fields in, call, fields out; false if the script threw</summary>
    private static bool Run(Script s, Behaviour b, Action<Behaviour> call)
    {
        var p = (byte*)Native.orst_get(handle, b.Entity.Id, s.Id);
        if (p == null) return true;
        s.Read(b, p);
        try
        {
            call(b);
        }
        catch (Exception x)
        {
            Log(2, $"{s.Name}: {x}");
            return false;
        }
        // The call may have added components, which moves the rows
        p = (byte*)Native.orst_get(handle, b.Entity.Id, s.Id);
        if (p != null) s.Write(b, p);
        return true;
    }

    private static bool UnloadScripts()
    {
        if (DropScripts() is not { } weak) return true;
        for (int i = 0; i < 10 && weak.IsAlive; i++)
        {
            GC.Collect();
            GC.WaitForPendingFinalizers();
        }
        return !weak.IsAlive;
    }

    /// <summary>A method of its own, so no stack slot of the caller still
    /// points at the context while it waits for the collector</summary>
    [MethodImpl(MethodImplOptions.NoInlining)]
    private static WeakReference? DropScripts()
    {
        playing = false;
        scripts.Clear();
        Engine = new Engine(handle);
        if (context is null) return null;
        var weak = new WeakReference(context);
        context.Unload();
        context = null;
        return weak;
    }

    private static int Fail(Exception x)
    {
        if (x is ReflectionTypeLoadException r && r.LoaderExceptions.FirstOrDefault() is { } first) x = first;
        if (x is TargetInvocationException { InnerException: { } inner }) x = inner;
        Marshal.FreeCoTaskMem(error);
        error = Marshal.StringToCoTaskMemUTF8(x.Message);
        return -1;
    }

    internal static void Log(uint level, string text)
    {
        if (log == null) return;
        var bytes = Encoding.UTF8.GetBytes(text);
        fixed (byte* p = bytes) log(level, p, (nuint)bytes.Length);
    }

    /// <summary>Console.WriteLine from scripts, a line at a time</summary>
    private sealed class ConsoleWriter(uint level) : TextWriter
    {
        private readonly StringBuilder line = new();
        public override Encoding Encoding => Encoding.UTF8;

        public override void Write(char c)
        {
            if (c == '\n')
            {
                Log(level, line.ToString().TrimEnd('\r'));
                line.Clear();
            }
            else line.Append(c);
        }
    }

    /// <summary>Scripts in a context of their own, so a new build can
    /// replace them. Files are read into memory, so dotnet build can
    /// overwrite them while they are loaded.</summary>
    private sealed class ScriptContext(string dir) : AssemblyLoadContext("ORST scripts", isCollectible: true)
    {
        public Assembly LoadFile(string path)
        {
            var pdb = Path.ChangeExtension(path, ".pdb");
            using var code = new MemoryStream(File.ReadAllBytes(path));
            using var symbols = File.Exists(pdb) ? new MemoryStream(File.ReadAllBytes(pdb)) : null;
            return LoadFromStream(code, symbols);
        }

        protected override Assembly? Load(AssemblyName name)
        {
            // The engine and Behaviour are the studio's, shared by every load
            if (name.Name == typeof(Engine).Assembly.GetName().Name) return typeof(Engine).Assembly;
            if (name.Name == typeof(Behaviour).Assembly.GetName().Name) return typeof(Behaviour).Assembly;
            var file = Path.Combine(dir, name.Name + ".dll");
            return File.Exists(file) ? LoadFile(file) : null;
        }
    }
}
