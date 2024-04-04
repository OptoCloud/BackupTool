using System.Diagnostics;
using System.Security.Cryptography;

namespace BackupTool.Utils;

public static class HashingUtils
{
    const int ReportBytesInterval = 1024 * 1024 * 10; // 10 MB

    public static string? GetIOExceptionMessage(IOException ex)
    {
        return ex.HResult switch
        {
            -2146233067 => "Operation is not supported. (0x80131515: E_NOTSUPPORTED)",
            -2147024784 => "There is not enough space on the disk. (0x80070070: ERROR_DISK_FULL)",
            -2147024864 => "The file is being used by another process. (0x80070020: ERROR_SHARING_VIOLATION)",
            -2147024882 => "There is not enough memory (RAM). (0x8007000E: E_OUTOFMEMORY)",
            -2147024809 => "Invalid arguments provided. (0x80070057: E_INVALIDARG)",
            -2147467263 => "Functionality not implemented. (0x80004001: E_NOTIMPL)",
            -2147024891 => "Access is denied. (0x80070005: E_ACCESSDENIED)",
            _ => ex.Message == string.Empty ? null : ex.Message
        };
    }

    public static async Task<byte[]> HashAsync(Stream stream, int hashingBlockSize, Action<ulong> progressCallback, CancellationToken cancellationToken = default)
    {
        if (!stream.CanRead) throw new ArgumentException("Stream cannot be read!", nameof(stream));

        byte[] buffer = new byte[hashingBlockSize];
        using SHA256 sha256 = SHA256.Create();

        ulong bytesReadTotal = 0;
        ulong lastReportBytesRead = 0;
        while (true)
        {
            int nRead = await stream.ReadAsync(buffer, cancellationToken);
            if (nRead <= 0) break;

            int nWritten = sha256.TransformBlock(buffer, 0, nRead, buffer, 0);
            if (nWritten != nRead) throw new UnreachableException("Failed to write to SHA256");

            bytesReadTotal += (ulong)nWritten;

            if (bytesReadTotal - lastReportBytesRead >= ReportBytesInterval)
            {
                progressCallback.Invoke(bytesReadTotal);
                lastReportBytesRead = bytesReadTotal;
            }
        }

        sha256.TransformFinalBlock(buffer, 0, 0);

        progressCallback.Invoke(bytesReadTotal);

        return sha256.Hash ?? throw new UnreachableException("Failed to compute SHA256");
    }
}
