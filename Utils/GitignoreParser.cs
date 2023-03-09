using System.Text;
using System.Text.RegularExpressions;

namespace OptoPacker.Utils;

internal static class GitignoreParser
{
    static bool IsSystemPath(string path) =>
        path.Count(c => c == '\\') <= 1 && (path.EndsWith("\\$RECYCLE.BIN") || path.EndsWith("\\System Volume Information"));

    static IEnumerable<string> Crawl(string path, Dictionary<string, GitRegex[]> allPatterns, IEnumerable<GitRegex> activePatterns)
    {
        if (IsSystemPath(path)) yield break;

        var results = new List<string>();
        var localActivePatterns = new List<GitRegex>(activePatterns);

        // If we have a pattern for current dir, use it
        if (allPatterns.TryGetValue(path, out var patterns))
            localActivePatterns.AddRange(patterns);

        string[] files;
        try
        {
            files = Directory.GetFiles(path, "*", SearchOption.TopDirectoryOnly).Select(f => f.Replace('/', '\\')).ToArray();
        }
        catch (Exception)
        {
            yield break;
        }

        foreach (var file in files)
        {
            if (localActivePatterns.All(p => p.IsTracked(file, false)))
                yield return file;
        }
        foreach (var dir in Directory.GetDirectories(path, "*", SearchOption.TopDirectoryOnly))
        {
            if (dir.EndsWith("\\.git")) continue;
            if (localActivePatterns.All(p => p.IsTracked(dir, true)))
            {
                foreach (var file in Crawl(dir, allPatterns, localActivePatterns)) yield return file;
            }
        }
    }
    static IEnumerable<string> GetFiles(string path, string pattern, bool topDir = true)
    {
        if (IsSystemPath(path)) yield break;

        string[] entries;
        try
        {
            entries = Directory.GetFiles(path, pattern, SearchOption.AllDirectories);
        }
        catch (Exception)
        {
            if (!topDir) yield break;
            entries = Array.Empty<string>();
        }

        if (entries.Length > 0)
        {
            foreach (var file in entries)
            {
                yield return file;
            }
            yield break;
        }

        try
        {
            entries = Directory.GetDirectories(path);
        }
        catch (Exception)
        {
            entries = Array.Empty<string>();
        }

        foreach (var subPath in entries)
        {
            foreach (var item in GetFiles(subPath, pattern, false))
            {
                yield return item;
            }
        }
    }
    public static IEnumerable<string> GetTrackedFiles(string basePath)
    {
        var IgnorePatterns = GetFiles(basePath, "*.gitignore")
            .ToDictionary(
                (p) => Path.GetDirectoryName(p)!,
                (p) => ParseGitignore(p).ToArray()
            );
        if (IgnorePatterns == null) return Array.Empty<string>();

        // Get tracked files
        var patternStack = new List<GitRegex>();

        // Crawl
        return Crawl(basePath, IgnorePatterns!, new GitRegex[0]);
    }

    static void AddEscapedRegexCharacter(this StringBuilder sb, char c)
    {
        if (c is '.' or '^' or '$' or '*' or '+' or '-' or '?' or '(' or ')' or '[' or ']' or '{' or '}' or '\\' or '|')
        {
            sb.Append('\\').Append(c);
        }
        else
        {
            sb.Append(c);
        }
    }

    static Regex? ParseGitPattern(ReadOnlySpan<char> str)
    {
        var sb = new StringBuilder();

        for (int i = 0; i < str.Length; i++)
        {
            var c = str[i];

            switch (c)
            {
                case '\\':
                    if (++i >= str.Length)
                    {
                        Console.WriteLine("Invalid escape sequence"); // Debug
                        return null;
                    }
                    sb.AddEscapedRegexCharacter(str[i]);
                    continue;
                case '*':
                    if (i + 1 < str.Length && str[i + 1] == '*')
                    {
                        sb.Append(".*");
                        i++;
                    }
                    else
                    {
                        sb.Append("[^/]*");
                    }
                    continue;
                case '?':
                    sb.Append("[^/]");
                    continue;
                case '[':
                    if (++i >= str.Length)
                    {
                        Console.WriteLine("Invalid character class"); // Debug
                        return null;
                    }

                    StringBuilder subSb = new StringBuilder();

                    switch (c = str[i++])
                    {
                        case ']':
                            continue; // Skip empty character class
                        case '!':
                            if (++i >= str.Length || str[i] is ']' or '-')
                            {
                                Console.WriteLine("Invalid character class"); // Debug
                                return null;
                            }
                            subSb.Append('^').AddEscapedRegexCharacter(str[i]);
                            break;
                        case '-':
                            Console.WriteLine("Invalid character class"); // Debug
                            return null;
                        default:
                            subSb.AddEscapedRegexCharacter(c);
                            break;
                    }

                    bool allowDashOrClose = true;
                    for (; i < str.Length; i++)
                    {
                        c = str[i];
                        switch (c)
                        {
                            case ']':
                                if (!allowDashOrClose)
                                {
                                    Console.WriteLine("Invalid character class"); // Debug
                                    return null;
                                }
                                goto endCharClass;
                            case '-':
                                if (!allowDashOrClose)
                                {
                                    Console.WriteLine("Invalid character class"); // Debug
                                    return null;
                                }
                                subSb.Append('-');
                                allowDashOrClose = false;
                                continue;
                            case '\\':
                                if (++i >= str.Length)
                                {
                                    Console.WriteLine("Invalid escape sequence"); // Debug
                                    return null;
                                }
                                subSb.AddEscapedRegexCharacter(str[i]);
                                allowDashOrClose = true;
                                continue;
                            default:
                                subSb.AddEscapedRegexCharacter(c);
                                allowDashOrClose = true;
                                continue;
                        }
                    }
                    Console.WriteLine("Invalid character class"); // Debug
                    return null;
                endCharClass:
                    sb.Append('[').Append(subSb).Append(']');
                    continue;
                default:
                    sb.AddEscapedRegexCharacter(c);
                    continue;
            }
        }

        string built = sb.ToString();
        if (String.IsNullOrEmpty(built))
            return null;

        try
        {
            return new Regex(sb.ToString(), RegexOptions.Compiled);
        }
        catch (Exception ex)
        {
            Console.WriteLine(ex); // Debug
            return null;
        }
    }

    public record struct GitRegex(string BasePath, Regex Regex, bool IsNegated, bool IsDirectory)
    {
        public bool IsTracked(string path, bool isDirectory)
        {
            if (IsDirectory != isDirectory) return true;
            bool matched = Regex.IsMatch(path);
            return IsNegated != matched;
        }
    }

    static IEnumerable<GitRegex> ParseGitignore(string gitignorePath)
    {
        // Read file
        var lines = File.ReadAllLines(gitignorePath);

        // Convert ignore patterns to regex
        foreach (var line in lines)
        {
            ReadOnlySpan<char> str = line.AsSpan();

            if (str.IsEmpty || str[0] == '#') continue;

            bool isNegated = str[0] == '!';
            if (isNegated) str = str[1..];

            bool isDirectory = str[^1] == '/';
            if (isDirectory) str = str[..^1];

            var regex = ParseGitPattern(str);
            if (regex == null) continue;

            yield return new GitRegex(
                Path.GetDirectoryName(gitignorePath)!,
                regex,
                isNegated,
                isDirectory
                );
        }
    }
}
