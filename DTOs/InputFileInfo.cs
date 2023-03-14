namespace OptoPacker.DTOs;

public record struct InputFileInfo(string Path, ulong Size, byte[] Hash, string? Error = null)
{
    private string? _hashString;
    public string HashString => _hashString ??= BitConverter.ToString(Hash).Replace("-", "").ToLowerInvariant();
}