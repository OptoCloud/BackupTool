using BackupTool.Database.Models;
using Microsoft.EntityFrameworkCore;

namespace BackupTool.Database;

public sealed class BTContext : DbContext
{
    public BTContext(DbContextOptions<DbContext> options) : base(options)
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
