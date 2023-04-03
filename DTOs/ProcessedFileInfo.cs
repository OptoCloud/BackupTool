namespace OptoPacker.DTOs;

public record struct ProcessedFileInfo(string Path, ulong Size, byte[] Hash, string? Error = null);