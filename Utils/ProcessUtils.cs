using System.Diagnostics;

namespace BackupTool.Utils;

internal static class ProcessUtils
{
    public static Process StartProcess(string fileName, string arguments = "")
    {
        var process = new Process();
        process.StartInfo.FileName = fileName;
        process.StartInfo.Arguments = arguments;
        process.StartInfo.UseShellExecute = false;
        process.StartInfo.RedirectStandardInput = true;
        process.StartInfo.RedirectStandardOutput = true;
        process.Start();
        return process;
    }

    public static Process? StartProcessOrNull(string fileName, string arguments = "")
    {
        try
        {
            return StartProcess(fileName, arguments);
        }
        catch (Exception)
        {
            return null;
        }
    }
}
