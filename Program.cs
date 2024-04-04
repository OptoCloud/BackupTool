using BackupTool;
using BackupTool.Database;
using BackupTool.Database.Models;
using BackupTool.DTOs;
using BackupTool.Utils;
using Microsoft.Data.Sqlite;
using Microsoft.EntityFrameworkCore;
using System.Diagnostics;
using System.Text;

const int ChunkSize = 128;
const int hashingBlockSize = 4 * 1024 * 1024;
int ParallelTasks = Environment.ProcessorCount;

// Get path to temp folder, and create sqlite database
string tempPath = Path.Combine(Path.GetTempPath(), "BackupTool", Guid.NewGuid().ToString());
string dbPath = Path.Combine(tempPath, "index.db");

// Create temp folder if it doesn't exist
if (!Directory.Exists(tempPath))
{
    Directory.CreateDirectory(tempPath);
}

ulong logLock = 0;

int cursorPos = 0;
uint filesWrittenToDb = 0;
List<int> prevLines = [];

ulong filesTotal = 0;
ulong bytesTotal = 0;
ulong filesAnalyzed = 0;
ulong bytesAnalyzed = 0;

var importer = new Importer();

// importer.ImportFileOrFolder(@"H:\");
importer.ImportFileOrFolder(@"D:\3D Projects");

var archiveWriter = new ArchiveWriter(@"D:\archive.7z", ArchiveWriter.CompressionLevel.Ultra);
if (!await archiveWriter.Start())
{
    Console.WriteLine("Failed to start ArchiveWriter!");
    return;
}

archiveWriter.Progress += () => printStatusReport();

// Create new database
var options = new DbContextOptionsBuilder<DbContext>()
    .UseSqlite($"Data Source={dbPath}")
    .Options;

using (var context = new BTContext(options))
{
    context.Database.EnsureCreated();

    ImportRoot[] importRoots = importer.Roots.ToArray();

    ulong directoryIdCounter = 0;
    Console.WriteLine("Creating directory database entries...");
    context.Directories.AddRange(importRoots.SelectMany(r => r.AllDirectoriesDFS).Select(d =>
    {
        d.Entity = new DirectoryEntity
        {
            Id = ++directoryIdCounter,
            Name = d.Name,
            ParentId = d.Parent?.Id
        };
        return d.Entity;
    }));
    await context.SaveChangesAsync();

    Console.WriteLine("Fetching files...");
    var importFiles = importRoots.SelectMany(r => r.AllFilesBFS).ToArray();
    filesTotal = (ulong)importFiles.Length;
    foreach (var file in importFiles)
    {
        bytesTotal += file.Size;
    }

    Console.WriteLine("Sorting files for better compression ratio...");
    Array.Sort(importFiles, new ImportFileByMimeSorter());

    var blobCache = new Dictionary<string, ulong>();

    ulong blobIdCounter = 0;
    ulong fileIdCounter = 0;
    Console.WriteLine("Starting to process files...");
    cursorPos = Console.CursorTop;
    foreach (var files in importFiles.GroupBy(f => f.Mime))
    {
        foreach (var file in files)
        {
            byte[] hash;

            using (var fs = File.OpenRead(file.FullPathStr))
            {
                // Analayze for for mime type
                if (string.IsNullOrEmpty(file.Mime))
                {
                    file.Mime = FileAnalyzer.GuessMimeByContents(fs);
                }

                fs.Position = 0;

                // Hash the file
                hash = await HashingUtils.HashAsync(fs, hashingBlockSize, printStatusReport);
            }

            filesAnalyzed++;
            bytesAnalyzed += file.Size;

            bool newBlob = false;

            var hashStr = Utils.BytesToHex(hash);
            if (!blobCache.TryGetValue(hashStr, out ulong blobId))
            {
                blobId = ++blobIdCounter;

                context.Blobs.Add(new BlobEntity()
                {
                    Id = blobId,
                    Hash = hash,
                    Size = file.Size,
                });

                blobCache.Add(hashStr, blobId);

                newBlob = true;
            }

            var (name, ext) = PathUtils.SplitFileName(file.Name);

            context.Files.Add(new FileEntity
            {
                Id = ++fileIdCounter,
                Name = name,
                Mime = file.Mime ?? FileAnalyzer.UnkownMimeType,
                Extension = ext,
                BlobId = blobId,
                DirectoryId = file.DirectoryId ?? throw new UnreachableException("DirectoryId should be populated!"),
            });

            filesWrittenToDb++;

            if (file.Size > 0 && newBlob)
            {
                archiveWriter.QueueEntry(file.FullPathStr, $"files/{hashStr[..2]}/{hashStr}", file.Size);
            }

            printStatusReport();
        }

        await context.SaveChangesAsync();
    }

    await context.SaveChangesAsync();
    await context.Database.CloseConnectionAsync();
}
// Stupid fix
SqliteConnection.ClearAllPools();
GC.Collect();
GC.WaitForPendingFinalizers();

var dbSize = new FileInfo(dbPath).Length;
if (dbSize <= 0) throw new UnreachableException("Database should be a file of non-zero size!");

archiveWriter.QueueEntry(dbPath, "index.db", (ulong)dbSize);

await archiveWriter.StopAsync();

printStatusReport();

File.Delete(dbPath);

Console.WriteLine("Done!");

void printStatusReport(ulong fileProgressBytes = 0)
{
    if (Interlocked.CompareExchange(ref logLock, 1, 0) == 1) return;

    string tarTotalBytesFormatted = Utils.FormatNumberByteSize(archiveWriter.TotalBytes);
    string tarWrittenBytesFormatted = Utils.FormatNumberByteSize(archiveWriter.WrittenBytes);
    string bytesTotalFormatted = Utils.FormatNumberByteSize(bytesTotal);
    string bytesAnalyzedFormatted = Utils.FormatNumberByteSize(bytesAnalyzed);
    string averageFileSizeFormatted = Utils.FormatNumberByteSize(bytesTotal / filesTotal);

    printStatus(cursorPos, "Status:",
        $"   {filesAnalyzed} / {filesTotal} files analyzed ({bytesAnalyzedFormatted} / {bytesTotalFormatted})",
        $"   {filesWrittenToDb} / {filesTotal} files written to DB",
        $"   {archiveWriter.WrittenFiles} / {archiveWriter.TotalFiles} blobs written to Tar ({tarWrittenBytesFormatted} / {tarTotalBytesFormatted})",
        $"   Average file size: {averageFileSizeFormatted}"
        );

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

class ImportFileByMimeSorter : IComparer<ImportFileInfo>
{
    public int Compare(ImportFileInfo? x, ImportFileInfo? y)
    {
        if (x == null)
        {
            return y == null ? 0 : 1;
        }
        else if (y == null)
        {
            return -1;
        }

        int mimeCompare = string.Compare(x.Mime, y.Mime);
        if (mimeCompare != 0) return mimeCompare;

        int extCompare = string.Compare(PathUtils.GetExtension(x.Name), PathUtils.GetExtension(y.Name));
        if (extCompare != 0) return extCompare;

        int sizeCompare = x.Size.CompareTo(y.Size);
        if (sizeCompare != 0) return sizeCompare;

        return string.Compare(x.Name, y.Name);
    }
}