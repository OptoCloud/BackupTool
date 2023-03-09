using static System.Reflection.Metadata.BlobBuilder;

namespace OptoPacker.DTOs;

public sealed record DirectoryEntry(string Name, List<FileEntry> Files, List<DirectoryEntry> Directories);