using System.Runtime.CompilerServices;

namespace OverSight;

/// <summary>An entity id; <see cref="None"/> (0) is no entity</summary>
public readonly struct Entity : IEquatable<Entity>
{
    public readonly ulong Id;
    public Entity(ulong id) { Id = id; }
    public static readonly Entity None = default;
    public bool IsNone => Id == 0;
    public bool Equals(Entity o) => Id == o.Id;
    public override bool Equals(object? o) => o is Entity e && Equals(e);
    public override int GetHashCode() => Id.GetHashCode();
    public static bool operator ==(Entity a, Entity b) => a.Id == b.Id;
    public static bool operator !=(Entity a, Entity b) => a.Id != b.Id;
    public override string ToString() => $"Entity({(uint)Id}v{Id >> 32})";
}

/// <summary>Registers a component under another name than the struct's
/// (a C++ game names it with OV_COMPONENT)</summary>
[AttributeUsage(AttributeTargets.Struct)]
public sealed class ComponentAttribute : Attribute
{
    public string Name { get; }
    public ComponentAttribute(string name) { Name = name; }
}

public delegate void EntityAction<T1>(Entity e, ref T1 a) where T1 : unmanaged;
public delegate void EntityAction<T1, T2>(Entity e, ref T1 a, ref T2 b) where T1 : unmanaged where T2 : unmanaged;
public delegate void EntityAction<T1, T2, T3>(Entity e, ref T1 a, ref T2 b, ref T3 c)
    where T1 : unmanaged where T2 : unmanaged where T3 : unmanaged;

/// <summary>
/// Entities and components. A component is an unmanaged struct (no
/// classes or strings inside), registered by its name:
/// <code>
/// struct Pos { public float X, Y; }
/// var hero = engine.Spawn();
/// engine.Set(hero, new Pos { X = 10, Y = 20 });
/// engine.Get&lt;Pos&gt;(hero).X += 1;
/// engine.ForEach((Entity e, ref Pos p, ref Vel v) => { p.X += v.X; });
/// </code>
/// A ref from Get stays valid until the next Spawn, Despawn, Set or Remove.
/// The same calls in the same order give the same ids and the same order
/// on every machine.
/// </summary>
public sealed partial class Engine
{
    private readonly Dictionary<Type, uint> components = new();

    private struct AlignOf<T> where T : unmanaged
    {
#pragma warning disable CS0649
        public byte Pad;
        public T Value;
#pragma warning restore CS0649
    }

    /// <summary>The component id of T, registered on first use</summary>
    public unsafe uint Component<T>() where T : unmanaged
    {
        if (components.TryGetValue(typeof(T), out uint id)) return id;
        var name = typeof(T).GetCustomAttributes(typeof(ComponentAttribute), false) is [ComponentAttribute a, ..]
            ? a.Name
            : typeof(T).Name;
        uint size = (uint)sizeof(T);
        uint align = (uint)(sizeof(AlignOf<T>) - sizeof(T));
        id = Check(Native.ov_component(Handle, name, size, align), "component " + name);
        components[typeof(T)] = id;
        return id;
    }

    public Entity Spawn() => new(Native.ov_spawn(Handle));
    public bool Despawn(Entity e) => Native.ov_despawn(Handle, e.Id);
    public bool Alive(Entity e) => Native.ov_alive(Handle, e.Id);
    public uint EntityCount => Native.ov_entity_count(Handle);

    /// <summary>Attach or overwrite a component</summary>
    public unsafe void Set<T>(Entity e, in T value) where T : unmanaged
    {
        fixed (T* p = &value)
        {
            if (!Native.ov_set(Handle, e.Id, Component<T>(), (IntPtr)p))
                throw new OverSightException($"{typeof(T).Name}: {e} is not alive");
        }
    }

    /// <summary>The component in place: changes to the ref change the world</summary>
    public unsafe ref T Get<T>(Entity e) where T : unmanaged
    {
        var p = Native.ov_get(Handle, e.Id, Component<T>());
        if (p == IntPtr.Zero) throw new OverSightException($"{e} has no {typeof(T).Name}");
        return ref Unsafe.AsRef<T>((void*)p);
    }

