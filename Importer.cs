using OptoPacker.Utils;

namespace OptoPacker;

internal sealed class Importer
{
    private readonly Dictionary<string, PathTree> _trees = [];

    public IEnumerable<PathTree> Trees => _trees.Values;

    public void ImportFileOrFolder(string path, CancellationToken cancellationToken = default)
    {
        bool isFile = File.Exists(path);
        bool isDirectory = Directory.Exists(path);

        if (!isFile && !isDirectory)
        {
            return;
        }

        var root = Path.GetPathRoot(path);
        if (string.IsNullOrEmpty(root))
        {
            root = Path.PathSeparator.ToString();
        }

        if (!_trees.TryGetValue(root, out var tree))
        {
            tree = new PathTree(root);
            _trees[root] = tree;
        }

        if (isFile)
        {
            tree.GetOrAdd(path);
        }
        else if (isDirectory)
        {
            Console.Write("Importing files: ");
            int numFiles = 0;
            int cursorPos = Console.CursorLeft;
            foreach (var filePath in GitignoreParser.GetTrackedFiles(path, cancellationToken))
            {
                cancellationToken.ThrowIfCancellationRequested();
                tree.GetOrAdd(filePath);
                Console.CursorLeft = cursorPos;
                Console.Write(++numFiles);
            }
            Console.WriteLine("\nDone.");
        }
    }
}
