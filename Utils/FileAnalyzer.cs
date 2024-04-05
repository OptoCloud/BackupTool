using MimeDetective;
using MimeDetective.Storage;
using MimeMapping;
using System.Collections.Frozen;
using System.Collections.Immutable;
using FileCategory = MimeDetective.Storage.Category;
using TRIDLicense = MimeDetective.Definitions.Licensing.UsageType;

namespace BackupTool.Utils;

internal static class FileAnalyzer
{
    public const string UnkownMimeType = "application/octet-stream";

    private static readonly FrozenDictionary<string, string> TypeMap;
    private static readonly FrozenDictionary<string, ImmutableHashSet<FileCategory>> MimeCategories;
    private static readonly ContentInspector Inspector;

    static FileAnalyzer()
    {
        Dictionary<string, string> typeMap = MimeUtility.TypeMap.ToDictionary();

        // Programming languages
        typeMap.TryAdd("cs", "text/plain");
        typeMap.TryAdd("py", "text/plain");
        typeMap.TryAdd("go", "text/plain");
        typeMap.TryAdd("license", "text/plain");
        typeMap.TryAdd("keep", "text/plain");
        typeMap.TryAdd("gitkeep", "text/plain");
        typeMap.TryAdd("gitignore", "text/plain");
        typeMap.TryAdd("collabignore", "text/plain");
        typeMap.TryAdd("vsconfig", "text/plain");

        // Unity
        typeMap.TryAdd("mat", "text/xml");
        typeMap.TryAdd("meta", "text/yaml");
        typeMap.TryAdd("uxml", "text/yaml");
        typeMap.TryAdd("unity", "text/yaml");
        typeMap.TryAdd("asset", "text/yaml");
        typeMap.TryAdd("anim", "text/yaml");
        typeMap.TryAdd("prefab", "text/yaml");
        typeMap.TryAdd("controller", "text/yaml");
        typeMap.TryAdd("asmdef", "application/json");
        typeMap.TryAdd("hlsl", "text/plain+shader-hlsl");
        typeMap.TryAdd("shader", "text/plain+shader-hlsl");
        typeMap.TryAdd("unitypackage", "application/x-tar+gz");
        typeMap.TryAdd("rsp", "text/plain");

        // LilToon stuff
        typeMap.TryAdd("fx", "text/plain+shader-hlsl");
        typeMap.TryAdd("lilblock", "text/plain+shader-hlsl");
        typeMap.TryAdd("lilinternal", "text/plain+shader-hlsl");

        // Poiyomi stuff
        typeMap.TryAdd("poi", "text/plain");
        typeMap.TryAdd("poitemplate", "text/plain+shader-hlsl");
        typeMap.TryAdd("poitemplatecollection", "text/plain+shader-hlsl");

        var definitions = new MimeDetective.Definitions.ExhaustiveBuilder() { UsageType = TRIDLicense.PersonalNonCommercial }
            .Build()
            .TrimMeta()
            .TrimDescription()
            .ToImmutableArray();

        var mimeCategories = new Dictionary<string, ImmutableHashSet<FileCategory>>();
        foreach (var definition in definitions)
        {
            if (string.IsNullOrEmpty(definition.File.MimeType)) continue;

            foreach (var ext in definition.File.Extensions)
            {
                typeMap.TryAdd(ext, definition.File.MimeType);
            }

            mimeCategories[definition.File.MimeType] = mimeCategories.TryGetValue(definition.File.MimeType, out var cats)
                ? cats.Union(definition.File.Categories)
                : definition.File.Categories;
        }

        TypeMap = typeMap.ToFrozenDictionary();
        MimeCategories = mimeCategories.ToFrozenDictionary();

        Inspector = new ContentInspectorBuilder() {
            Definitions = definitions,
            Parallel = true,
            StringSegmentOptions = new()
            {
                OptimizeFor = MimeDetective.Engine.StringSegmentResourceOptimization.HighSpeed
            },
        }.Build() ?? throw new ApplicationException("Failed to build file inspector");
    }

    public static ImmutableHashSet<FileCategory> GetCategories(string? mime)
    {
        if (string.IsNullOrEmpty(mime))
        {
            return [];
        }

        if (!MimeCategories.TryGetValue(mime, out var categories))
        {
            return [];
        }

        return categories;
    }

    public static string? GuessMimeByFileName(string path)
    {
        string ext = PathUtils.GetExtension(path).ToLowerInvariant();
        if (ext.Length <= 0)
        {
            return Path.GetFileName(path) switch
            {
                "LICENSE" or "README" => "text/plain",
                _ => null,
            };
        }

        if (!TypeMap.TryGetValue(ext, out string? mime))
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
        string? mime = GuessMimeByFileName(path);
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