using OptoPacker.Database;
using OptoPacker.Database.Models;
using OptoPacker.DTOs;

namespace OptoPacker.Utils;

public sealed class PathTree
{
    public PathTreeDirectory Root { get; }
    public string BasePath { get; }

    public PathTree(string rootPath)
    {
        BasePath = rootPath.Replace('\\', '/');
        Root = new PathTreeDirectory(BasePath, null);
    }

    public PathTreeDirectory GetOrAdd(string filePath)
    {
        string originalPath = filePath;
        filePath = filePath.Replace('\\', '/');
        
        // Remove the root path from the file path if it matches
        if (filePath.StartsWith(BasePath))
        {
            filePath = filePath[BasePath.Length..];
        }

        var pathParts = filePath.Split('/', StringSplitOptions.RemoveEmptyEntries);
        var fileName = pathParts[^1];

        var currentNode = Root;
        foreach (var pathPart in pathParts[..^1])
        {
            currentNode = currentNode.GetOrAddChild(pathPart);
        }

        currentNode.GetOrAddFile(fileName, originalPath);

        return currentNode;
    }

    public Task DbCreateDirectoriesAsync(OptoPackerContext dbCtx, CancellationToken cancellationToken = default)
    {
        return DbCreateDirectoriesAsync(dbCtx, [Root], 0, cancellationToken);
    }
    private static async Task DbCreateDirectoriesAsync(OptoPackerContext dbCtx, PathTreeDirectory[] nodes, int depth, CancellationToken cancellationToken)
    {
        List<PathTreeDirectory> nextNodes = [];
        
        // Shallow first, add all directories, then save changes
        foreach (var node in nodes)
        {
            nextNodes.AddRange(node.Children);
            await dbCtx.Directories.AddAsync(new DirectoryEntity { Name = node.Name, ParentId = node.ParentId }, cancellationToken);
        }

        await dbCtx.SaveChangesAsync(cancellationToken);

        // Now that we have the IDs, we can update the parent IDs
        foreach (var node in nodes)
        {
            var dbNode = dbCtx.Directories.Single(x => x.Name == node.Name && x.ParentId == node.ParentId);
            node.Id = dbNode.Id;
        }

        // Recurse
        if (nextNodes.Count > 0)
        {
            await DbCreateDirectoriesAsync(dbCtx, [.. nextNodes], depth + 1, cancellationToken);
        }
    }

    public IEnumerable<PathTreeFile> GetAllFiles()
    {
        return GetAllFiles(Root);
    }
    private static IEnumerable<PathTreeFile> GetAllFiles(PathTreeDirectory node)
    {
        foreach (var file in node.Files)
        {
            yield return file;
        }

        foreach (var child in node.Children)
        {
            foreach (var file in GetAllFiles(child))
            {
                yield return file;
            }
        }
    }

    public PathTreeDirectory this[string filePath] => GetOrAdd(filePath);
}

public sealed class PathTreeDirectory(string name, PathTreeDirectory? parent)
{
    public ulong? Id { get; set; } = null;
    public string Name { get; } = name;
    public PathTreeDirectory? Parent { get; } = parent;
    public ulong? ParentId => Parent?.Id;
    public List<PathTreeDirectory> Children { get; } = [];
    public List<PathTreeFile> Files { get; } = [];

    public PathTreeDirectory GetOrAddChild(string name)
    {
        var child = Children.FirstOrDefault(x => x.Name == name);
        if (child == null)
        {
            child = new PathTreeDirectory(name, this);
            Children.Add(child);
        }

        return child;
    }

    public void GetOrAddFile(string filePath, string originalPath)
    {
        Files.Add(new PathTreeFile(filePath, originalPath, this));
    }
}
public sealed class PathTreeFile(string name, string originalPath, PathTreeDirectory directory)
{
    public ulong? Id { get; set; } = null;
    public string Name { get; } = name;
    public string OriginalPath { get; } = originalPath;
    public PathTreeDirectory Directory { get; } = directory;
    public ulong? DirectoryId => Directory.Id;
    public ulong Size { get; set; } = 0;
    public byte[] Hash { get; set; } = [];
}