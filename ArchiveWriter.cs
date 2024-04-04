using BackupTool.Utils;
using System.Collections.Concurrent;
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
        if (File.Exists(_archivePath))
        {
            File.Delete(_archivePath);
        }

        using var process = ProcessUtils.StartProcess("7z", $"a -t7z \"{_archivePath}\" -si\"data.tar\" -mx={(int)_compressionLevel} -m0=lzma2 -aoa");

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

        Console.WriteLine(await process.StandardOutput.ReadToEndAsync());
        await process.WaitForExitAsync();
    }

    private async Task<string?> Get7zVersion()
    {
        using var p7z = ProcessUtils.StartProcessOrNull("7z");
        if (p7z == null) return null;

        await p7z.WaitForExitAsync();

        while (true)
        {
            string? line = await p7z.StandardOutput.ReadLineAsync();
            if (line == null) break;

            var parts = line.Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
            if (parts.Length == 0) continue;

            if (parts[0] == "7-Zip" && parts.Contains("Copyright"))
            {
                if (parts.Length >= 2)
                {
                    return parts[1];
                }
            }
        }

        return null;
    }

    public async Task<bool> Start()
    {
        if (_run || _task != null) return false;

        string? version = await Get7zVersion();
        if (version == null)
        {
            await File.WriteAllBytesAsync("7z.exe", Properties.Resources._7z_EXE);
            await File.WriteAllBytesAsync("7z.dll", Properties.Resources._7z_DLL);
            version = await Get7zVersion();
        }

        if (version == null)
        {
            Console.WriteLine("Unable to launch 7-Zip!");
            return false;
        }

        Console.WriteLine($"Starting 7-Zip {version}");

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
