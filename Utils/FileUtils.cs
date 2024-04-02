using OptoPacker.Delegates;
using OptoPacker.DTOs;
using System.Runtime.CompilerServices;
using System.Security.Cryptography;
using System;

namespace OptoPacker.Utils;

public static class FileUtils
{
    const int ReportBytesInterval = 1024 * 1024 * 10; // 10 MB
    

    private static async Task<byte[]?> HashAsync(FileStream fileStream, SingleFileStatusReportFunc statusReportFunc, int hashingBlockSize, CancellationToken cancellationToken = default)
    {
        if (!fileStream.CanRead) return null;

        byte[] buffer = new byte[hashingBlockSize];
        using SHA256 sha256 = SHA256.Create();

        int bytesReadChunk;
        ulong bytesReadTotal = 0;
        ulong lastReportBytesRead = 0;
        do
        {
            bytesReadChunk = await fileStream.ReadAsync(buffer, cancellationToken);
            if (bytesReadChunk > 0)
            {
                _ = sha256.TransformBlock(buffer, 0, bytesReadChunk, buffer, 0);

                bytesReadTotal += (ulong)bytesReadChunk;
                if (bytesReadTotal - lastReportBytesRead >= ReportBytesInterval)
                {
                    statusReportFunc.Invoke(bytesReadTotal);
                    lastReportBytesRead = bytesReadTotal;
                }
            }
        } while (bytesReadChunk > 0);

        sha256.TransformFinalBlock(buffer, 0, 0);

        return sha256.Hash;
    }
    public static async IAsyncEnumerable<ProcessedFileInfo> HashAllAsync(string[] filePaths, MultiFileStatusReportFunc statusReportFunc, int hashingBlockSize, [EnumeratorCancellation] CancellationToken cancellationToken = default)
    {
        var statusReport = new MultiFileStatusReport((uint)filePaths.Length, 0, 0, 0, 0);
        void subStatusReportFunc(ulong bytesRead)
        {
            statusReportFunc.Invoke(statusReport with
            {
                BytesProcessed = statusReport.BytesProcessed + bytesRead
            });
        }

        List<(string path, FileStream fileStream)> files = [];
        foreach (string path in filePaths)
        {
            string? error = null;
            FileStream? fileStream = null;
            try
            {
                fileStream = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read, hashingBlockSize, true);
                if (fileStream.Length < 0)
                {
                    fileStream?.Dispose();
                    error = "Failed to get stream length";
                }
            }
            catch (IOException ex)
            {
                error = ex.HResult switch
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

            if (fileStream == null)
            {
                yield return ProcessedFileInfo.ErrorInfo(path, 0, error ?? "Unkown error while opening file");
                statusReport.FilesFailed++;
                continue;
            }

            files.Add((path, fileStream));
        }

        statusReport.BytesTotal = (ulong)files.Sum(f => f.fileStream.Length);

        foreach ((string path, FileStream fileStream) in files)
        {
            cancellationToken.ThrowIfCancellationRequested();

            ulong fileSize = (ulong)fileStream.Length;

            if (fileSize == 0)
            {
                yield return ProcessedFileInfo.EmptyInfo(path);
                statusReport.FilesProcessed++;
                continue;
            }

            byte[]? hash = null;
            string? error = null;
            try
            {
                hash = await HashAsync(fileStream, subStatusReportFunc, hashingBlockSize, cancellationToken);
                statusReport.BytesProcessed += fileSize;
            }
            catch (Exception ex)
            {
                error = ex.Message;
            }

            if (error != null)
            {
                if (error == string.Empty) error = "Unknown error while hashing file";
                yield return ProcessedFileInfo.ErrorInfo(path, fileSize, error);
                statusReport.FilesFailed++;
                continue;
            }

            if (hash == null)
            {
                yield return ProcessedFileInfo.ErrorInfo(path, fileSize, "Failed to hash file (unknown error)");
                statusReport.FilesFailed++;
                continue;
            }

            yield return new ProcessedFileInfo(path, fileSize, hash);
            statusReport.FilesProcessed++;
        }

        foreach ((string path, FileStream fileStream) in files) fileStream?.Dispose();

        statusReportFunc?.Invoke(statusReport);
    }
}
