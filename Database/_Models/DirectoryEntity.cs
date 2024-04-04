using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Metadata.Builders;

namespace BackupTool.Database.Models;

public sealed class DirectoryEntity
{
    public static string TableName => "directories";
    public static ulong RootId => 1;
    public static string RootName => "root";

    public ulong Id { get; set; }
    public required string Name { get; set; }
    public ulong? ParentId { get; set; }
    public DirectoryEntity? Parent { get; set; }
    public ICollection<DirectoryEntity>? Children { get; set; }
    public ICollection<FileEntity>? Files { get; set; }
}

public sealed class DirectoryEntityConfiguration : IEntityTypeConfiguration<DirectoryEntity>
{
    public void Configure(EntityTypeBuilder<DirectoryEntity> builder)
    {
        builder.ToTable(DirectoryEntity.TableName);
        builder.HasKey(x => x.Id);

        builder.Property(x => x.Id)
            .HasColumnName("id")
            .ValueGeneratedOnAdd();

        builder.Property(x => x.ParentId)
            .HasColumnName("parent_id");

        builder.Property(x => x.Name)
            .HasColumnName("name")
            .IsRequired();

        builder.HasOne(x => x.Parent)
            .WithMany(x => x.Children)
            .HasForeignKey(x => x.ParentId)
            .OnDelete(DeleteBehavior.Cascade);

        builder.HasIndex(x => new { x.ParentId, x.Name })
            .IsUnique();
    }
}