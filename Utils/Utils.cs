using OptoPacker.DTOs;
using System.Security.Cryptography;

namespace OptoPacker.Utils;

internal static class Utilss
{
    [ThreadStatic]
    static SHA256? _sha256;
    static SHA256 Sha256 => _sha256 ??= SHA256.Create();

    public static async Task<(byte[] hash, long length)> HashAsync(Stream stream)
    {
        var hash = await Sha256.ComputeHashAsync(stream);
        return (hash, (int)stream.Length);
    }

    public static async Task<(byte[] hash, long length)> HashAsync(string path)
    {
        using var stream = File.OpenRead(path);
        return await HashAsync(stream);
    }

    public static async IAsyncEnumerable<InputFileInfo> HashAllAsync(string rootPath, IEnumerable<string> files)
    {
        foreach (var file in files)
        {
            byte[] hash;
            long size;
            try
            {
                (hash, size) = await HashAsync(file);
            }
            catch (Exception ex)
            {
                Console.WriteLine($"Skipping {Path.GetFileName(file)}: {ex.Message}");
                continue;
            }
            if (hash == null || size < 0)
            {
                Console.WriteLine($"Skipping {Path.GetFileName(file)}: Invalid hash or size");
                continue;
            }

            yield return new InputFileInfo(rootPath, (ulong)size, 0, hash);
        }
    }

    public static string FormatNumberByteSize(ulong bytes)
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

        return $"{f:0.00} {unit}".PadLeft(10);
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
