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
const int hashingBlockSize = 16 * 1024;
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
ulong filesDeeplyAnalyzed = 0;

var importer = new Importer();

// importer.ImportFileOrFolder(@"H:\");
importer.ImportFileOrFolder(@"D:\3D Projects");

var fileCacheManager = new FileCacheManager(256 * 1024 * 1024);

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

    Console.WriteLine("Further analyzing and hashing files...");
    cursorPos = Console.CursorTop;
    foreach (var file in importFiles)
    {
        using var cachedFile = await fileCacheManager.GetCachedFile(file.FullPathStr);
        if (cachedFile == null) continue;

        var stream = cachedFile.Stream;

        // Analayze for for mime type
        if (string.IsNullOrEmpty(file.Mime))
        {
            file.Mime = FileAnalyzer.GuessMimeByContents(stream);
            filesDeeplyAnalyzed++;
        }

        stream.Position = 0;

        // Hash the file
        file.Hash = await HashingUtils.HashAsync(stream, hashingBlockSize, printStatusReport);

        filesAnalyzed++;
        bytesAnalyzed += file.Size;
    }

    Console.WriteLine("Sorting files for better compression ratio...");
    Array.Sort(importFiles, new ImportFileByMimeSorter());

    var blobCache = new Dictionary<string, ulong>();

    ulong blobIdCounter = 0;
    ulong fileIdCounter = 0;
    Console.WriteLine("Writing files to database and archive...");
    foreach (var files in importFiles.GroupBy(f => f.Mime))
    {
        Console.WriteLine($"Writing {files.Key} files...");
        cursorPos = Console.CursorTop;
        foreach (var file in files)
        {
            bool newBlob = false;

            var hash = file.Hash ?? throw new UnreachableException("Hash should be populated!");

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
        $"   Files deeply analyzed: {filesDeeplyAnalyzed}",
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

sealed class ImportFileByMimeSorter : IComparer<ImportFileInfo>
{
    

    private int CompareMime(ImportFileInfo x, ImportFileInfo y)
    {
        var xCats = FileAnalyzer.GetCategories(x.Mime);
        if (xCats.IsEmpty) return 0;

        var yCats = FileAnalyzer.GetCategories(y.Mime);
        if (yCats.IsEmpty) return 0;

        var xIsEncrypted = xCats.Contains(MimeDetective.Storage.Category.Encrypted);
        var yIsEncrypted = yCats.Contains(MimeDetective.Storage.Category.Encrypted);
        if (xIsEncrypted != yIsEncrypted) return xIsEncrypted ? 1 : -1;

        var xIsCompressed = xCats.Contains(MimeDetective.Storage.Category.Compressed);
        var yIsCompressed = yCats.Contains(MimeDetective.Storage.Category.Compressed);
        if (xIsCompressed !=  yIsCompressed) return xIsCompressed ? 1 : -1;

        var xIsExecutable = xCats.Contains(MimeDetective.Storage.Category.Executable);
        var yIsExecutable = yCats.Contains(MimeDetective.Storage.Category.Executable);
        if (xIsExecutable != yIsExecutable) return xIsExecutable ? 1 : -1;

        var xIsVideo = xCats.Contains(MimeDetective.Storage.Category.Video);
        var yIsVideo = yCats.Contains(MimeDetective.Storage.Category.Video);
        if (xIsVideo != yIsVideo) return xIsVideo ? 1 : - 1;

        var xIsImage = xCats.Contains(MimeDetective.Storage.Category.Image);
        var yIsImage = yCats.Contains(MimeDetective.Storage.Category.Image);
        if (xIsImage != yIsImage) return xIsImage ? 1 : -1;

        var xIsAudio = xCats.Contains(MimeDetective.Storage.Category.Audio);
        var yIsAudio = yCats.Contains(MimeDetective.Storage.Category.Audio);
        if (xIsAudio != yIsAudio) return xIsAudio ? 1 : -1;

        var xIsConfiguration = xCats.Contains(MimeDetective.Storage.Category.Configuration);
        var yIsConfiguration = yCats.Contains(MimeDetective.Storage.Category.Configuration);
        if (xIsConfiguration != yIsConfiguration) return xIsConfiguration ? -1 : 1; // Reverse order here, scripts are usually plaintext (very compressible)

        var xIsScript = xCats.Contains(MimeDetective.Storage.Category.Script);
        var yIsScript = yCats.Contains(MimeDetective.Storage.Category.Script);
        if (xIsScript != yIsScript) return xIsScript ? -1 : 1; // Reverse order here, scripts are usually plaintext (very compressible)

        var xIsDocument = xCats.Contains(MimeDetective.Storage.Category.Document);
        var yIsDocument = yCats.Contains(MimeDetective.Storage.Category.Document);
        if (xIsDocument != yIsDocument) return xIsDocument ? 1 : -1;

        return string.Compare(x.Mime, y.Mime);
    }

    public int Compare(ImportFileInfo? x, ImportFileInfo? y)
    {
        if (x == y) return 0;
        if (x == null) return 1;
        if (y == null) return -1;

        int categoryCompare = CompareMime(x, y);
        if (categoryCompare != 0) return categoryCompare;

        int extCompare = string.Compare(PathUtils.GetExtension(x.Name), PathUtils.GetExtension(y.Name));
        if (extCompare != 0) return extCompare;

        return x.Size.CompareTo(y.Size);
    }
}