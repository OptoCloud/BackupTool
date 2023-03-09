using OptoPacker.Utils;

string RootPath = @"C:\Users\eirik.boe\source";

Console.WriteLine("Gathering files... (This might take a couple minutes)");

var trackedFiles = GitignoreParser.GetTrackedFiles(RootPath);

Console.WriteLine("Indexing files...");

await foreach (var file in Utilss.HashAllAsync(RootPath, trackedFiles))
{
    Console.WriteLine($"Adding: {file.Hash:x2} - {Utilss.FormatNumberByteSize(file.Size)} - {file.Path}");
}

Console.WriteLine("Done!");