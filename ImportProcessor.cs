using OptoPacker.Delegates;
using OptoPacker.DTOs;
using OptoPacker.Utils;
using System.Collections.Concurrent;
using System.Runtime.CompilerServices;

namespace OptoPacker;

internal static class ImportProcessor
{
    private static async Task FileProcessorJob(int hashingBlockSize, MultiFileStatusReportFunc statusReportFunc, ConcurrentBag<string[]> inputBag, ConcurrentBag<ProcessedFileInfo[]> outputBag, CancellationToken cancellationToken)
    {
        MultiFileStatusReport localReport = new MultiFileStatusReport(0, 0, 0, 0, 0);


        while (inputBag.TryTake(out string[]? chunk))
        {
            MultiFileStatusReport chunkReport = new MultiFileStatusReport((uint)chunk.Length, 0, 0, 0, 0);

            ProcessedFileInfo[] output = new ProcessedFileInfo[chunk.Length];

            void subStatusReportFunc(MultiFileStatusReport report)
            {
                chunkReport = report;
                statusReportFunc.Invoke(localReport + report);
            }
            int j = 0;
            await foreach (ProcessedFileInfo file in FileUtils.HashAllAsync(chunk, subStatusReportFunc, hashingBlockSize, cancellationToken))
            {
                output[j++] = file;
            }

            outputBag.Add(j == chunk.Length ? output : output[..j]);
            localReport += chunkReport;
        }
    }

    public static async IAsyncEnumerable<ProcessedFileInfo> ProcessFilesAsync(string[] files, MultiFileStatusReportFunc statusReportFunc, int hashingBlockSize, int chunkingSize = 128, int parallelism = -1, [EnumeratorCancellation] CancellationToken cancellationToken = default)
    {
        // Default to number of logical cores
        parallelism = parallelism == -1 ? Environment.ProcessorCount : parallelism;

        Task[] jobs = new Task[parallelism];
        ConcurrentBag<string[]> inputChunksBag = new ConcurrentBag<string[]>(files.Chunk(chunkingSize));
        ConcurrentBag<ProcessedFileInfo[]> outputChunksBag = new ConcurrentBag<ProcessedFileInfo[]>();
        MultiFileStatusReport[] jobStatuses = new MultiFileStatusReport[parallelism];

        for (int i = 0; i < jobs.Length; i++)
        {
            int jobIndex = i;
            void subStatusReportFunc(MultiFileStatusReport report)
            {
                jobStatuses[jobIndex] = report;
                lock (jobStatuses)
                {
                    statusReportFunc(jobStatuses.Aggregate((a, b) => a + b) with { filesTotal = (uint)files.Length });
                }
            }
            jobs[i] = FileProcessorJob(hashingBlockSize, subStatusReportFunc, inputChunksBag, outputChunksBag, cancellationToken);
        }

        var allTasks = Task.WhenAll(jobs);

        // Gradually yield results as they are ready
        while (!allTasks.IsCompleted)
        {
            await Task.Delay(100, cancellationToken);
            if (outputChunksBag.TryTake(out ProcessedFileInfo[]? chunk))
            {
                foreach (ProcessedFileInfo file in chunk)
                {
                    yield return file;
                }
            }
        }

        // Ensure all tasks are completed
        await allTasks;

        var aggregatedStatus = jobStatuses.Aggregate((a, b) => a + b);

        // Ensure all input chunks are processed
        foreach (string[] chunk in inputChunksBag.ToArray())
        {
            void subStatusReportFunc(MultiFileStatusReport report)
            {
                statusReportFunc((aggregatedStatus + report) with { filesTotal = (uint)files.Length });
            }
            await foreach (ProcessedFileInfo file in FileUtils.HashAllAsync(chunk, subStatusReportFunc, hashingBlockSize, cancellationToken))
            {
                yield return file;
            }
        }

        // Ensure all output chunks are exported
        while (outputChunksBag.TryTake(out ProcessedFileInfo[]? chunk))
        {
            foreach (ProcessedFileInfo file in chunk)
            {
                yield return file;
            }
        }
    }
}
