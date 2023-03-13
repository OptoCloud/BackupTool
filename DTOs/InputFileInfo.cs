namespace OptoPacker.DTOs;

public record struct InputFileInfo(string Path, ulong Size, byte[] Hash, string? Error = null);