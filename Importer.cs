using BackupTool.DTOs;
using BackupTool.Utils;
using System.Diagnostics;

namespace BackupTool;

internal sealed class Importer
{
    private readonly Dictionary<string, ImportRoot> _roots = [];
    public IEnumerable<ImportRoot> Roots => _roots.Values;

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

        if (!_roots.TryGetValue(root, out var tree))
        {
            tree = new ImportRoot(root);
            _roots[root] = tree;
        }

        if (isFile)
        {
            tree.GetOrAddFile(path);
        }
        else if (isDirectory)
        {
            Console.Write("Importing files: ");
            int cursorPos = Console.CursorLeft;

            int numFiles = 0;
            foreach (var filePath in GitignoreParser.GetTrackedFiles(path, cancellationToken))
            {
                cancellationToken.ThrowIfCancellationRequested();

                var file = tree.GetOrAddFile(filePath);
                if (file is null) throw new InvalidOperationException("File path is not a file");

                var fileSize = new FileInfo(filePath).Length;
                if (fileSize < 0) throw new UnreachableException("File cant have a negative size!");

                file.Size = (ulong)fileSize;
                file.Mime = FileAnalyzer.GuessMimeByExtension(filePath);

                Console.CursorLeft = cursorPos;
                Console.Write(++numFiles);
            }

            Console.WriteLine();
        }
    }
}
