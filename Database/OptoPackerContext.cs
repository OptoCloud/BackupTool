using Microsoft.EntityFrameworkCore;
using OptoPacker.Database.Models;

namespace OptoPacker.Database;

public sealed class OptoPackerContext : DbContext
{
    public OptoPackerContext(DbContextOptions<OptoPackerContext> options) : base(options)
    {
    }

    public DbSet<FileEntity> Files { get; set; }
    public DbSet<BlobEntity> Blobs { get; set; }
    public DbSet<DirectoryEntity> Directories { get; set; }

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        modelBuilder.ApplyConfiguration(new FileEntityConfiguration());
        modelBuilder.ApplyConfiguration(new BlobEntityConfiguration());
        modelBuilder.ApplyConfiguration(new DirectoryEntityConfiguration());
    }
}
