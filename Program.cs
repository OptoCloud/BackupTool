using OptoPacker.DTOs;
using OptoPacker.Utils;
using System.Collections.Concurrent;
using System.Text;

string RootPath = @"D:\3D Projects\VRChat";
int ChunkSize = 128;
int ParallelTasks = Environment.ProcessorCount;
int PrintIntervalMs = 100;
int hashingBlockSize = 4096;

Console.WriteLine("Gathering files...");
string[] files = GitignoreParser.GetTrackedFiles(RootPath).OrderBy(_ => Random.Shared.Next()).ToArray();
string[][] fileChunks = files.Chunk(ChunkSize).ToArray();
ConcurrentBag<string[]> fileChunksBag = new ConcurrentBag<string[]>(fileChunks);
Console.WriteLine($"Done, processing statistics:");
Console.WriteLine($"  Files: {files.Length} ({fileChunks.Length} chunks, each {ChunkSize} files)");
Console.WriteLine($"  Parallel tasks: {ParallelTasks}");
Console.WriteLine($"  Print interval: {PrintIntervalMs} ms");
Console.WriteLine($"  Hashing block size: {hashingBlockSize} bytes");
Console.WriteLine($"  Total open file handles: {ParallelTasks * ChunkSize}");
Console.WriteLine();

Console.WriteLine("Hashing files... (This might take a couple minutes)");
ConcurrentBag<InputFileInfo[]> processedChunksBag = new ConcurrentBag<InputFileInfo[]>();

async Task process(string[] chunk, FileUtils.MultiFileStatusReportFunc statusReport, CancellationToken cancellationToken)
{
    InputFileInfo[] output = new InputFileInfo[chunk.Length];

    int j = 0;
    await foreach (InputFileInfo file in FileUtils.HashAllAsync(RootPath, chunk, hashingBlockSize, statusReport, cancellationToken: cancellationToken))
    {
        output[j++] = file;
    }

    processedChunksBag?.Add(j == chunk.Length ? output : output[..j]);
}

Task[] jobs = new Task[ParallelTasks];
FileUtils.MultiFileStatusReport[] jobStatuses = new FileUtils.MultiFileStatusReport[ParallelTasks];
for (int i = 0; i < jobs.Length; i++)
{
    int jobIndex = i;
    jobs[i] = Task.Run(async () =>
    {
        FileUtils.MultiFileStatusReport localReport = new FileUtils.MultiFileStatusReport(0, 0, 0, 0, 0);
        while (fileChunksBag.TryTake(out string[]? chunk))
        {
            FileUtils.MultiFileStatusReport chunkReport = new FileUtils.MultiFileStatusReport((uint)chunk.Length, 0, 0, 0, 0);
            await process(chunk, sr =>
            {
                lock (jobStatuses)
                {
                    chunkReport = sr;
                    jobStatuses[jobIndex] = localReport + sr;
                }
            }, CancellationToken.None);
            localReport += chunkReport;
        }
    });
}

bool isDone = false;

List<int> prevLines = new List<int>();
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

Task monitor = Task.Run(async () =>
{
    int cursorPos = Console.CursorTop;
    while (!isDone)
    {
        await Task.Delay(PrintIntervalMs);
        FileUtils.MultiFileStatusReport summary = new FileUtils.MultiFileStatusReport(0, 0, 0, 0, 0);
        lock (jobStatuses)
        {
            foreach (FileUtils.MultiFileStatusReport jobStatus in jobStatuses) summary += jobStatus;
        }
        if (summary.bytesTotal == 0 || summary.filesTotal == 0) continue;
        printStatus(cursorPos, "Status:",
            $"   Processed {summary.filesProcessed} / {files.Length} Files",
            $"   Processed {Utilss.FormatNumberByteSize(summary.bytesProcessed)} / {Utilss.FormatNumberByteSize(summary.bytesTotal)} Bytes",
            $"   Average files size: {Utilss.FormatNumberByteSize(summary.bytesTotal / summary.filesTotal)}"
            );
    }
});

await Task.WhenAll(jobs);
isDone = true;
await monitor;

if (fileChunksBag.ToArray().Length != 0) throw new Exception("fileChunksBag is not empty");

InputFileInfo[][] processedChunks = processedChunksBag.ToArray();
InputFileInfo[] processedFiles = new InputFileInfo[processedChunks.Sum(c => c.Length)];
int k = 0;
foreach (InputFileInfo[] chunk in processedChunks)
{
    Array.Copy(chunk, 0, processedFiles, k, chunk.Length);
    k += chunk.Length;
}

Array.Sort(processedFiles, (a, b) => string.Compare(a.Path.Split('.', '/')[^1], b.Path.Split('.', '/')[^1]));

foreach (InputFileInfo item in processedFiles)
{
    if (!String.IsNullOrEmpty(item.Error))
    {
        Console.WriteLine($"Error: {item.Path} - {item.Error}");
    }
}

Console.WriteLine("\nDone hashing files!");