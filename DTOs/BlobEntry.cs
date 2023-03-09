namespace OptoPacker.DTOs;

public sealed record BlobEntry(uint Id, ulong Size, byte[] Hash);
