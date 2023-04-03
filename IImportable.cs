using Microsoft.EntityFrameworkCore;
using OptoPacker.Database;

namespace OptoPacker;

internal interface IImportable
{
    public string BasePath { get; }
    IEnumerable<string> GetFiles(CancellationToken cancellationToken = default);
}
