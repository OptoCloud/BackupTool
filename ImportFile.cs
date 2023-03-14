namespace OptoPacker;

public sealed class ImportFile : IImportable
{
    public ImportFile(string path)
    {
        Path = path;
    }

    public string Path { get; }

    public IEnumerable<string> GetFiles(CancellationToken cancellationToken)
    {
        yield return Path;
    }
}
