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
int hashingBlockSize = 4 * 1024 * 1024;

var imports = new List<IImportable>()
{
    //new ImportFolder( @"H:\"),
    new ImportFolder( @"D:\3D Projects"),
};

// Get path to temp folder, and create sqlite database
string tempPath = Path.GetTempPath();
string dbPath = Path.Combine(tempPath, "OptoPacker.db");
string archivePath = "D:\\archive.7z";

int cursorPos;
uint filesComplete = 0;
ulong bytesProcessed = 0;
List<int> prevLines = new List<int>();
Dictionary<string, PathTreeFile> files;

// Start the 7-Zip process
using (var process = new Process())
{
    process.StartInfo.FileName = "7z"; // Path to 7z executable
    process.StartInfo.Arguments = $"a -t7z \"{archivePath}\" -si\"data.tar\" -mx=5 -m0=lzma2 -aoa";
    process.StartInfo.RedirectStandardInput = true;
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

        using (var context = new OptoPackerContext(options))
        {
            context.Database.EnsureCreated();

            Console.WriteLine("Gathering files...");
            var importFiles = imports.Select(x => new PathTree(x.BasePath).AddMany(x.GetFiles())).ToArray();

            Console.WriteLine("Creating directory database entries...");
            var levels = new List<FlattenedPathTreeLevel>();
            foreach (var importFile in importFiles)
            {
                await importFile.DbCreateDirectoriesAsync(context);
            }

            Console.WriteLine("Hashing files... (This might take a couple minutes)");
            cursorPos = Console.CursorTop;
            files = importFiles.SelectMany(x => x.GetAllFiles()).ToDictionary(x => x.OriginalPath);
            await foreach (ProcessedFileInfo file in ImportProcessor.ProcessFilesAsync(files.Keys.ToArray(), printStatusReport, hashingBlockSize, ChunkSize, ParallelTasks))
            {
                string hash = Utilss.BytesToHex(file.Hash);

                var tarWriterTask = tarWriter.WriteEntryAsync(file.Path, $"files/{hash[..2]}/{hash}");

                var blob = await context.Blobs.FirstOrDefaultAsync();
                if (blob == null)
                {
                    blob = new BlobEntity()
                    {
                        Hash = file.Hash,
                        Size = file.Size,
                    };
                    await context.Blobs.AddAsync(blob);
                    await context.SaveChangesAsync();
                }

                var filee = files[file.Path];
                string name = filee.Name;
                int extPos = name.LastIndexOf('.');

                await context.Files.AddAsync(new FileEntity
                {
                    BlobId = blob.Id,
                    DirectoryId = filee.DirectoryId!.Value,
                    Name = extPos > 0 ? name[..extPos] : name,
                    Extension = extPos > 0 ? name[(extPos + 1)..] : string.Empty,
                    Blob = blob,
                });
                await context.SaveChangesAsync();

                await tarWriterTask;
                filesComplete++;
                bytesProcessed += file.Size;
            }

            await context.SaveChangesAsync();
            await context.Database.CloseConnectionAsync();
        }
        // Stupid fix
        SqliteConnection.ClearAllPools();
        GC.Collect();
        GC.WaitForPendingFinalizers();

        // Add the database to the archive
        tarWriter.WriteEntry(dbPath, "index.db");
    }

    process.WaitForExit();
}
File.Delete(dbPath);

Console.WriteLine("Done!");

void printStatusReport(MultiFileStatusReport summary)
{
    printStatus(cursorPos, "Status:",
        $"   {summary.filesProcessed} / {files.Count} files hashed ({Utilss.FormatNumberByteSize(summary.bytesProcessed)} / {Utilss.FormatNumberByteSize(summary.bytesTotal)})",
        $"   {filesComplete} / {files.Count} files archived ({Utilss.FormatNumberByteSize(bytesProcessed)} / {Utilss.FormatNumberByteSize(summary.bytesTotal)})",
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

sealed record FlattenedPathTreeLevel(List<FlattenedPathTreeChunk> Chunks);
sealed record FlattenedPathTreeChunk(ulong? parentId, string? parentName, string[] childrenNames);