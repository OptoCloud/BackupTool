using BackupTool.Utils;

namespace BackupTool.DTOs;

public sealed class ImportRoot
{
    public string BasePath { get; }
    public ImportDirectoryInfo Root { get; }
    public IEnumerable<ImportFileInfo> AllFilesDFS => Root.AllFilesDFS;
    public IEnumerable<ImportFileInfo> AllFilesBFS => Root.AllFilesBFS;
    public IEnumerable<ImportDirectoryInfo> AllDirectoriesDFS => Root.AllDirectoriesDFS;
    public IEnumerable<ImportDirectoryInfo> AllDirectoriesBFS => Root.AllDirectoriesBFS;

    public ImportRoot(string rootPath)
    {
        BasePath = Path.GetFullPath(rootPath);
        Root = new ImportDirectoryInfo(BasePath, null);
    }

    public ImportFileInfo? GetOrAddFile(string filePath)
    {
        if (string.IsNullOrEmpty(filePath) || PathUtils.IsDirectory(filePath)) return null;

        string relativePath = Path.GetRelativePath(BasePath, filePath);
        if (relativePath == filePath) return null; // Paths dont share root

        var pathParts = PathUtils.GetPathParts(relativePath);
        var fileName = pathParts[^1];
        var dirParts = pathParts[..^1];

        var currentDir = Root;
        foreach (var pathPart in dirParts)
        {
            currentDir = currentDir.GetOrAddDirectory(pathPart);
        }

        return currentDir.GetOrAddFile(fileName);
    }
}