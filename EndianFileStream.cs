namespace OptoPacker;

internal class EndianFileStream : FileStream
{
    readonly byte[] _buffer = new byte[1024];
    int _bufferPos = 0;

    public EndianFileStream(string path, FileMode mode) : base(path, mode)
    {
    }

    int FillBuffer()
    {
        _bufferPos = 0;
        return Read(_buffer, 0, (int)Math.Min(Length - Position, _buffer.Length));
    }
    void EnsureBuffer(int bytes)
    {
        if (bytes > _buffer.Length) throw new ArgumentOutOfRangeException("Bytes exceed buffer size!");
        if (bytes > _buffer.Length - _bufferPos)
        {
            if (FillBuffer() < bytes) throw new Exception("Size to be read exceeds file bounds");
        }
    }

    public short ReadInt16()
    {
        EnsureBuffer(sizeof(short));
        return BitConverter.ToInt16(_buffer, 0);
    }
    public ushort ReadUInt16()
    {
        EnsureBuffer(sizeof(ushort));
        return BitConverter.ToUInt16(_buffer, 0);
    }
    public int ReadInt32()
    {
        EnsureBuffer(sizeof(int));
        return BitConverter.ToInt32(_buffer, 0);
    }
    public uint ReadUInt32()
    {
        EnsureBuffer(sizeof(uint));
        return BitConverter.ToUInt32(_buffer, 0);
    }
    public long ReadInt64()
    {
        EnsureBuffer(sizeof(long));
        return BitConverter.ToInt64(_buffer, 0);
    }
    public ulong ReadUInt64()
    {
        EnsureBuffer(sizeof(ulong));
        return BitConverter.ToUInt64(_buffer, 0);
    }

}