    public unsafe bool TryGet<T>(Entity e, out T value) where T : unmanaged
    {
        var p = Native.ov_get(Handle, e.Id, Component<T>());
        value = p == IntPtr.Zero ? default : *(T*)p;
        return p != IntPtr.Zero;
    }

    public bool Has<T>(Entity e) where T : unmanaged => Native.ov_has(Handle, e.Id, Component<T>());
    public bool Remove<T>(Entity e) where T : unmanaged => Native.ov_remove(Handle, e.Id, Component<T>());
    /// <summary>Despawn everything; component types stay</summary>
    public void ClearWorld() => Native.ov_world_clear(Handle);

    /// <summary>Entities, components, random generator and tick counter as
    /// bytes: save them, or Restore them to continue exactly from here</summary>
    public byte[] Snapshot()
    {
        var bytes = new byte[(int)Native.ov_snapshot(Handle, null, 0)];
        Native.ov_snapshot(Handle, bytes, (nuint)bytes.Length);
        return bytes;
    }

    /// <summary>Continue from a snapshot; throws (and changes nothing) on bad bytes</summary>
    public void Restore(byte[] snapshot)
    {
        if (!Native.ov_restore(Handle, snapshot, (nuint)snapshot.Length))
            throw new OverSightException("restore: " + LastError);
    }

    /// <summary>Fingerprint of the snapshot: games running in step have equal hashes</summary>
    public ulong StateHash => Native.ov_state_hash(Handle);

    /// <summary>Entities that have all of the components (none: every entity)</summary>
    public Entity[] Query(params uint[] components)
    {
        var ids = new ulong[EntityCount];
        uint n = Native.ov_query(Handle, components, (uint)components.Length, ids, (uint)ids.Length);
        var result = new Entity[Math.Min(n, (uint)ids.Length)];
        for (int i = 0; i < result.Length; i++) result[i] = new Entity(ids[i]);
        return result;
    }

    public Entity[] Query<T1>() where T1 : unmanaged => Query(Component<T1>());
    public Entity[] Query<T1, T2>() where T1 : unmanaged where T2 : unmanaged
        => Query(Component<T1>(), Component<T2>());
    public Entity[] Query<T1, T2, T3>() where T1 : unmanaged where T2 : unmanaged where T3 : unmanaged
        => Query(Component<T1>(), Component<T2>(), Component<T3>());

    // ForEach: spawning or changing which components an entity has inside
    // the action moves data; collect such changes and do them after.
    public unsafe void ForEach<T1>(EntityAction<T1> action) where T1 : unmanaged
    {
        uint a = Component<T1>();
        foreach (var e in Query(a))
            action(e, ref Unsafe.AsRef<T1>((void*)Native.ov_get(Handle, e.Id, a)));
    }

    public unsafe void ForEach<T1, T2>(EntityAction<T1, T2> action) where T1 : unmanaged where T2 : unmanaged
    {
        uint a = Component<T1>(), b = Component<T2>();
        foreach (var e in Query(a, b))
            action(e, ref Unsafe.AsRef<T1>((void*)Native.ov_get(Handle, e.Id, a)),
                ref Unsafe.AsRef<T2>((void*)Native.ov_get(Handle, e.Id, b)));
    }

    public unsafe void ForEach<T1, T2, T3>(EntityAction<T1, T2, T3> action)
        where T1 : unmanaged where T2 : unmanaged where T3 : unmanaged
    {
        uint a = Component<T1>(), b = Component<T2>(), c = Component<T3>();
        foreach (var e in Query(a, b, c))
            action(e, ref Unsafe.AsRef<T1>((void*)Native.ov_get(Handle, e.Id, a)),
                ref Unsafe.AsRef<T2>((void*)Native.ov_get(Handle, e.Id, b)),
                ref Unsafe.AsRef<T3>((void*)Native.ov_get(Handle, e.Id, c)));
    }
}
