using OptoPacker.DTOs;
using System.Runtime.CompilerServices;
using System.Security.Cryptography;

namespace OptoPacker.Utils;

public static class FileUtils
{
    const int ReportBytesInterval = 1024 * 1024 * 10; // 10 MB

    private delegate void SingleFileStatusReport(ulong bytesRead);
    private static async Task<byte[]?> HashAsync(FileStream fileStream, int chunkSize = 4096, SingleFileStatusReport? statusReport = null, CancellationToken cancellationToken = default)
    {
        if (!fileStream.CanRead) return null;

        byte[] buffer = new byte[chunkSize];
        using SHA256 sha256 = SHA256.Create();

        int bytesReadChunk;
        ulong bytesReadTotal = 0;
        ulong lastReportBytesRead = 0;
        do
        {
            bytesReadChunk = await fileStream.ReadAsync(buffer, 0, buffer.Length, cancellationToken);
            if (bytesReadChunk > 0)
            {
                sha256.TransformBlock(buffer, 0, bytesReadChunk, buffer, 0);

                bytesReadTotal += (ulong)bytesReadChunk;
                if (bytesReadTotal - lastReportBytesRead >= ReportBytesInterval)
                {
                    statusReport?.Invoke(bytesReadTotal);
                    lastReportBytesRead = bytesReadTotal;
                }
            }
        } while (bytesReadChunk > 0);

        sha256.TransformFinalBlock(buffer, 0, 0);

        return sha256.Hash;
    }
    public static async Task<(byte[]? hash, long length)> HashAsync(string path, int chunkSize = 4096, CancellationToken cancellationToken = default)
    {
        using FileStream stream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read, chunkSize, true);

        byte[]? hash = await HashAsync(stream, chunkSize, null, cancellationToken);

        return (hash, stream.Length);
    }
    private static InputFileInfo EmptyInputFileInfo(string path) => new InputFileInfo(path, 0, Array.Empty<byte>());
    private static InputFileInfo ErrorInputFileInfo(string path, ulong size, string error) => new InputFileInfo(path, size, Array.Empty<byte>(), error);
    public record struct MultiFileStatusReport(uint filesTotal, uint filesProcessed, uint filesFailed, ulong bytesTotal, ulong bytesProcessed)
    {
        public static MultiFileStatusReport operator +(MultiFileStatusReport a, MultiFileStatusReport b)
        {
            return new MultiFileStatusReport(
                a.filesTotal + b.filesTotal,
                a.filesProcessed + b.filesProcessed,
                a.filesFailed + b.filesFailed,
                a.bytesTotal + b.bytesTotal,
                a.bytesProcessed + b.bytesProcessed
                );
        }
    }
    public delegate void MultiFileStatusReportFunc(MultiFileStatusReport statusReport);
    public static async IAsyncEnumerable<InputFileInfo> HashAllAsync(string rootPath, string[] filePaths, int chunkSize = 4096, MultiFileStatusReportFunc? statusReportFunc = null, [EnumeratorCancellation] CancellationToken cancellationToken = default)
    {
        MultiFileStatusReport statusReport = new MultiFileStatusReport((uint)filePaths.Length, 0, 0, 0, 0);

        List<(string path, FileStream fileStream)> files = new List<(string path, FileStream fileStream)>();
        foreach (string path in filePaths)
        {
            string? error = null;
            FileStream? fileStream = null;
            try
            {
                fileStream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read, chunkSize, true);
                if (fileStream.Length < 0)
                {
                    fileStream?.Dispose();
                    error = "Failed to get stream length";
                }
            }
            catch (Exception ex)
            {
                error = ex.Message;
                if (error == string.Empty) error = null;
            }

            if (fileStream == null)
            {
                yield return ErrorInputFileInfo(path, 0, error ?? "Unkown error while opening file");
                statusReport.filesFailed++;
                continue;
            }

            files.Add((path, fileStream));
        }

        statusReport.bytesTotal = (ulong)files.Sum(f => f.fileStream.Length);

        foreach ((string path, FileStream fileStream) in files)
        {
            cancellationToken.ThrowIfCancellationRequested();

            ulong fileSize = (ulong)fileStream.Length;

            if (fileSize == 0)
            {
                yield return EmptyInputFileInfo(path);
                statusReport.filesProcessed++;
                continue;
            }

            byte[]? hash = null;
            string? error = null;
            try
            {
                hash = await HashAsync(fileStream, chunkSize, (nRead) => statusReportFunc?.Invoke(statusReport with { bytesProcessed = statusReport.bytesProcessed + nRead }), cancellationToken);
                statusReport.bytesProcessed += fileSize;
            }
            catch (Exception ex)
            {
                error = ex.Message;
            }

            if (error != null)
            {
                if (error == string.Empty) error = "Unknown error while hashing file";
                yield return ErrorInputFileInfo(path, fileSize, error);
                statusReport.filesFailed++;
                continue;
            }

            if (hash == null)
            {
                yield return ErrorInputFileInfo(path, fileSize, "Failed to hash file (unknown error)");
                statusReport.filesFailed++;
                continue;
            }

            yield return new InputFileInfo(path, fileSize, hash);
            statusReport.filesProcessed++;
        }

        foreach ((string path, FileStream fileStream) in files) fileStream?.Dispose();

        statusReportFunc?.Invoke(statusReport);
    }
}
