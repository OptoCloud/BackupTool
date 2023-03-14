using OptoPacker.Utils;

namespace OptoPacker;

public sealed class ImportFolder : IImportable
{
    public ImportFolder(string path)
    {
        Path = path;
    }

    public string Path { get; }

    public IEnumerable<string> GetFiles()
    {
        return GitignoreParser.GetTrackedFiles(Path);
    }
}
