using Microsoft.EntityFrameworkCore;
using OptoPacker;
using OptoPacker.Database;
using OptoPacker.DTOs;
using OptoPacker.Utils;
using System.Text;

int ChunkSize = 128;
int ParallelTasks = Environment.ProcessorCount;
int PrintIntervalMs = 100;
int hashingBlockSize = 4096;

List<IImportable> imports = new List<IImportable>()
{
    new ImportFolder( @"H:\"),
    new ImportFile( @"D:\Piracy\finished torrents\Операция «Мертвый снег» 2 (2014) BDRip 745.avi"),
};

// Get path to temp folder, and create sqlite database
string tempPath = Path.GetTempPath();
string dbPath = Path.Combine(tempPath, "OptoPacker.db");
// Delete old database if it exists
if (File.Exists(dbPath))
{
    File.Delete(dbPath);
}
// Create new database
var options = new DbContextOptionsBuilder<OptoPackerContext>()
    .UseSqlite($"Data Source={dbPath}")
    .Options;
using var context = new OptoPackerContext(options);
context.Database.EnsureCreated();

Console.WriteLine("Gathering files...");
var importFiles = imports.Select(x => new PathTree(x.BasePath).AddMany(x.GetFiles())).ToArray();

Console.WriteLine("Creating directory database entries...");
List<FlattenedPathTreeLevel> levels = new List<FlattenedPathTreeLevel>();
foreach (var importFile in importFiles)
{
    importFile.DbCreateDirectories(context);
}

int i = 0;
Console.WriteLine("Hashing files... (This might take a couple minutes)");
int cursorPos = Console.CursorTop;
List<int> prevLines = new List<int>();
var files = importFiles.SelectMany(x => x.GetAllFiles()).ToDictionary(x => x.OriginalPath);
await foreach (ProcessedFileInfo file in ImportProcessor.ProcessFiles(files.Keys.ToArray(), printStatusReport, hashingBlockSize, ChunkSize, ParallelTasks))
{
    files[file.Path].Size = file.Size;
    files[file.Path].Hash = file.Hash;
    i++;
}

void printStatusReport(MultiFileStatusReport summary)
{
    printStatus(cursorPos, "Status:",
        $"   Processed {summary.filesProcessed} / {files.Count} Files",
        $"   Processed {Utilss.FormatNumberByteSize(summary.bytesProcessed)} / {Utilss.FormatNumberByteSize(summary.bytesTotal)} Bytes",
        $"   Average files size: {Utilss.FormatNumberByteSize(summary.bytesTotal / summary.filesTotal)}"
        );
}
void printStatus(int basePos, string title, params string[] lines)
{
    for (int i = prevLines.Count; i < lines.Length + 1; i++) prevLines.Add(0);

    Console.CursorTop = basePos;
    Console.CursorLeft = 0;

    StringBuilder sb = new StringBuilder();

    if (prevLines[0] > title.Length) title = title.PadRight(prevLines[0]);
    sb.AppendLine(title);
    prevLines[0] = title.Length;

    for (int i = 0; i < lines.Length; i++)
    {
        string line = lines[i];
        if (prevLines[i + 1] > line.Length) line = line.PadRight(prevLines[i + 1]);
        sb.AppendLine(line);
        prevLines[i + 1] = line.Length;
    }

    Console.Write(sb.ToString());
}

Console.WriteLine($"Done, processed {i} / {files.Count} files");

sealed record FlattenedPathTreeLevel(List<FlattenedPathTreeChunk> Chunks);
sealed record FlattenedPathTreeChunk(ulong? parentId, string? parentName, string[] childrenNames);