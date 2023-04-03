using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Metadata.Builders;

namespace OptoPacker.Database.Models;

public sealed class FileEntity
{
    public static string TableName => "files";

    public ulong Id { get; set; }
    public ulong BlobId { get; set; }
    public ulong FolderId { get; set; }
    public string Name { get; set; }
    public string Extension { get; set; }

    public BlobEntity Blob { get; set; }
    public DirectoryEntity Folder { get; set; }
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

        builder.Property(x => x.BlobId)
            .HasColumnName("blob_id")
            .IsRequired();

        builder.Property(x => x.FolderId)
            .HasColumnName("folder_id")
            .IsRequired();

        builder.Property(x => x.Name)
            .HasColumnName("name")
            .IsRequired();

        builder.Property(x => x.Extension)
            .HasColumnName("extension")
            .IsRequired();

        builder.HasOne(x => x.Blob)
            .WithMany()
            .HasForeignKey(x => x.BlobId)
            .OnDelete(DeleteBehavior.Restrict);

        builder.HasOne(x => x.Folder)
            .WithMany(x => x.Files)
            .HasForeignKey(x => x.FolderId)
            .OnDelete(DeleteBehavior.Restrict);

        builder.HasIndex(x => new { x.FolderId, x.Name, x.Extension })
            .IsUnique();
    }
}