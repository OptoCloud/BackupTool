using Microsoft.Data.Sqlite;
using Microsoft.EntityFrameworkCore;
using OptoPacker;
using OptoPacker.Database;
using OptoPacker.Database.Models;
using OptoPacker.DTOs;
using OptoPacker.Utils;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.Formats.Tar;
using System.Text;

int ChunkSize = 128;
int ParallelTasks = Environment.ProcessorCount;
int hashingBlockSize = 4 * 1024 * 1024;
int compressionLevel = 9; // 0-9

// Get path to temp folder, and create sqlite database
string tempPath = Path.Combine(Path.GetTempPath(), "OptoPacker", Guid.NewGuid().ToString());
string dbPath = Path.Combine(tempPath, "index.db");
string archivePath = "D:\\archive.7z";

// Create temp folder if it doesn't exist
if (!Directory.Exists(tempPath))
{
    Directory.CreateDirectory(tempPath);
}

var importer = new Importer();

// importer.ImportFileOrFolder(@"H:\");
importer.ImportFileOrFolder(@"D:\3D Projects");

ulong logLock = 0;

int cursorPos = 0;
uint filesWrittenToDb = 0;
List<int> prevLines = [];
Dictionary<string, PathTreeFile> files = [];

var lastSummary = new MultiFileStatusReport();

ulong _tarTotalFiles = 0;
ulong _tarTotalBytes = 0;
ulong _tarWrittenFiles = 0;
ulong _tarWrittenBytes = 0;
var tarWriterBag = new ConcurrentBag<TarFileEntry>();
var tarWriterTask = Task.Run(async () =>
{
    // Start the 7-Zip process
    using var process = new Process();

    process.StartInfo.FileName = "7z"; // Path to 7z executable
    process.StartInfo.Arguments = $"a -t7z \"{archivePath}\" -si\"data.tar\" -mx={compressionLevel} -m0=lzma2 -aoa";
    process.StartInfo.RedirectStandardInput = true;
    process.StartInfo.RedirectStandardOutput = true;
    process.StartInfo.UseShellExecute = false;
    process.Start();

    using (var tarWriter = new TarWriter(process.StandardInput.BaseStream))
    {
        bool run = true;

        while (run)
        {
            while (tarWriterBag.TryTake(out TarFileEntry entry))
            {
                if (entry.Size == 0) continue;

                await tarWriter.WriteEntryAsync(entry.ExternalPath, entry.InternalPath);

                // Status reporting
                Interlocked.Increment(ref _tarWrittenFiles);
                Interlocked.Add(ref _tarWrittenBytes, entry.Size);
                printStatusReport();

                if (entry.ExitAfterWrite)
                {
                    run = false;
                }
            }

            await Task.Delay(100);
        }
    }

    _ = process.StandardOutput.ReadToEnd();
    process.WaitForExit();
});

// Create new database
var options = new DbContextOptionsBuilder<OptoPackerContext>()
    .UseSqlite($"Data Source={dbPath}")
    .Options;

using (var context = new OptoPackerContext(options))
{
    context.Database.EnsureCreated();

    PathTree[] importFiles = importer.Trees.ToArray();

    Console.WriteLine("Creating directory database entries...");
    List<FlattenedPathTreeLevel> levels = [];
    foreach (var importFile in importFiles)
    {
        await importFile.DbCreateDirectoriesAsync(context);
    }

    Dictionary<string, BlobEntity> blobEntityCache = [];

    Console.WriteLine("Hashing files... (This might take a couple minutes)");
    cursorPos = Console.CursorTop;
    files = importFiles.SelectMany(x => x.GetAllFiles()).ToDictionary(x => x.OriginalPath);
    await foreach (ProcessedFileInfo[] fileChunk in ImportProcessor.ProcessFilesAsync([.. files.Keys], printStatusReportU, hashingBlockSize, ChunkSize, ParallelTasks))
    {
        List<(ProcessedFileInfo, string, BlobEntity, bool)> fileBlobs = [];

        // Insert all blobs we can
        foreach (ProcessedFileInfo file in fileChunk)
        {
            bool inserted = false;

            string hash = Utilss.BytesToHex(file.Hash);

            if (!blobEntityCache.TryGetValue(hash, out BlobEntity? blob))
            {
                try
                {
                    blob = new BlobEntity()
                    {
                        Hash = file.Hash,
                        Size = file.Size,
                    };

                    context.Blobs.Add(blob);

                    inserted = true;
                }
                catch (Exception)
                {
                    blob = await context.Blobs.FirstOrDefaultAsync(blob => blob.Hash == file.Hash);
                }

                if (blob is null)
                {
                    Console.WriteLine($"{hash} could not be created or found!");
                    continue;
                }

                blobEntityCache.Add(hash, blob);
            }

            fileBlobs.Add((file, hash, blob, inserted));
        }

        // Save changes
        await context.SaveChangesAsync();

        foreach (var (file, hash, blob, inserted) in fileBlobs)
        {
            var pathTreefile = files[file.Path];
            var (name, ext) = SplitFileName(pathTreefile.Name);

            context.Files.Add(new FileEntity
            {
                BlobId = blob.Id,
                DirectoryId = pathTreefile.DirectoryId!.Value,
                Name = name,
                Extension = ext,
            });

            filesWrittenToDb++;

            if (file.Size > 0 && inserted)
            {
                tarWriterBag.Add(new TarFileEntry(file.Path, $"files/{hash[..2]}/{hash}", file.Size));
                Interlocked.Increment(ref _tarTotalFiles);
                Interlocked.Add(ref _tarTotalBytes, file.Size);
            }
        }

        await context.SaveChangesAsync();

        printStatusReport();
    }

    await context.SaveChangesAsync();
    await context.Database.CloseConnectionAsync();
}
// Stupid fix
SqliteConnection.ClearAllPools();
GC.Collect();
GC.WaitForPendingFinalizers();

