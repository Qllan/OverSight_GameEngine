using System.Reflection;
using System.Runtime.InteropServices;

namespace ORST;

/// <summary>Field types of a component, as in orst.h</summary>
public enum FieldType : uint
{
    F32 = 1, I32 = 2, U32 = 3, Bool = 4, Vec2 = 5, Color = 6, Rect = 7, Entity = 8,
    Texture = 9, Tilemap = 10, Polygon = 11, U8 = 12, Text = 13,
}

[Flags]
public enum FieldFlags : uint { None = 0, Angle = 1, Layers = 2 }

/// <summary>A slider from min to max in the editor</summary>
[AttributeUsage(AttributeTargets.Field)]
public sealed class RangeAttribute : Attribute
{
    public float Min { get; }
    public float Max { get; }
    public RangeAttribute(float min, float max) { Min = min; Max = max; }
}

/// <summary>A float in radians, shown in degrees</summary>
[AttributeUsage(AttributeTargets.Field)]
public sealed class AngleAttribute : Attribute { }

/// <summary>A uint of layer bits, shown as checkboxes</summary>
[AttributeUsage(AttributeTargets.Field)]
public sealed class LayersAttribute : Attribute { }

/// <summary>Left out of the editor and of scene files</summary>
[AttributeUsage(AttributeTargets.Field)]
public sealed class HiddenAttribute : Attribute { }

/// <summary>One field of a component, as tools see it</summary>
public sealed record ComponentField(string Name, FieldType Type, uint Offset, FieldFlags Flags,
                                    float Min, float Max, string[] Options);

public sealed partial class Engine
{
    /// <summary>What the editor and scene files know about a component</summary>
    public ComponentField[] Fields(uint component)
    {
        var n = Native.orst_field_count(Handle, component);
        var fields = new ComponentField[n];
        for (uint i = 0; i < n; i++)
        {
            if (!Native.orst_get_field(Handle, component, i, out var f)) throw new OrstException(LastError);
            var options = Marshal.PtrToStringUTF8(f.Options) is { Length: > 0 } o ? o.Split('|') : [];
            fields[i] = new(Marshal.PtrToStringUTF8(f.Name)!, (FieldType)f.Type, f.Offset, (FieldFlags)f.Flags,
                            f.Min, f.Max, options);
        }
        return fields;
    }

    public ComponentField[] Fields<T>() where T : unmanaged => Fields(Component<T>());

    /// <summary>Describe T's public fields to the engine: the ones of
    /// supported types (numbers, bool, Vec2, Color, Rect, Entity, enums
    /// numbered 0, 1, 2...) without [Hidden]. Called on registration; the
    /// built-ins are described by the engine itself.</summary>
    private unsafe void DescribeFields<T>(uint id) where T : unmanaged
    {
        if (typeof(T).Assembly == typeof(Engine).Assembly) return;
        var fields = new List<(ComponentField Field, IntPtr Name, IntPtr Options)>();
        try
        {
            foreach (var fi in typeof(T).GetFields(BindingFlags.Public | BindingFlags.Instance))
            {
                if (fi.GetCustomAttribute<HiddenAttribute>() != null || Describe(fi) is not { } f) continue;
                f = f with { Offset = OffsetOf<T>(fi) };
                fields.Add((f, Marshal.StringToCoTaskMemUTF8(f.Name),
                            f.Options.Length > 0 ? Marshal.StringToCoTaskMemUTF8(string.Join('|', f.Options)) : IntPtr.Zero));
            }
            var native = fields.Select(x => new NativeField
            {
                Name = x.Name, Type = (uint)x.Field.Type, Offset = x.Field.Offset, Flags = (uint)x.Field.Flags,
                Min = x.Field.Min, Max = x.Field.Max, Options = x.Options,
            }).ToArray();
            if (!Native.orst_set_fields(Handle, id, native, (uint)native.Length))
                throw new OrstException(LastError);
        }
        finally
        {
            foreach (var (_, name, options) in fields)
            {
                Marshal.FreeCoTaskMem(name);
                Marshal.FreeCoTaskMem(options);
            }
        }
    }

    private static ComponentField? Describe(FieldInfo fi)
    {
        var t = fi.FieldType;
        string[] options = [];
        if (t.IsEnum)
        {
            var values = Enum.GetValuesAsUnderlyingType(t).Cast<object>().Select(Convert.ToInt64).ToArray();
            if (values.SequenceEqual(Enumerable.Range(0, values.Length).Select(i => (long)i)))
                options = Enum.GetNames(t);
            t = Enum.GetUnderlyingType(t);
        }
        FieldType? type = t == typeof(float) ? FieldType.F32
            : t == typeof(int) ? FieldType.I32
            : t == typeof(uint) ? FieldType.U32
            : t == typeof(bool) ? FieldType.Bool
            : t == typeof(byte) ? FieldType.U8
            : t == typeof(Vec2) ? FieldType.Vec2
            : t == typeof(Color) ? FieldType.Color
            : t == typeof(Rect) ? FieldType.Rect
            : t == typeof(Entity) ? FieldType.Entity
            : null;
        if (type is not { } ty) return null;
        var flags = (fi.GetCustomAttribute<AngleAttribute>() != null ? FieldFlags.Angle : 0)
                  | (fi.GetCustomAttribute<LayersAttribute>() != null ? FieldFlags.Layers : 0);
        var range = fi.GetCustomAttribute<RangeAttribute>();
        return new(fi.Name, ty, 0, flags, range?.Min ?? 0, range?.Max ?? 0, options);
    }

    /// <summary>Where a field lies in T's bytes. Marshal.OffsetOf gives the
    /// marshalled layout, which differs once a bool is in the struct, so
    /// set the field to a value with no zero byte and see where it lands.</summary>
    private static unsafe uint OffsetOf<T>(FieldInfo fi) where T : unmanaged
    {
        const float F = 2.3694278e-38f; // bits 0x01010101
        var t = fi.FieldType.IsEnum ? Enum.GetUnderlyingType(fi.FieldType) : fi.FieldType;
        object probe = t == typeof(float) ? F
            : t == typeof(int) ? 0x01010101
            : t == typeof(uint) ? 0x01010101u
            : t == typeof(bool) ? true
            : t == typeof(byte) ? (byte)1
            : t == typeof(Vec2) ? new Vec2(F, F)
            : t == typeof(Color) ? new Color(1, 1, 1, 1)
            : t == typeof(Rect) ? new Rect(F, F, F, F)
            : new Entity(0x0101010101010101);
        if (fi.FieldType.IsEnum) probe = Enum.ToObject(fi.FieldType, probe);
        object boxed = default(T);
        fi.SetValue(boxed, probe);
        var value = (T)boxed;
        var bytes = (byte*)&value;
        for (uint i = 0; i < sizeof(T); i++)
            if (bytes[i] != 0) return i;
        throw new OrstException($"field {fi.Name} not found in {typeof(T).Name}");
    }
}
