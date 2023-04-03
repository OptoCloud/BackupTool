namespace OptoPacker;

public sealed class ImportFile : IImportable
{
    public ImportFile(string path)
    {
        if (!File.Exists(path))
        {
            throw new FileNotFoundException("File not found", path);
        }

        FileName = Path.GetFileName(path);
        BasePath = Path.GetDirectoryName(path)!;
    }

    public string FileName { get; }
    public string BasePath { get; }

    public IEnumerable<string> GetFiles(CancellationToken cancellationToken)
    {
        yield return FileName;
    }
}
