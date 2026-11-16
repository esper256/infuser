// Infuser developer tests only. These are NOT Unity assemblies or an IL2CPP test.
using System;
using System.Collections;
using System.Text.Json;
namespace UnityEngine
{
    public class MonoBehaviour
    {
        public GameObject gameObject = new GameObject("");
        protected static void DontDestroyOnLoad(object value) { }
        protected Coroutine StartCoroutine(IEnumerator value) => new Coroutine();
        protected void StopCoroutine(Coroutine value) { }
    }
    public class GameObject { public string name; public GameObject(string name) { this.name = name; } public T AddComponent<T>() where T : new() => new T(); }
    public class Coroutine { }
    public class WaitForSecondsRealtime { public WaitForSecondsRealtime(float seconds) { } }
    public static class Debug { public static void Log(string value) => Console.WriteLine(value); }
    public static class JsonUtility
    {
        private static readonly JsonSerializerOptions options = new JsonSerializerOptions { IncludeFields = true, IgnoreReadOnlyProperties = true };
        public static string ToJson(object value) => JsonSerializer.Serialize(value, value.GetType(), options);
        public static T FromJson<T>(string value) => JsonSerializer.Deserialize<T>(value, options);
    }
}
namespace UnityEngine.Scripting { public class PreserveAttribute : Attribute { } }
namespace UnityEditor
{
    public enum BuildTarget { WebGL, StandaloneLinux64 }
    public class MenuItem : Attribute { public MenuItem(string path) { } }
    public static class EditorUtility { public static string OpenFolderPanel(string title, string folder, string name) => ""; }
    public static class AssetDatabase { public static string ScriptPath; public static string GUIDToAssetPath(string guid) => ScriptPath; }
}
namespace UnityEditor.PackageManager
{
    public class PackageInfo
    {
        public string resolvedPath, assetPath;
        public static PackageInfo Current;
        public static PackageInfo FindForAssetPath(string path) => Current;
    }
}
namespace UnityEditor.Build
{
    public class BuildFailedException : Exception { public BuildFailedException(string message) : base(message) { } }
    public interface IPostprocessBuildWithReport { int callbackOrder { get; } void OnPostprocessBuild(UnityEditor.Build.Reporting.BuildReport report); }
}
namespace UnityEditor.Build.Reporting
{
    public class BuildSummary { public UnityEditor.BuildTarget platform; public string outputPath; }
    public class BuildReport { public BuildSummary summary; }
}
