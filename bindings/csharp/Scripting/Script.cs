using System.Reflection;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace ORST.Scripting;

/// <summary>One Behaviour class: the component its fields make up, and
/// the live instances while playing</summary>
internal sealed unsafe class Script
{
    private readonly record struct Slot(FieldInfo Field, FieldType Type, uint Offset);

    public readonly Type Type;
    public readonly uint Id;
    private readonly Slot[] slots;
    /// <summary>Entity id -> its instance, while playing</summary>
    public readonly Dictionary<ulong, Behaviour> Live = new();

    private Script(Type type, uint id, Slot[] slots)
    {
        Type = type;
        Id = id;
        this.slots = slots;
    }

    public string Name => Type.Name;

    /// <summary>Lay out the class's fields as a component and give it to
    /// the engine; values already on entities are kept by field name</summary>
    public static Script Define(Engine engine, Type type)
    {
        var slots = new List<Slot>();
        var described = new List<ComponentField>();
        uint size = 0, align = 1;
        foreach (var fi in type.GetFields(BindingFlags.Public | BindingFlags.Instance))
        {
            if (fi.GetCustomAttribute<HiddenAttribute>() != null || Engine.Describe(fi) is not { } f) continue;
            var (bytes, a) = Measure(f.Type);
            size = (size + a - 1) / a * a;
            slots.Add(new(fi, f.Type, size));
            described.Add(f with { Offset = size });
            size += bytes;
            align = Math.Max(align, a);
        }
        size = (size + align - 1) / align * align;

        var defaults = new byte[size];
        var instance = (Behaviour)Activator.CreateInstance(type, nonPublic: true)!;
        var script = new Script(type, 0, slots.ToArray());
        fixed (byte* p = defaults) script.Write(instance, p);

        var strings = new List<IntPtr>();
        try
        {
            IntPtr Utf8(string s)
            {
                var ptr = Marshal.StringToCoTaskMemUTF8(s);
                strings.Add(ptr);
                return ptr;
            }
            var native = described.Select(f => new NativeField
            {
                Name = Utf8(f.Name), Type = (uint)f.Type, Offset = f.Offset, Flags = (uint)f.Flags,
                Min = f.Min, Max = f.Max,
                Options = f.Options.Length > 0 ? Utf8(string.Join('|', f.Options)) : IntPtr.Zero,
            }).ToArray();
            uint id;
            fixed (byte* d = defaults)
                id = Native.orst_define_component(engine.Handle, type.Name, size, align, native,
                                                  (uint)native.Length, (IntPtr)d);
            if (id == 0) throw new OrstException(type.Name + ": " + Engine.LastError);
            return new Script(type, id, script.slots);
        }
        finally
        {
            foreach (var s in strings) Marshal.FreeCoTaskMem(s);
        }
    }

    private static (uint Size, uint Align) Measure(FieldType t) => t switch
    {
        FieldType.Bool or FieldType.U8 => (1, 1),
        FieldType.Vec2 => (8, 4),
        FieldType.Color => (4, 1),
        FieldType.Rect => (16, 4),
        FieldType.Entity => (8, 8),
        _ => (4, 4),
    };

    /// <summary>The component's bytes into the instance's fields</summary>
    public void Read(Behaviour b, byte* p)
    {
        foreach (var s in slots)
        {
            var at = p + s.Offset;
            object v = s.Type switch
            {
                FieldType.F32 => Unsafe.ReadUnaligned<float>(at),
                FieldType.I32 => Unsafe.ReadUnaligned<int>(at),
                FieldType.U32 => Unsafe.ReadUnaligned<uint>(at),
                FieldType.Bool => *at != 0,
                FieldType.U8 => *at,
                FieldType.Vec2 => Unsafe.ReadUnaligned<Vec2>(at),
                FieldType.Color => Unsafe.ReadUnaligned<Color>(at),
                FieldType.Rect => Unsafe.ReadUnaligned<Rect>(at),
                _ => new Entity(Unsafe.ReadUnaligned<ulong>(at)),
            };
            if (s.Field.FieldType.IsEnum) v = Enum.ToObject(s.Field.FieldType, v);
            s.Field.SetValue(b, v);
        }
    }

    /// <summary>The instance's fields into the component's bytes</summary>
    public void Write(Behaviour b, byte* p)
    {
        foreach (var s in slots)
        {
            var at = p + s.Offset;
            var v = s.Field.GetValue(b)!;
            if (s.Field.FieldType.IsEnum) v = Convert.ChangeType(v, Enum.GetUnderlyingType(s.Field.FieldType));
            switch (s.Type)
            {
                case FieldType.F32: Unsafe.WriteUnaligned(at, (float)v); break;
                case FieldType.I32: Unsafe.WriteUnaligned(at, (int)v); break;
                case FieldType.U32: Unsafe.WriteUnaligned(at, (uint)v); break;
                case FieldType.Bool: *at = (bool)v ? (byte)1 : (byte)0; break;
                case FieldType.U8: *at = (byte)v; break;
                case FieldType.Vec2: Unsafe.WriteUnaligned(at, (Vec2)v); break;
                case FieldType.Color: Unsafe.WriteUnaligned(at, (Color)v); break;
                case FieldType.Rect: Unsafe.WriteUnaligned(at, (Rect)v); break;
                default: Unsafe.WriteUnaligned(at, ((Entity)v).Id); break;
            }
        }
    }
}
