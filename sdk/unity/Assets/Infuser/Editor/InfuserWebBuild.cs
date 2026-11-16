using System.IO;
using UnityEditor;
using UnityEditor.Build;
using UnityEditor.Build.Reporting;

// Copy Assets/Infuser as a unit. No custom Web template or Unity package required.
public sealed class InfuserWebBuild : IPostprocessBuildWithReport
{
    public int callbackOrder => 100;
    public void OnPostprocessBuild(BuildReport report)
    {
        if (report.summary.platform != BuildTarget.WebGL) return;
        const string source = "Assets/Infuser/Web/bridge.mjs";
        if (!File.Exists(source)) throw new BuildFailedException("Missing " + source + ". Copy the complete Infuser adapter into Assets/Infuser.");
        File.Copy(source, Path.Combine(report.summary.outputPath, "infuser-unity.mjs"), true);
    }
}
