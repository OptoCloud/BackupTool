namespace OptoPacker.DTOs;

public record struct MultiFileStatusReport(uint FilesTotal, uint FilesProcessed, uint FilesFailed, ulong BytesTotal, ulong BytesProcessed)
{
    public static MultiFileStatusReport operator +(MultiFileStatusReport a, MultiFileStatusReport b)
    {
        return new MultiFileStatusReport(
            a.FilesTotal + b.FilesTotal,
            a.FilesProcessed + b.FilesProcessed,
            a.FilesFailed + b.FilesFailed,
            a.BytesTotal + b.BytesTotal,
            a.BytesProcessed + b.BytesProcessed
            );
    }
}
