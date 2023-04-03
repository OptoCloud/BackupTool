using OptoPacker.Utils;

namespace OptoPacker;

public sealed class ImportFolder : IImportable
{
    public ImportFolder(string path)
    {
        if (!Directory.Exists(path))
        {
            throw new DirectoryNotFoundException("Directory not found");
        }

        BasePath = path;
    }

    public string BasePath { get; }

    public IEnumerable<string> GetFiles(CancellationToken cancellationToken)
    {
        return GitignoreParser.GetTrackedFiles(BasePath, cancellationToken);
    }
}
