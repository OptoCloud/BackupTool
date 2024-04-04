namespace BackupTool.Utils;

internal static class PathUtils
{
    public static readonly char[] PathSeperators = ['\\', '/'];

    public static bool IsDirectory(string path)
    {
        return path.EndsWith('\\') || path.EndsWith('/');
    }

    public static string GetDiretoryPath(string path)
    {
        if (IsDirectory(path)) return path;

        int idx = path.LastIndexOfAny(PathSeperators);
        if (idx == -1) return string.Empty;

        return path[..(idx + 1)];
    }

    public static (string name, string extension) SplitFileName(string fileName)
    {
        int idx = fileName.LastIndexOf('.');
        if (idx < 0) return (fileName, string.Empty);

        return (fileName[..idx], fileName[(idx + 1)..]);
    }

    public static string GetExtension(string path)
    {
        int idx = path.LastIndexOf('.');
        if (idx < 0) return string.Empty;

        int sepIdx = path.LastIndexOfAny(PathSeperators);
        if (sepIdx > idx) return String.Empty;

        return path[(idx + 1)..];
    }

    public static string[] GetPathParts(string path)
    {
        return path.Split(PathSeperators, StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
    }
}
