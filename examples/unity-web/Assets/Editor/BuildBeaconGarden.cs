using UnityEditor;
using UnityEditor.Build;
using UnityEditor.Build.Reporting;
using UnityEditor.SceneManagement;
using UnityEngine;

public static class BuildBeaconGarden
{
    [MenuItem("Infuser/Build Beacon Garden Web")]
    public static void Build()
    {
        var scene = EditorSceneManager.NewScene(NewSceneSetup.EmptyScene, NewSceneMode.Single);
        new GameObject("Beacon Garden").AddComponent<BeaconGarden>();
        EditorSceneManager.SaveScene(scene, "Assets/BeaconGarden.unity");
        PlayerSettings.companyName = "Infuser Examples";
        PlayerSettings.productName = "Beacon Garden";
        PlayerSettings.WebGL.compressionFormat = WebGLCompressionFormat.Gzip;
        PlayerSettings.WebGL.decompressionFallback = false;
        var report = BuildPipeline.BuildPlayer(new BuildPlayerOptions {
            scenes = new[] { "Assets/BeaconGarden.unity" }, locationPathName = "export",
            target = BuildTarget.WebGL, options = BuildOptions.None
        });
        if (report.summary.result != BuildResult.Succeeded) throw new BuildFailedException("Beacon Garden Web build failed; see the Unity Editor log.");
    }
}
