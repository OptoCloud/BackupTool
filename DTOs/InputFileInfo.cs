namespace OptoPacker.DTOs;

public sealed record InputFileInfo(string Path, UInt64 Size, UInt32 BlobId, byte[] Hash);