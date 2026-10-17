using System.Text;

namespace OverSight;

/// <summary>
/// Input actions: the game asks for "jump", not Space, and players rebind
/// it. Pressed and released are for the action as a whole, so two keys on
/// "jump" press once.
/// <code>
/// uint jump = engine.Action("jump");
/// engine.Bind(jump, Binding.Key(Key.Space));
/// engine.Bind(jump, Binding.Button(Pad.South));
/// if (engine.ActionPressed(jump)) ...
/// </code>
/// A rebinding menu waits for CapturedBinding (the first input pressed this
/// frame) and stores it with SetBinding. SaveBindings gives a text players
/// can edit: "jump = key:Space pad:South".
/// </summary>
public sealed partial class Engine
{
    /// <summary>The id of an action (from 1), made on first use</summary>
    public uint Action(string name) => Check(Native.ov_action(Handle, name), "action " + name);

    /// <summary>Add a binding; one already there is not added twice</summary>
    public bool Bind(uint action, Binding binding) => Native.ov_bind(Handle, action, binding);
    /// <summary>Replace binding index, or add it when index is the count</summary>
    public bool SetBinding(uint action, int index, Binding binding) => Native.ov_set_binding(Handle, action, (uint)index, binding);
    public bool RemoveBinding(uint action, int index) => Native.ov_remove_binding(Handle, action, (uint)index);
    public void ClearBindings(uint action) => Native.ov_clear_bindings(Handle, action);

    public Binding[] Bindings(uint action)
    {
        var all = new Binding[Native.ov_binding_count(Handle, action)];
        for (int i = 0; i < all.Length; i++) Native.ov_get_binding(Handle, action, (uint)i, out all[i]);
        return all;
    }

    /// <summary>"key:Space", "pad:South", "pad1:LeftX-"</summary>
    public string BindingText(Binding binding)
    {
        var n = Native.ov_binding_text(Handle, binding, null, 0);
        var buf = new byte[(int)n + 1];
        Native.ov_binding_text(Handle, binding, buf, (nuint)buf.Length);
        return Encoding.UTF8.GetString(buf, 0, (int)n);
    }

    public bool ActionDown(uint action) => Native.ov_action_down(Handle, action);
    /// <summary>This frame only</summary>
    public bool ActionPressed(uint action) => Native.ov_action_pressed(Handle, action);
    public bool ActionReleased(uint action) => Native.ov_action_released(Handle, action);
    /// <summary>0..1; sticks give how far they are pushed</summary>
    public float ActionValue(uint action) => Native.ov_action_value(Handle, action);
    /// <summary>-1..1 from two opposite actions</summary>
    public float ActionAxis(uint negative, uint positive) => Native.ov_action_axis(Handle, negative, positive);

    /// <summary>The first input pressed this frame, for "press a key" menus</summary>
    public Binding? CapturedBinding => Native.ov_captured_binding(Handle, out var b) ? b : null;

    public string SaveBindings()
    {
        var n = Native.ov_bindings_save(Handle, null, 0);
        var buf = new byte[(int)n + 1];
        Native.ov_bindings_save(Handle, buf, (nuint)buf.Length);
        return Encoding.UTF8.GetString(buf, 0, (int)n);
    }

    /// <summary>Actions in the text get exactly its bindings, others keep
    /// theirs; throws (and changes nothing) on a bad line</summary>
    public void LoadBindings(string text)
    {
        if (!Native.ov_bindings_load(Handle, text)) throw new OverSightException(LastError);
    }
}
