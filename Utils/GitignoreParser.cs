﻿using System.Diagnostics.CodeAnalysis;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;

namespace BackupTool.Utils;

internal static class GitignoreParser
{
    static bool IsSystemPath(string path)
    {
        if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
        {
            var directory = new DirectoryInfo(path);
            if (directory.Parent == null) return false;

            while (directory.Parent?.Parent != null)
            {
                directory = directory.Parent;
            }

            var dirName = directory.Name;

            return dirName == "System Volume Information" || dirName == "$RECYCLE.BIN";

        }

        return false;
    }
    static StringBuilder AddEscapedRegexCharacter(this StringBuilder sb, char c)
    {
        return (c is '.' or '^' or '$' or '*' or '+' or '-' or '?' or '(' or ')' or '[' or ']' or '{' or '}' or '|') ? sb.Append('\\').Append(c) : (c == '\\') ? sb : sb.Append(c);
    }

    static string? ParseGitPattern(ReadOnlySpan<char> str)
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

                    var subSb = new StringBuilder();

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
                                    Console.WriteLine("Invalid character class"); // Debug ("/build[.-]*")
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

        if (sb.Length <= 0)
            return null;

        sb.Append('$');

        try
        {
            return sb.ToString();
        }
        catch (Exception ex)
        {
            Console.WriteLine(ex); // Debug
            return null;
        }
    }

    public record struct GitRegex(string BasePath, Regex Regex, bool IsRootOnly, bool IsDirectory, bool IsNegated)
    {
        public static GitRegex Create(string basePath, [StringSyntax("regex")] string regex, bool rootOnly, bool dirOnly, bool negated)
        {
            return new GitRegex(basePath, new Regex(regex, RegexOptions.Compiled | RegexOptions.Singleline), rootOnly, dirOnly, negated);
        }

        private int _depth = -1;
        public int Depth
        {
            get
            {
                if (_depth == -1)
                {
                    _depth = BasePath.Count(c => c == '/');
                }
                return _depth;
            }
        }
    }

    private static IEnumerable<GitRegex> ParseGitignore(Stream stream, string path)
    {
        using var streamReader = new StreamReader(stream);

        // Convert ignore patterns to regex
        while (true)
        {
            string? line = streamReader.ReadLine();
            if (line == null) break;

            ReadOnlySpan<char> str = line.AsSpan();

            if (str.IsEmpty || str[0] == '#') continue;

            bool isNegated = str[0] == '!';
            if (isNegated) str = str[1..];

            bool isDirectory = str[^1] == '/';
            if (isDirectory) str = str[..^1];

            var regex = ParseGitPattern(str);
            if (regex == null) continue;

            yield return GitRegex.Create(
                Path.GetDirectoryName(path)!,
                regex,
                str.Contains('/'),
                isDirectory,
                isNegated
                );
        }
    }

    static IEnumerable<GitRegex> ParseGitignore(byte[] data, string path)
    {
        return ParseGitignore(new MemoryStream(data), path);
    }

    static IEnumerable<GitRegex> ParseGitignore(string path)
    {
        return ParseGitignore(File.OpenRead(path), path);
    }

    public static GitRegex[] GetUnityProjectIgnores(string projectDir)
    {
        try
        {
            var projectVersionFilePath = Path.Combine(projectDir, "ProjectSettings", "ProjectVersion.txt");
            if (File.ReadLines(projectVersionFilePath).Any(l => l.StartsWith("m_EditorVersion:")))
            {
                return ParseGitignore(Properties.Resources.GitIgnore_Unity, projectDir).ToArray();
            }
        }
        catch (Exception)
        {
        }

        return [];
    }

    private record struct DirectoryEntry(string Path, bool IsDirectory);
    static IEnumerable<string> Crawl(string path, IEnumerable<GitRegex> activePatterns, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();

        if (IsSystemPath(path)) yield break;

        DirectoryEntry[] entries;
        try
        {
            var files = Directory.GetFiles(path, "*", SearchOption.TopDirectoryOnly);
            var dirs = Directory.GetDirectories(path, "*", SearchOption.TopDirectoryOnly);

            entries = new DirectoryEntry[files.Length + dirs.Length];

            for (int i = 0; i < files.Length; i++)
            {
                cancellationToken.ThrowIfCancellationRequested();
                var file = files[i].Replace('\\', '/');

                if (file.EndsWith("/.gitignore"))
                {
                    activePatterns = activePatterns.Concat(ParseGitignore(file));
                }

                entries[i] = new DirectoryEntry(file, false);
            }

            int unityDirs = 0;
            for (int i = 0; i < dirs.Length; i++)
            {
                cancellationToken.ThrowIfCancellationRequested();
                var dir = dirs[i].Replace('\\', '/');

                if (dir.EndsWith("/ProjectSettings") || dir.EndsWith("/Assets"))
                {
                    unityDirs++;
                }

                entries[files.Length + i] = new DirectoryEntry(dir, true);
            }

            if (unityDirs >= 2)
            {
                activePatterns = activePatterns.Concat(GetUnityProjectIgnores(path));
            }
        }
        catch (Exception)
        {
            yield break;
        }

        GitRegex[] localActivePatterns = activePatterns.ToArray();

        foreach (var entry in entries)
        {
            cancellationToken.ThrowIfCancellationRequested();
            if (entry.IsDirectory && entry.Path.EndsWith("/.git")) continue;

            var matched = false;
            foreach (var pattern in localActivePatterns)
            {
                if ((pattern.IsDirectory && !entry.IsDirectory) || (matched && !pattern.IsNegated)) continue;
                if (!pattern.Regex.IsMatch(entry.Path)) continue;

                if (pattern.IsDirectory)
                {
                    matched = true;
                    break;
                }

                matched = !pattern.IsNegated;
            }

            if (!matched)
            {
                if (entry.IsDirectory)
                {
                    foreach (var result in Crawl(entry.Path, localActivePatterns.Where(p => !p.IsRootOnly), cancellationToken))
                    {
                        yield return result;
                    }
                }
                else
                {
                    yield return entry.Path;
                }
            }
        }
    }
    public static IEnumerable<string> GetTrackedFiles(string basePath, CancellationToken cancellationToken)
    {
        if (!Directory.Exists(basePath)) return [];

        return Crawl(basePath, [], cancellationToken);
    }
}
