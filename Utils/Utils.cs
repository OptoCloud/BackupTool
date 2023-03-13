namespace OptoPacker.Utils;

internal static class Utilss
{

    public static string FormatNumberByteSize(ulong bytes, int padding = -1)
    {
        uint i = 0;
        float f = bytes;
        while (f > 1024f)
        {
            f /= 1024f;
            i++;
        }

        string unit = i switch
        {
            0 => " B",
            1 => "KB",
            2 => "MB",
            3 => "GB",
            4 => "TB",
            5 => "PB",
            6 => "EB",
            7 => "ZB",
            8 => "YB",
            _ => "??"
        };

        string str = $"{f:0.00} {unit}";
        return padding > 0 ? str.PadLeft(padding) : str;
    }

    public static string GetRelativeName(string rootPath, string subPath, bool isDir)
    {
        if (rootPath.Length > subPath.Length) throw new ArgumentException("rootPath must be shorter than subPath");

        var relativePath = subPath[rootPath.Length..];

        if (isDir && !relativePath.EndsWith('\\'))
            relativePath += '\\';

        return relativePath;
    }
}
