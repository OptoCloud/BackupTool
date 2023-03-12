using OptoPacker.DTOs;
using System.Security.Cryptography;

namespace OptoPacker.Utils;

internal static class Utilss
{
    [ThreadStatic]
    static SHA256? _sha256;
    static SHA256 Sha256 => _sha256 ??= SHA256.Create();

    const int ChunkSize = 4096;
    public static async Task<(byte[]? hash, long length)> HashAsync(string path)
    {
        using var stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read, ChunkSize, true);

        byte[] buffer = new byte[ChunkSize];
        int nBytesRead;
        do
        {
            nBytesRead = await stream.ReadAsync(buffer, 0, buffer.Length);
            if (nBytesRead > 0)
            {
                Sha256.TransformBlock(buffer, 0, nBytesRead, buffer, 0);
            }
        } while (nBytesRead > 0);

        Sha256.TransformFinalBlock(buffer, 0, 0);
        
        return (Sha256.Hash, stream.Length);
    }

    public static async IAsyncEnumerable<InputFileInfo> HashAllAsync(string rootPath, IEnumerable<string> files)
    {
        foreach (var file in files)
        {
            byte[]? hash;
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

            yield return new InputFileInfo(file, (ulong)size, 0, hash);
        }
    }

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
