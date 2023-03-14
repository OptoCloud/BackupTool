namespace OptoPacker;

internal interface IImportable
{
    IEnumerable<string> GetFiles(CancellationToken cancellationToken = default);
}
