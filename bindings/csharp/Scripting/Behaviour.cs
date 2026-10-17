using ORST.Scripting;

namespace ORST;

/// <summary>
/// A script on an entity, as in Unity. Its public fields of the types
/// components have (numbers, bool, Vec2, Color, Rect, Entity, enums) are
/// a component named after the class: the Inspector shows them, scenes
/// save them, and the initial values are the defaults.
/// <code>
/// public class Spin : Behaviour
/// {
///     [Range(0, 10)] public float Speed = 2;
///     public override void Update(float dt) => Transform.Rotation += Speed * dt;
/// }
/// </code>
/// Start runs before the first Update of each entity; Update runs every
/// tick (60 a second) before the engine moves things, after the contact
/// calls for what the engine found in the tick before.
/// </summary>
public abstract class Behaviour
{
    /// <summary>The entity this script is on</summary>
    public Entity Entity { get; internal set; }

    /// <summary>The game's engine</summary>
    public Engine Engine => Host.Engine ?? throw new InvalidOperationException("no engine yet");

    /// <summary>The entity's Transform, in place</summary>
    public ref Transform Transform => ref Engine.Get<Transform>(Entity);

    public virtual void Start() { }
    public virtual void Update(float dt) { }
    /// <summary>The entity's collider began to touch another's (or to
    /// overlap a trigger); seen in the tick after the engine found it</summary>
    public virtual void OnContactBegin(Entity other, Contact contact) { }
    /// <summary>The two stopped touching</summary>
    public virtual void OnContactEnd(Entity other, Contact contact) { }

    /// <summary>A line in the Console</summary>
    public static void Log(object? message) => Host.Log(0, message?.ToString() ?? "null");
    public static void Warn(object? message) => Host.Log(1, message?.ToString() ?? "null");
    public static void Error(object? message) => Host.Log(2, message?.ToString() ?? "null");
}
