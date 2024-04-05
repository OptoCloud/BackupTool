using BackupTool.Database.Models;

namespace BackupTool.DTOs;

public sealed record ImportFileInfo(string Name, ImportDirectoryInfo Directory)
{
    public ulong Size { get; set; } = 0;

    public string? Mime { get; set; }
    public byte[]? Hash { get; set; }

    public BlobEntity? BlobEntity { get; set; }
    public ulong? BlobId => BlobEntity?.Id;

    public FileEntity? Entity { get; set; }
    public ulong? Id => Entity?.Id;

    public ulong? DirectoryId => Directory?.Id;

    public string[] FullPath => [.. Directory.FullPath, Name];
    public string FullPathStr => Path.Combine(FullPath);
}