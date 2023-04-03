using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Metadata.Builders;

namespace OptoPacker.Database.Models;

public sealed class BlobEntity
{
    public static string TableName => "blobs";

    public ulong Id { get; set; }
    public ulong Size { get; set; }
    public required byte[] Hash { get; set; }
}

public sealed class BlobEntityConfiguration : IEntityTypeConfiguration<BlobEntity>
{
    public void Configure(EntityTypeBuilder<BlobEntity> builder)
    {
        builder.ToTable(BlobEntity.TableName);
        builder.HasKey(x => x.Id);

        builder.Property(x => x.Id)
            .HasColumnName("id")
            .ValueGeneratedOnAdd();

        builder.Property(x => x.Size)
            .HasColumnName("size")
            .IsRequired();

        builder.Property(x => x.Hash)
            .HasColumnName("hash")
            .IsRequired();

        builder.HasIndex(x => x.Hash)
            .IsUnique();
    }
}