using System.Text;

namespace OverSight;

/// <summary>A loaded skeleton file; skeletons made from it keep working after it is disposed</summary>
public sealed class SkeletonData : Resource
{
    internal SkeletonData(Engine e, uint id) : base(e, id) { }
    protected override void Free(IntPtr e, uint id) => Native.ov_free_skeleton_data(e, id);
}

/// <summary>
/// An animated skeleton: bones, slots showing regions of one atlas, and
/// animations. The file is JSON with angles in degrees, clockwise; here
/// angles are radians.
/// </summary>
public sealed class Skeleton : Resource
{
    internal Skeleton(Engine e, uint id) : base(e, id) { }
    protected override void Free(IntPtr e, uint id) => Native.ov_free_skeleton(e, id);

    private IntPtr E => Engine.Handle;

    /// <summary>Fades from the current animation over mix seconds; the one already playing keeps going</summary>
    public void Play(string name, bool looped = true, float mix = 0f)
    {
        if (!Native.ov_skeleton_play(E, Id, name, looped, mix)) throw new OverSightException(Engine.LastError);
    }

    public void Stop() => Native.ov_skeleton_stop(E, Id);

    /// <summary>The current animation's name ("" for none)</summary>
    public string Animation
    {
        get
        {
            var n = Native.ov_skeleton_animation(E, Id, null, 0);
            var buf = new byte[(int)n + 1];
            Native.ov_skeleton_animation(E, Id, buf, (nuint)buf.Length);
            return Encoding.UTF8.GetString(buf, 0, (int)n);
        }
    }

    /// <summary>A non-looping animation reached its end</summary>
    public bool Done => Native.ov_skeleton_done(E, Id);

    /// <summary>The event was passed in the last update</summary>
    public bool Fired(string @event) => Native.ov_skeleton_fired(E, Id, @event);

    /// <summary>Where the skeleton stands</summary>
    public Transform Root { set => Native.ov_set_skeleton_root(E, Id, value); }
    public Color Tint { set => Native.ov_set_skeleton_tint(E, Id, value); }
    public float Speed { set => Native.ov_set_skeleton_speed(E, Id, value); }

    /// <summary>Attachments as sprites of the atlas, at the current layer, depth and space</summary>
    public void Draw(Texture atlas) => Native.ov_draw_skeleton(E, Id, atlas.Id);

    /// <summary>A bone's index, -1 if none</summary>
    public int Bone(string name) => Native.ov_skeleton_bone(E, Id, name);

    public Transform BoneWorld(int bone) => Native.ov_skeleton_bone_world(E, Id, bone);

    /// <summary>A point in a bone's own space, in the world (a muzzle's tip)</summary>
    public Vec2 BonePoint(int bone, Vec2 local) => Native.ov_skeleton_bone_point(E, Id, bone, local);

    /// <summary>Turn a bone by code, blended over the animation by weight; 0 gives it back</summary>
    public void SetOverride(int bone, float rotation, float weight = 1f) =>
        Native.ov_set_bone_override(E, Id, bone, rotation, weight);

    /// <summary>null hides the slot</summary>
    public void SetAttachment(string slot, string? attachment)
    {
        if (!Native.ov_skeleton_set_attachment(E, Id, slot, attachment!)) throw new OverSightException(Engine.LastError);
    }
}

public sealed partial class Engine
{
    public SkeletonData LoadSkeleton(string path) => new(this, Check(Native.ov_load_skeleton(Handle, path), path));

    public SkeletonData SkeletonFromJson(string json) =>
        new(this, Check(Native.ov_skeleton_from_json(Handle, json), "skeleton"));

    public Skeleton CreateSkeleton(SkeletonData data) =>
        new(this, Check(Native.ov_create_skeleton(Handle, data.Id), "skeleton"));

    /// <summary>Advance every skeleton by dt seconds</summary>
    public void UpdateSkeletons(float dt) => Native.ov_update_skeletons(Handle, dt);
}
