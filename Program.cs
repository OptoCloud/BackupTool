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
ulong filesWrittenToTar = 0;
ulong bytesWrittenToTar = 0;
List<int> prevLines = [];
Dictionary<string, PathTreeFile> files = [];

var lastSummary = new MultiFileStatusReport();

// Start the 7-Zip process
using (var process = new Process())
{
    process.StartInfo.FileName = "7z"; // Path to 7z executable
    process.StartInfo.Arguments = $"a -t7z \"{archivePath}\" -si\"data.tar\" -mx={compressionLevel} -m0=lzma2 -aoa";
    process.StartInfo.RedirectStandardInput = true;
    process.StartInfo.RedirectStandardOutput = true;
    process.StartInfo.UseShellExecute = false;
    process.Start();

    using (var tarWriter = new TarWriter(process.StandardInput.BaseStream))
    {
        // Delete old database if it exists
        if (File.Exists(dbPath))
        {
            File.Delete(dbPath);
        }
        // Create new database
        var options = new DbContextOptionsBuilder<OptoPackerContext>()
            .UseSqlite($"Data Source={dbPath}")
            .Options;

        bool importingFiles = true;
        var tarWriterQueue = new ConcurrentQueue<(string, byte[], ulong)>();
        var tarWritingTask = Task.Run(async () =>
        {
            while (importingFiles)
            {
                while (tarWriterQueue.TryDequeue(out (string path, byte[] hash, ulong size) entry))
                {
                    if (entry.size == 0) continue;

                    string hash = Utilss.BytesToHex(entry.hash);
                    await tarWriter.WriteEntryAsync(entry.path, $"files/{hash[..2]}/{hash}");
                    Interlocked.Increment(ref filesWrittenToTar);
                    Interlocked.Add(ref bytesWrittenToTar, entry.size);

                    printStatusReport();
                }

                await Task.Delay(100);
            }
        });

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

            Dictionary<string, ulong> blobIdCache = [];

            Console.WriteLine("Hashing files... (This might take a couple minutes)");
            cursorPos = Console.CursorTop;
            files = importFiles.SelectMany(x => x.GetAllFiles()).ToDictionary(x => x.OriginalPath);
            await foreach (ProcessedFileInfo file in ImportProcessor.ProcessFilesAsync([.. files.Keys], printStatusReportU, hashingBlockSize, ChunkSize, ParallelTasks))
            {
                if (file.Size > 0) {
                    tarWriterQueue.Enqueue((file.Path, file.Hash, file.Size));
                }

                string strHash = Convert.ToBase64String(file.Hash);
                if (!blobIdCache.TryGetValue(strHash, out ulong blobId))
                {
                    BlobEntity? blob;
                    try
                    {
                        blob = new BlobEntity()
                        {
                            Hash = file.Hash,
                            Size = file.Size,
                        };
                        await context.Blobs.AddAsync(blob);
                        await context.SaveChangesAsync();
                    }
                    catch (Exception)
                    {
                        blob = await context.Blobs.FirstOrDefaultAsync(blob => blob.Hash == file.Hash);
                    }

                    if (blob == null) throw new Exception("Blob could not be created or found");

                    blobId = blob.Id;
                    blobIdCache[strHash] = blobId;
                }

                var pathTreefile = files[file.Path];
                string name = pathTreefile.Name;
                int extPos = name.LastIndexOf('.');

                await context.Files.AddAsync(new FileEntity
                {
                    BlobId = blobId,
                    DirectoryId = pathTreefile.DirectoryId!.Value,
                    Name = extPos > 0 ? name[..extPos] : name,
                    Extension = extPos > 0 ? name[(extPos + 1)..] : string.Empty,
                });
                await context.SaveChangesAsync();

                filesWrittenToDb++;

                printStatusReport();
            }

            await context.SaveChangesAsync();
            await context.Database.CloseConnectionAsync();
        }
        // Stupid fix
        SqliteConnection.ClearAllPools();
        GC.Collect();
        GC.WaitForPendingFinalizers();

        importingFiles = false;

        await tarWritingTask;

        // Add the database to the archive
        tarWriter.WriteEntry(dbPath, "index.db");
    }

    _ = process.StandardOutput.ReadToEnd();
    process.WaitForExit();
}
File.Delete(dbPath);

Console.WriteLine("Done!");

void printStatusReportInner()
{
    ulong filesWrittenToTarLocal = Interlocked.Read(ref filesWrittenToTar);
    ulong bytesWrittenToTarLocal = Interlocked.Read(ref bytesWrittenToTar);

    string bytesTotalFormatted = Utilss.FormatNumberByteSize(lastSummary.BytesTotal);
    printStatus(cursorPos, "Status:",
        $"   {lastSummary.FilesProcessed} / {files.Count} files hashed ({Utilss.FormatNumberByteSize(lastSummary.BytesProcessed)} / {bytesTotalFormatted})",
        $"   {filesWrittenToDb} / {files.Count} files written to DB",
        $"   {filesWrittenToTarLocal} / {files.Count} files written to Tar ({Utilss.FormatNumberByteSize(bytesWrittenToTarLocal)} / {bytesTotalFormatted})",
        $"   Average files size: {Utilss.FormatNumberByteSize(lastSummary.BytesTotal / lastSummary.FilesTotal)}"
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

sealed record FlattenedPathTreeLevel(List<FlattenedPathTreeChunk> Chunks);
sealed record FlattenedPathTreeChunk(ulong? ParentId, string? ParentName, string[] ChildrenNames);