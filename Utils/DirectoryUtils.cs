﻿namespace OptoPacker.Utils;

internal static class DirectoryUtils
{
    public static IEnumerable<string> EnumerateFilesSafely(string path, string searchPattern, SearchOption searchOption)
    {
        string[] files;

        try
        {
            files = Directory.GetFiles(path);
        }
        catch (Exception)
        {
            files = Array.Empty<string>();
        }

        foreach (string file in files)
        {
            yield return file;
        }

        if (searchOption == SearchOption.TopDirectoryOnly)
            yield break;


        string[] directories;
        try
        {
            directories = Directory.GetDirectories(path);
        }
        catch (Exception)
        {
            directories = Array.Empty<string>();
        }

        foreach (string directory in directories)
        {
            foreach (string file in EnumerateFilesSafely(directory, searchPattern, searchOption))
            {
                yield return file;
            }
        }
    }
}
