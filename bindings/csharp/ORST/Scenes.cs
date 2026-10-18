namespace ORST;

/// <summary>
/// Scenes: numbers the game picks (0 = none), such as a menu, a level or a
/// pause screen. The engine keeps a stack of them and covers each switch
/// with a transition. The switch happens in its middle, when the screen is
/// covered, and SceneEntered reports it for that one frame:
/// <code>
/// if (engine.SceneEntered == (uint)Scene.Level) LoadLevel();
/// </code>
/// A change while another runs is refused (false).
/// </summary>
public sealed partial class Engine
{
    public bool GoScene(uint scene, Transition transition = Transition.Fade, float seconds = 0.5f, Color? color = null) =>
        Native.orst_scene_go(Handle, scene, (int)transition, seconds, color ?? Color.Black);

    /// <summary>Put a scene over the current one (a pause menu)</summary>
    public bool PushScene(uint scene, Transition transition = Transition.Fade, float seconds = 0.5f, Color? color = null) =>
        Native.orst_scene_push(Handle, scene, (int)transition, seconds, color ?? Color.Black);

    /// <summary>Back to the scene below; false when there is none</summary>
    public bool PopScene(Transition transition = Transition.Fade, float seconds = 0.5f, Color? color = null) =>
        Native.orst_scene_pop(Handle, (int)transition, seconds, color ?? Color.Black);

    /// <summary>The scene on top</summary>
    public uint Scene => Native.orst_scene(Handle);
    /// <summary>Became current this frame (after a pop: resumed); 0: none</summary>
    public uint SceneEntered => Native.orst_scene_entered(Handle);
    /// <summary>Stopped being current this frame; 0: none</summary>
    public uint SceneLeft => Native.orst_scene_left(Handle);
    public uint SceneDepth => Native.orst_scene_depth(Handle);
    public bool SceneTransitioning => Native.orst_scene_transitioning(Handle);
    /// <summary>How covered the screen is, 0..1</summary>
    public float SceneCover => Native.orst_scene_cover(Handle);
}
