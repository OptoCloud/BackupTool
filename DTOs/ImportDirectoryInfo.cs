using BackupTool.Database.Models;

namespace BackupTool.DTOs;

public sealed record ImportDirectoryInfo(string Name, ImportDirectoryInfo? Parent)
{
    public DirectoryEntity? Entity { get; set; }
    public ulong? Id => Entity?.Id;

    public List<ImportDirectoryInfo> Children { get; } = [];

    public List<ImportFileInfo> Files { get; } = [];

    public string[] FullPath => Parent is null ? [Name] : [.. Parent.FullPath, Name];
    public string FullPathStr => Path.Combine(FullPath);


    /// <summary>
    /// Retrieves all children recursivley using DFS (Depth First Search)
    /// </summary>
    public IEnumerable<ImportDirectoryInfo> AllChildrenDFS
    {
        get
        {
            foreach (var child in Children)
            {
                yield return child;
                foreach (var nested in child.AllChildrenDFS)
                {
                    yield return nested;
                }
            }
        }
    }

    /// <summary>
    /// Retrieves all children recursivley using BFS (Breadth First Search)
    /// </summary>
    public IEnumerable<ImportDirectoryInfo> AllChildrenBFS
    {
        get
        {
            foreach (var child in Children)
            {
                yield return child;
            }

            foreach (var nested in Children.SelectMany(c => c.AllChildrenBFS))
            {
                yield return nested;
            }
        }
    }


    /// <summary>
    /// Retrieves all directories (children including self) recursivley using DFS (Depth First Search)
    /// </summary>
    public IEnumerable<ImportDirectoryInfo> AllDirectoriesDFS
    {
        get
        {
            yield return this;
            foreach (var nested in AllChildrenDFS)
            {
                yield return nested;
            }
        }
    }

    /// <summary>
    /// Retrieves all directories (children including self) recursivley using BFS (Breadth First Search)
    /// </summary>
    public IEnumerable<ImportDirectoryInfo> AllDirectoriesBFS
    {
        get
        {
            yield return this;
            foreach (var nested in AllChildrenBFS)
            {
                yield return nested;
            }
        }
    }

    /// <summary>
    /// Retrieves all files recursivley using DFS (Depth First Search)
    /// </summary>
    public IEnumerable<ImportFileInfo> AllFilesDFS => AllDirectoriesDFS.SelectMany(x => x.Files);

    /// <summary>
    /// Retrieves all files recursivley using BFS (Breadth First Search)
    /// </summary>
    public IEnumerable<ImportFileInfo> AllFilesBFS => AllDirectoriesBFS.SelectMany(x => x.Files);

    public ImportDirectoryInfo GetOrAddDirectory(string directoryName)
    {
        var child = Children.FirstOrDefault(x => x.Name == directoryName);
        if (child == null)
        {
            child = new ImportDirectoryInfo(directoryName, this);
            Children.Add(child);
        }

        return child;
    }

    public ImportFileInfo GetOrAddFile(string fileName)
    {
        var file = new ImportFileInfo(fileName, this);

        Files.Add(file);

        return file;
    }
}