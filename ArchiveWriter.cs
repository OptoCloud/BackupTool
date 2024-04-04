using System.Collections.Concurrent;
using System.Diagnostics;
using System.Formats.Tar;

namespace BackupTool;

internal sealed class ArchiveWriter
{
    private sealed record TarFileEntry(string ExternalPath, string InternalPath, ulong Size);

    public enum CompressionLevel
    {
        Store = 0,
        Fastest = 1,
        Fast = 3,
        Normal = 5,
        Maximum = 7,
        Ultra = 9
    }
    public delegate void ProgressCallback();

    private readonly string _archivePath;
    private readonly CompressionLevel _compressionLevel;
    private readonly ConcurrentBag<TarFileEntry> _writerBag = [];

    private bool _run = false;
    private Task? _task = null;

    private ulong _totalFiles = 0;
    private ulong _totalBytes = 0;
    private ulong _writtenFiles = 0;
    private ulong _writtenBytes = 0;

    public ulong TotalFiles => Interlocked.Read(ref _totalFiles);
    public ulong TotalBytes => Interlocked.Read(ref _totalBytes);
    public ulong WrittenFiles => Interlocked.Read(ref _writtenFiles);
    public ulong WrittenBytes => Interlocked.Read(ref _writtenBytes);

    public event ProgressCallback Progress = delegate { };

    public ArchiveWriter(string archivePath, CompressionLevel compressionLevel)
    {
        _archivePath = archivePath;
        _compressionLevel = compressionLevel;
    }

    private async Task WriterTask()
    {
        using var process = new Process();

        process.StartInfo.FileName = "7z"; // Path to 7z executable
        process.StartInfo.Arguments = $"a -t7z \"{_archivePath}\" -si\"data.tar\" -mx={(int)_compressionLevel} -m0=lzma2 -aoa";
        process.StartInfo.RedirectStandardInput = true;
        process.StartInfo.RedirectStandardOutput = true;
        process.StartInfo.UseShellExecute = false;
        process.Start();

        using (var tarWriter = new TarWriter(process.StandardInput.BaseStream))
        {
            while (_run || WrittenFiles < TotalFiles)
            {
                while (_writerBag.TryTake(out TarFileEntry? entry))
                {
                    if (entry.Size == 0) continue;

                    await tarWriter.WriteEntryAsync(entry.ExternalPath, entry.InternalPath);

                    Interlocked.Increment(ref _writtenFiles);
                    Interlocked.Add(ref _writtenBytes, entry.Size);
                }

                await Task.Delay(100);
            }
        }

        _ = process.StandardOutput.ReadToEnd();
        process.WaitForExit();
    }

    public bool Start()
    {
        if (_run || _task != null) return false;

        _run = true;
        _task = Task.Run(WriterTask);

        return true;
    }

    public async Task StopAsync()
    {
        _run = false;
        if (_task != null)
        {
            await _task;
            _task = null;
        }
    }

    public void QueueEntry(string path, string internalPath, ulong size)
    {
        if (size == 0) return;

        _writerBag.Add(new TarFileEntry(path, internalPath, size));
        Interlocked.Increment(ref _totalFiles);
        Interlocked.Add(ref _totalBytes, size);
    }
}
