using Microsoft.Data.Sqlite;
using Microsoft.EntityFrameworkCore;
using OptoPacker;
using OptoPacker.Database;
using OptoPacker.Database.Models;
using OptoPacker.DTOs;
using OptoPacker.Utils;
using System.Diagnostics;
using System.Formats.Tar;
using System.Text;

int ChunkSize = 128;
int ParallelTasks = Environment.ProcessorCount;
int hashingBlockSize = 4096;

List<IImportable> imports = new List<IImportable>()
{
    //new ImportFolder( @"H:\"),
    new ImportFolder( @"D:\3D Projects"),
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

string archivePath = "D:\\archive.zip";
// Start the 7-Zip process
using (Process process = new Process())
{
    process.StartInfo.FileName = "7z"; // Path to 7z executable
    process.StartInfo.Arguments = $"a -t7z \"{archivePath}\" -si\"data.tar\" -mx=9 -m0=lzma2 -aoa";
    process.StartInfo.RedirectStandardInput = true;
    process.StartInfo.UseShellExecute = false;
    process.Start();

    using (TarWriter tarWriter = new TarWriter(process.StandardInput.BaseStream))
    {
        // Iterate over the hashPathDict and add entries to the tar archive
        foreach (var item in hashPathDict)
        {
            string hash = item.Key;
            string filePath = item.Value;
            tarWriter.WriteEntry(filePath, $"files/{hash[..2]}/{hash}");
        }

        // Add the database to the archive
        tarWriter.WriteEntry(dbPath, "index.db");
    }

    process.WaitForExit();
}
File.Delete(dbPath);

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