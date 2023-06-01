using Microsoft.Data.Sqlite;
using Microsoft.EntityFrameworkCore;
using OptoPacker;
using OptoPacker.Database;
using OptoPacker.Database.Models;
using OptoPacker.DTOs;
using OptoPacker.Utils;
using System.Reflection;
using System.Text;

int ChunkSize = 128;
int ParallelTasks = Environment.ProcessorCount;
int hashingBlockSize = 4096;

List<IImportable> imports = new List<IImportable>()
{
    //new ImportFolder( @"H:\"),
    new ImportFolder( @"D:\User\Downloads"),
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

int i = 0;
int cursorPos;
Dictionary<string, PathTreeFile> files;
List<int> prevLines = new List<int>();
Dictionary<string, string> hashPathDict = new Dictionary<string, string>();
using (var context = new OptoPackerContext(options))
{
    context.Database.EnsureCreated();

    Console.WriteLine("Gathering files...");
    var importFiles = imports.Select(x => new PathTree(x.BasePath).AddMany(x.GetFiles())).ToArray();

    Console.WriteLine("Creating directory database entries...");
    List<FlattenedPathTreeLevel> levels = new List<FlattenedPathTreeLevel>();
    foreach (var importFile in importFiles)
    {
        await importFile.DbCreateDirectoriesAsync(context);
    }


    Console.WriteLine("Hashing files... (This might take a couple minutes)");
    cursorPos = Console.CursorTop;
    files = importFiles.SelectMany(x => x.GetAllFiles()).ToDictionary(x => x.OriginalPath);
    Dictionary<string, BlobEntity> dbBlobs = new Dictionary<string, BlobEntity>();
    List<(PathTreeFile file, BlobEntity blob)> fileBlobPairs = new List<(PathTreeFile file, BlobEntity blob)>();
    await foreach (ProcessedFileInfo file in ImportProcessor.ProcessFilesAsync(files.Keys.ToArray(), printStatusReport, hashingBlockSize, ChunkSize, ParallelTasks))
    {
        string hash = Utilss.BytesToHex(file.Hash);

        // Get or queue blob
        if (!dbBlobs.TryGetValue(hash, out BlobEntity? blob))
        {
            blob = new BlobEntity()
            {
                Hash = file.Hash,
                Size = file.Size,
            };
            dbBlobs.Add(hash, blob);
            hashPathDict.Add(hash, file.Path);
        }
        fileBlobPairs.Add((files[file.Path], blob));
        i++;
    }

    Console.WriteLine("Writing blobs to database...");
    await context.Blobs.AddRangeAsync(dbBlobs.Values.ToArray());
    await context.SaveChangesAsync();

    Console.WriteLine("Assigning file blobIds...");
    List<FileEntity> dbFiles = new List<FileEntity>();
    foreach (var (file, blob) in fileBlobPairs)
    {
        string name = file.Name;
        int extPos = name.LastIndexOf('.');

        dbFiles.Add(new FileEntity
        {
            BlobId = blob.Id,
            DirectoryId = file.DirectoryId!.Value,
            Name = extPos > 0 ? name[..extPos] : name,
            Extension = extPos > 0 ? name[(extPos + 1)..] : string.Empty
        });
    }

    Console.WriteLine("Writing files to database...");
    await context.Files.AddRangeAsync(dbFiles);
    await context.SaveChangesAsync();
    await context.Database.CloseConnectionAsync();
}
// Stupid fix
SqliteConnection.ClearAllPools();
GC.Collect();
GC.WaitForPendingFinalizers();

string archivePath = "D:\\archive.7z";
if (File.Exists(archivePath))
{
    File.Delete(archivePath);
}

var assemblyDllPath = Path.Combine(Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location)!, "External\\7z.dll");
SevenZip.SevenZipBase.SetLibraryPath(assemblyDllPath);
SevenZip.SevenZipCompressor sevenZipCompressor = new SevenZip.SevenZipCompressor();
sevenZipCompressor.CompressionMethod = SevenZip.CompressionMethod.Lzma2;
sevenZipCompressor.CompressionLevel = SevenZip.CompressionLevel.High;
sevenZipCompressor.CompressionMode = SevenZip.CompressionMode.Create;
sevenZipCompressor.IncludeEmptyDirectories = false;

sevenZipCompressor.CompressFiles(archivePath, hashPathDict.Values.ToArray()); // ERROR: Throws DLL not found exception

// TODO: rename files to hashes inside archive

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