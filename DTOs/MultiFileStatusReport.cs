namespace OptoPacker.DTOs;

public record struct MultiFileStatusReport(uint filesTotal, uint filesProcessed, uint filesFailed, ulong bytesTotal, ulong bytesProcessed)
{
    public static MultiFileStatusReport operator +(MultiFileStatusReport a, MultiFileStatusReport b)
    {
        return new MultiFileStatusReport(
            a.filesTotal + b.filesTotal,
            a.filesProcessed + b.filesProcessed,
            a.filesFailed + b.filesFailed,
            a.bytesTotal + b.bytesTotal,
            a.bytesProcessed + b.bytesProcessed
            );
    }
}
