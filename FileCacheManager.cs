using System.Buffers;
using System.Diagnostics;

namespace BackupTool;

internal sealed record FileCacheManager
{
    private const int MaxArrayPoolSize = 64 * 1024 * 1024; // 64MB

    private readonly int _ramMaxLoad;
    private readonly string? _cacheDrive;
    private readonly ulong _cacheDriveMaxLoad;

    public FileCacheManager(int ramMaxLoad, string? cacheDrivePath = null, ulong cacheDriveMaxLoad = 0)
    {
        if (ramMaxLoad < 0) throw new ArgumentOutOfRangeException("Data values cannot be negative!", nameof(ramMaxLoad));

        _ramMaxLoad = ramMaxLoad;
        _cacheDrive = _cacheDrive == null ? null : Path.Combine(_cacheDrive, "temp");
        _cacheDriveMaxLoad = cacheDriveMaxLoad;
    }

    public enum CacheType
    {
        None,
        Ram,
        Disk
    }

    public interface ICachedFile : IDisposable
    {
        public CacheType CacheType { get; }
        public ulong Size { get; }
        public Stream Stream { get; }
    }

    private sealed class RamFile : ICachedFile
    {
        private readonly byte[] _byteArray;
        private readonly MemoryStream _memoryStream;

        public RamFile(byte[] fileBytes, int fileSize)
        {
            _byteArray = fileBytes;
            _memoryStream = new MemoryStream(_byteArray, false);
            Size = (ulong)fileSize;
        }

        public CacheType CacheType => CacheType.Ram;
        public ulong Size { get; }
        public Stream Stream => _memoryStream;

        bool _disposed = false;
        public void Dispose()
        {
            if (_disposed) return;
            _disposed = true;

            try
            {
                _memoryStream?.Dispose();
            }
            finally
            {
                if (_byteArray.Length < MaxArrayPoolSize)
                {
                    ArrayPool<byte>.Shared.Return(_byteArray);
                }
            }
        }
    }

    private sealed class CacheDiskFile : ICachedFile
    {
        private readonly string _filePath;
        private readonly BufferedStream _stream;

        public CacheDiskFile(string filePath, ulong fileSize)
        {
            _filePath = filePath;
            _stream = new BufferedStream(File.OpenRead(filePath));
            Size = fileSize;
        }

        public Stream Stream => _stream;
        public ulong Size { get; }
        public CacheType CacheType => CacheType.Disk;

        bool _disposed = false;
        public void Dispose()
        {
            if (_disposed) return;
            _disposed = true;

            try
            {
                _stream?.Dispose();
            }
            finally
            {
                File.Delete(_filePath);
            }
        }
    }

    private sealed class DiskFile : ICachedFile
    {
        private readonly BufferedStream _stream;

        public DiskFile(string filePath, ulong fileSize)
        {
            _stream = new BufferedStream(File.OpenRead(filePath));
            Size = fileSize;
        }

        public Stream Stream => _stream;
        public ulong Size { get; }
        public CacheType CacheType => CacheType.None;

        bool _disposed = false;
        public void Dispose()
        {
            if (_disposed) return;
            _disposed = true;

            _stream?.Dispose();
        }
    }

    public async Task<ICachedFile?> GetCachedFile(string path, CancellationToken cancellationToken = default)
    {
        if (string.IsNullOrEmpty(path) || !File.Exists(path)) return null;

        ulong fileSize;
        {
            FileInfo fileInfo = new FileInfo(path);
            if (fileInfo.Length < 0) throw new UnreachableException("File sizes can't be negative!");

            fileSize = (ulong)fileInfo.Length;
        }

        if (fileSize <= (ulong)_ramMaxLoad)
        {
            using var fileStream = File.OpenRead(path);

            int fileSizeInt = (int)fileSize; // This is ok since _ramMaxLoad cannot be more than int.MaxValue

            byte[] bytes = fileSizeInt > MaxArrayPoolSize ? new byte[fileSizeInt] : ArrayPool<byte>.Shared.Rent(fileSizeInt);

            int nRead = 0;
            while (nRead < fileSizeInt)
            {
                int retval = await fileStream.ReadAsync(bytes.AsMemory(nRead, fileSizeInt - nRead), cancellationToken);
                if (retval < 0) throw new Exception("Failed to read file!");

                nRead += retval;
            }

            return new RamFile(bytes, fileSizeInt);
        }

        if (fileSize <= _cacheDriveMaxLoad && !string.IsNullOrEmpty(_cacheDrive))
        {
            string tempPath = Path.Combine(_cacheDrive, Guid.NewGuid().ToString());

            bool ok = false;
            try
            {
                File.Copy(tempPath, path, true);
                var file = new CacheDiskFile(tempPath, fileSize);
                ok = true;
                return file;
            }
            finally
            {
                if (!ok && File.Exists(tempPath))
                {
                    File.Delete(tempPath);
                }
            }

        }

        return new DiskFile(path, fileSize);
    }
}
