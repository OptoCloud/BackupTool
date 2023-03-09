namespace OptoPacker.Utils.Utils;

internal static class FileAnalyzer
{
    private const int AnalyzeSize = 512;
    private const int AnalysisCacheConclusiveSize = 128;
    private const int AnalysisCacheDifferenceFactor = 20;

    private record struct FileAnalysisResult(int AsciiCount, int UnicodeCount, int UnknownCount);
    private static readonly FileAnalysisResult DefaultAnalysisResult = new FileAnalysisResult(0, 0, 0);
    private static readonly Dictionary<string, FileAnalysisResult> FileAnalysisCache = new Dictionary<string, FileAnalysisResult>();

    private static FileAnalysisResult GetAnalysisResult(string ext)
    {
        if (!string.IsNullOrEmpty(ext) && FileAnalysisCache.TryGetValue(ext, out var result))
            return result;

        return DefaultAnalysisResult;
    }

    public enum AnalysisSummary
    {
        ASCII,
        Unicode,
        Binary,
        Unconclusive
    }
    private static AnalysisSummary GetAnalysisSummary(FileAnalysisResult result)
    {
        if (result.AsciiCount + result.UnicodeCount + result.UnknownCount >= AnalysisCacheConclusiveSize)
        {
            if (result.AsciiCount > (result.UnknownCount + result.UnicodeCount) * AnalysisCacheDifferenceFactor) return AnalysisSummary.ASCII;
            if (result.UnknownCount > (result.AsciiCount + result.UnicodeCount) * AnalysisCacheDifferenceFactor) return AnalysisSummary.Binary;
            if (result.UnicodeCount > (result.AsciiCount + result.UnknownCount) * AnalysisCacheDifferenceFactor) return AnalysisSummary.Unicode;
        }

        return AnalysisSummary.Unconclusive;
    }

    private static bool TryGetAnalysisResultAndSummary(string ext, out FileAnalysisResult result, out AnalysisSummary summary)
    {
        result = GetAnalysisResult(ext);
        summary = GetAnalysisSummary(result);
        return summary != AnalysisSummary.Unconclusive;
    }
    private static void SetAnalysisResult(string ext, FileAnalysisResult prevResult, TextEncoding encoding)
    {
        if (!string.IsNullOrEmpty(ext))
        {
            FileAnalysisCache[ext] = encoding switch
            {
                TextEncoding.ASCII => prevResult with { AsciiCount = prevResult.AsciiCount + 1 },
                TextEncoding.Unknown => prevResult with { UnknownCount = prevResult.UnknownCount + 1 },
                _ => prevResult with { UnicodeCount = prevResult.UnicodeCount + 1 }
            };
        }
    }

    public enum TextEncoding
    {
        ASCII,
        UTF1,
        UTF7,
        UTF8,
        UTF16_LE,
        UTF16_BE,
        UTF32_LE,
        UTF32_BE,
        UTF_EBCDIC,
        UTF_SCSU,
        UTF_BOCU1,
        UTF_GB_18030,
        Unknown
    }
    public static TextEncoding GetTextEncoding(string fileExtension, ReadOnlySpan<byte> bytes)
    {
        TextEncoding encoding;

        if (bytes.Length >= 4)
        {
            encoding = bytes[..4] switch
            {
                [0xFE, 0xFF, _, _] => TextEncoding.UTF16_BE,
                [0x0E, 0xFE, 0xFF, _] => TextEncoding.UTF_SCSU,
                [0x2B, 0x2F, 0x76, _] => TextEncoding.UTF7,
                [0xEF, 0xBB, 0xBF, _] => TextEncoding.UTF8,
                [0xF7, 0x64, 0x4C, _] => TextEncoding.UTF1,
                [0xFB, 0xEE, 0x28, _] => TextEncoding.UTF_BOCU1,
                [0xFF, 0xFE, 0x00, 0x00] => TextEncoding.UTF32_LE,
                [0x00, 0x00, 0xFE, 0xFF] => TextEncoding.UTF32_BE,
                [0xDD, 0x73, 0x66, 0x73] => TextEncoding.UTF_EBCDIC,
                [0x84, 0x31, 0x95, 0x33] => TextEncoding.UTF_GB_18030,
                [0xFF, 0xFE, _, _] => TextEncoding.UTF16_LE,
                _ => TextEncoding.Unknown
            };
        }
        else
        {
            encoding = TextEncoding.Unknown;
        }

        if (encoding == TextEncoding.Unknown)
        {
            encoding = TextEncoding.ASCII;
            foreach (byte b in bytes)
            {
                if (b is < 0x20 and not (0x09 or 0x0A or 0x0D) or > 0x7E)
                {
                    encoding = TextEncoding.Unknown;
                    break;
                }
            }
        }

        return encoding;
    }

    public enum ContentType
    {
        ASCII,
        Unicode,
        Binary
    }
    public record struct FileAnalysis(string Path, string Extension, ContentType ContentType);
    public static FileAnalysis AnalyseFile(string filePath)
    {
        ContentType contentType;
        string fileExtension = Path.GetExtension(filePath);

        if (!TryGetAnalysisResultAndSummary(fileExtension, out var result, out var summary))
        {
            using FileStream fs = new FileStream(filePath, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
            using BufferedStream bs = new BufferedStream(fs);
            using BinaryReader br = new BinaryReader(bs);

            byte[] bytes = br.ReadBytes(AnalyzeSize);

            TextEncoding encoding = GetTextEncoding(fileExtension, bytes);

            SetAnalysisResult(fileExtension, result, encoding);

            contentType = encoding switch
            {
                TextEncoding.ASCII => ContentType.ASCII,
                TextEncoding.Unknown => ContentType.Binary,
                _ => ContentType.Unicode
            };
        }
        else
        {
            contentType = summary switch
            {
                AnalysisSummary.ASCII => ContentType.ASCII,
                AnalysisSummary.Binary => ContentType.Binary,
                _ => ContentType.Unicode
            };
        }

        return new FileAnalysis(filePath, fileExtension, contentType);
    }
}