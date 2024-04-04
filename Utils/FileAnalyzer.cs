using MimeDetective;
using MimeMapping;
using System.Collections.ObjectModel;

namespace BackupTool.Utils;

internal static class FileAnalyzer
{
    public const string UnkownMimeType = "application/octet-stream";

    private static ReadOnlyDictionary<string, string> GetTypeMap()
    {
        Dictionary<string, string> dict = MimeUtility.TypeMap.ToDictionary();

        // Programming languages
        dict.TryAdd("cs", "text/plain");
        dict.TryAdd("py", "text/plain");
        dict.TryAdd("go", "text/plain");
        dict.TryAdd("license", "text/plain");
        dict.TryAdd("keep", "text/plain");
        dict.TryAdd("gitkeep", "text/plain");
        dict.TryAdd("gitignore", "text/plain");
        dict.TryAdd("collabignore", "text/plain");
        dict.TryAdd("vsconfig", "text/plain");

        // Unity
        dict.TryAdd("mat", "text/xml");
        dict.TryAdd("meta", "text/yaml");
        dict.TryAdd("uxml", "text/yaml");
        dict.TryAdd("unity", "text/yaml");
        dict.TryAdd("asset", "text/yaml");
        dict.TryAdd("anim", "text/yaml");
        dict.TryAdd("prefab", "text/yaml");
        dict.TryAdd("controller", "text/yaml");
        dict.TryAdd("asmdef", "application/json");
        dict.TryAdd("hlsl", "text/plain+shader-hlsl");
        dict.TryAdd("shader", "text/plain+shader-hlsl");
        dict.TryAdd("unitypackage", "application/x-tar+gz");
        dict.TryAdd("rsp", "text/plain");

        // LilToon stuff
        dict.TryAdd("fx", "text/plain+shader-hlsl");
        dict.TryAdd("lilblock", "text/plain+shader-hlsl");
        dict.TryAdd("lilinternal", "text/plain+shader-hlsl");

        // Poiyomi stuff
        dict.TryAdd("poi", "text/plain");
        dict.TryAdd("poitemplate", "text/plain+shader-hlsl");
        dict.TryAdd("poitemplatecollection", "text/plain+shader-hlsl");

        return dict.AsReadOnly();
    }
    private static readonly ReadOnlyDictionary<string, string> TypeMap = GetTypeMap();

    private static IList<MimeDetective.Storage.Definition> ExhaustiveDefinitions => new MimeDetective.Definitions.ExhaustiveBuilder() { UsageType = MimeDetective.Definitions.Licensing.UsageType.PersonalNonCommercial }.Build();
    private static readonly ContentInspector Inspector = new ContentInspectorBuilder() { Definitions = ExhaustiveDefinitions, Parallel = true }.Build() ?? throw new ApplicationException("Failed to build file inspector");

    public static string? GuessMimeByExtension(string path)
    {
        string ext = PathUtils.GetExtension(path).ToLowerInvariant();
        if (ext.Length <= 0)
            return null;

        if (!TypeMap.TryGetValue(ext, out string? mime)) // DEBUG: Is filename prefixed by '.'?
            return null;

        return mime;
    }

    public static string? GuessMimeByContents(Stream stream)
    {
        return Inspector
            .Inspect(stream)
            .Where(m => m.Type == MimeDetective.Engine.DefinitionMatchType.Complete)
            .FirstOrDefault()?
            .Definition
            .File
            .MimeType;
    }

    public static string GuessMime(string path)
    {
        string? mime = GuessMimeByExtension(path);
        if (mime != null) return mime;

        string filename = Path.GetFileName(path);

        switch (filename)
        {
            case "README":
            case "LICENSE":
                return "text/plain";
            default:
                break;
        }

        using var fs = File.OpenRead(path);

        return GuessMimeByContents(fs) ?? UnkownMimeType;
    }
}