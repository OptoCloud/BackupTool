using OptoPacker;
using OptoPacker.DTOs;
using OptoPacker.Utils;
using System.Text;

int ChunkSize = 128;
int ParallelTasks = Environment.ProcessorCount;
int PrintIntervalMs = 100;
int hashingBlockSize = 4096;

List<IImportable> imports = new List<IImportable>()
{
    new ImportFolder( @"C:\Users\eirik.boe\"),
    new ImportFile( @"C:\Users\eirik.boe\Downloads\1496085feb3c3459d9323b86f2234984.jpg"),
};

Console.WriteLine("Gathering files...");
string[] files = imports.SelectMany(x => x.GetFiles()).ToArray();

Console.WriteLine($"Done, processing statistics:");
Console.WriteLine($"  Files: {files.Length}");
Console.WriteLine($"  Parallel tasks: {ParallelTasks}");
Console.WriteLine($"  Print interval: {PrintIntervalMs} ms");
Console.WriteLine($"  Hashing block size: {hashingBlockSize} bytes");
Console.WriteLine($"  Total open file handles: {ParallelTasks * ChunkSize}");
Console.WriteLine();

int i = 0;
Console.WriteLine("Hashing files... (This might take a couple minutes)");
int cursorPos = Console.CursorTop;
List<int> prevLines = new List<int>();
await foreach (InputFileInfo file in ImportProcessor.ProcessFiles(files, printStatusReport, hashingBlockSize, ChunkSize, ParallelTasks))
{
    i++;
}

void printStatusReport(MultiFileStatusReport summary)
{
    printStatus(cursorPos, "Status:",
        $"   Processed {summary.filesProcessed} / {files.Length} Files",
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

Console.WriteLine($"Done, processed {i} / {files.Length} files");