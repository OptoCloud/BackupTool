using OptoPacker.DTOs;
using OptoPacker.Utils;
using System.Collections.Concurrent;

string RootPath = @"D:\3D Projects\VRChat";
int ChunkSize = 64;
int ParallelTasks = Environment.ProcessorCount;
int PrintIntervalMs = 250;

Console.WriteLine("Gathering files...");
var files = GitignoreParser.GetTrackedFiles(RootPath).OrderBy(_ => Random.Shared.Next()).ToArray();
var fileChunks = files.Chunk(ChunkSize).ToArray();
var fileChunksBag = new ConcurrentBag<string[]>(fileChunks);
Console.WriteLine($"Found {files.Length} files, queued {fileChunks.Length} jobs");


Console.WriteLine("Hashing files... (This might take a couple minutes)");
var processedChunksBag = new ConcurrentBag<InputFileInfo[]>();
var jobs = new Task[ParallelTasks];
for (int i = 0; i < jobs.Length; i++)
{
    jobs[i] = Task.Run(async () =>
    {
        while (fileChunksBag.TryTake(out var filesChunk))
        {
            InputFileInfo[] output = new InputFileInfo[filesChunk.Length];

            int j = 0;
            await foreach (var file in Utilss.HashAllAsync(RootPath, filesChunk))
            {
                output[j++] = file;
            }

            processedChunksBag.Add(j == filesChunk.Length ? output : output[..j]);
        }
    });
}

bool isDone = false;

InputFileInfo[][] processedChunks = new InputFileInfo[fileChunks.Length][];
var monitor = Task.Run(async () =>
{
    uint processedFileCount = 0;
    uint processedChunksCount = 0;
    ulong processedBytes = 0;
    int duplicates = 0;
    ulong deduplicationsaved = 0;
    HashSet<string> hashes = new HashSet<string>();
    while (!isDone)
    {
        await Task.Delay(PrintIntervalMs);
        while (processedChunksBag.TryTake(out var chunk))
        {
            processedChunks[processedChunksCount++] = chunk;
            processedFileCount += (uint)chunk.Length;
            foreach (var f in chunk)
            {
                processedBytes += f.Size;
                if (!hashes.Add(Convert.ToBase64String(f.Hash)))
                {
                    duplicates++;
                    deduplicationsaved += f.Size - (ulong)(32 + 8 + f.Path.Length);
                }
            }
        }
        if (processedFileCount > 0)
        {
            Console.Write($"\rProcessed: {processedFileCount} / {files.Length} ({duplicates} Duplicates, {Utilss.FormatNumberByteSize(deduplicationsaved)} Saved on dedup, {Utilss.FormatNumberByteSize(processedBytes)} Total, {Utilss.FormatNumberByteSize(processedBytes / processedFileCount)} Average)");
        }
    }
    return processedChunksCount;
});

await Task.WhenAll(jobs);
Console.WriteLine("Jobs done, awaiting monitor...");
isDone = true;
await monitor;

Console.WriteLine("Done hashing files! Left: " + fileChunksBag.ToArray().Length);