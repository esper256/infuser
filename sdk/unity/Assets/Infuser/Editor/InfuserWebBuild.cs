using System.IO;
using UnityEditor;
using UnityEditor.Build;
using UnityEditor.Build.Reporting;
using UnityEditor.PackageManager;
using UnityEngine;

// The same adapter works as a UPM package or a copied Assets folder.
public sealed class InfuserWebBuild : IPostprocessBuildWithReport
{
    private const string ScriptGuid = "fba60d8e8b044a8fbb6bc0fb20dd5802";
    public int callbackOrder => 100;
    public static string BridgeSource()
    {
        var script = AssetDatabase.GUIDToAssetPath(ScriptGuid);
        if (string.IsNullOrEmpty(script)) throw new BuildFailedException("Cannot locate the Infuser adapter. Keep its .meta files when copying, or install the complete package.");
        var package = PackageInfo.FindForAssetPath(script);
        var diskPath = package == null ? script : Path.Combine(package.resolvedPath, script.Substring(package.assetPath.Length + 1));
        return Path.GetFullPath(Path.Combine(Path.GetDirectoryName(diskPath), "../Web/bridge.mjs"));
    }
    public void OnPostprocessBuild(BuildReport report)
    {
        if (report.summary.platform != BuildTarget.WebGL) return;
        var source = BridgeSource();
        if (!File.Exists(source)) throw new BuildFailedException("Missing " + source + ". Restore the complete Infuser adapter before rebuilding.");
        File.Copy(source, Path.Combine(report.summary.outputPath, "infuser-unity.mjs"), true);
        ValidateExport(report.summary.outputPath);
    }
    public static void ValidateExport(string directory)
    {
        foreach (var name in new[] { "index.html", "infuser-unity.mjs" })
        {
            var path = Path.Combine(directory, name);
            if (!File.Exists(path) || new FileInfo(path).Length == 0)
                throw new BuildFailedException("Missing or empty export file: " + path + ". Rebuild Web before publishing.");
        }
    }
    [MenuItem("Infuser/Validate Web export")]
    public static void ValidateSelectedExport()
    {
        var directory = EditorUtility.OpenFolderPanel("Choose the Web export containing index.html", "", "");
        if (string.IsNullOrEmpty(directory)) return;
        ValidateExport(directory);
        Debug.Log("Infuser export files are present. Publish and test gameplay separately: " + directory);
    }
}