var dbSize = new FileInfo(dbPath).Length;
if (dbSize <= 0) throw new Exception("Wtf?");

tarWriterBag.Add(new TarFileEntry(dbPath, "index.db", (ulong)dbSize, true));
Interlocked.Increment(ref _tarTotalFiles);
Interlocked.Add(ref _tarTotalBytes, (ulong)dbSize);

await tarWriterTask;

File.Delete(dbPath);

Console.WriteLine("Done!");

void printStatusReportInner()
{
    ulong tarTotalFiles = Interlocked.Read(ref _tarTotalFiles);
    ulong tarTotalBytes = Interlocked.Read(ref _tarTotalBytes);
    ulong tarWrittenFiles = Interlocked.Read(ref _tarWrittenFiles);
    ulong tarWrittenBytes = Interlocked.Read(ref _tarWrittenBytes);

    string tarTotalBytesFormatted = Utilss.FormatNumberByteSize(tarTotalBytes);
    string tarWrittenBytesFormatted = Utilss.FormatNumberByteSize(tarWrittenBytes);
    string bytesTotalFormatted = Utilss.FormatNumberByteSize(lastSummary.BytesTotal);
    string bytesHashedFormatted = Utilss.FormatNumberByteSize(lastSummary.BytesProcessed);
    string averageFileSizeFormatted = Utilss.FormatNumberByteSize(lastSummary.BytesTotal / lastSummary.FilesTotal);

    printStatus(cursorPos, "Status:",
        $"   {lastSummary.FilesProcessed} / {files.Count} files hashed ({bytesHashedFormatted} / {bytesTotalFormatted})",
        $"   {filesWrittenToDb} / {files.Count} files written to DB",
        $"   {tarWrittenFiles} / {tarTotalFiles} blobs written to Tar ({tarWrittenBytesFormatted} / {tarTotalBytesFormatted})",
        $"   Average file size: {averageFileSizeFormatted}"
        );
}
void printStatusReport()
{
    if (Interlocked.CompareExchange(ref logLock, 1, 0) == 1) return;

    printStatusReportInner();

    Interlocked.Exchange(ref logLock, 0);
}
void printStatusReportU(MultiFileStatusReport summary)
{
    if (Interlocked.CompareExchange(ref logLock, 1, 0) == 1) return;

    lastSummary = summary;
    printStatusReportInner();

    Interlocked.Exchange(ref logLock, 0);
}
void printStatus(int basePos, string title, params string[] lines)
{
    for (int i = prevLines.Count; i < lines.Length + 1; i++) prevLines.Add(0);

    Console.CursorTop = basePos;
    Console.CursorLeft = 0;

    var sb = new StringBuilder();

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
(string name, string extension) SplitFileName(string fileName)
{
    int idx = fileName.LastIndexOf('.');

    if (idx <= 0) return (fileName, String.Empty);

    return (fileName[..idx], fileName[(idx + 1)..]);
}

record struct TarFileEntry(string ExternalPath, string InternalPath, ulong Size, bool ExitAfterWrite = false);

sealed record FlattenedPathTreeLevel(List<FlattenedPathTreeChunk> Chunks);
sealed record FlattenedPathTreeChunk(ulong? ParentId, string? ParentName, string[] ChildrenNames);