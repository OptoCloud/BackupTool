using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Metadata.Builders;

namespace OptoPacker.Database.Models;

public sealed class FileEntity
{
    public static string TableName => "files";

    public ulong Id { get; set; }
    public required string Name { get; set; }
    public required string Extension { get; set; }

    public ulong BlobId { get; set; }
    public BlobEntity? Blob { get; set; }

    public ulong DirectoryId { get; set; }
    public DirectoryEntity? Directory { get; set; }
}

public sealed class FileEntityConfiguration : IEntityTypeConfiguration<FileEntity>
{
    public void Configure(EntityTypeBuilder<FileEntity> builder)
    {
        builder.ToTable(FileEntity.TableName);
        builder.HasKey(x => x.Id);

        builder.Property(x => x.Id)
            .HasColumnName("id")
            .ValueGeneratedOnAdd();

        builder.Property(x => x.Name)
            .HasColumnName("name")
            .IsRequired();

        builder.Property(x => x.Extension)
            .HasColumnName("extension")
            .IsRequired();

        builder.Property(x => x.BlobId)
            .HasColumnName("blob_id")
            .IsRequired();

        builder.Property(x => x.DirectoryId)
            .HasColumnName("directory_id")
            .IsRequired();

        builder.HasOne(x => x.Blob)
            .WithMany()
            .HasForeignKey(x => x.BlobId)
            .OnDelete(DeleteBehavior.Restrict);

        builder.HasOne(x => x.Directory)
            .WithMany(x => x.Files)
            .HasForeignKey(x => x.DirectoryId)
            .OnDelete(DeleteBehavior.Restrict);

        builder.HasIndex(x => new { x.DirectoryId, x.Name, x.Extension })
            .IsUnique();
    }
}