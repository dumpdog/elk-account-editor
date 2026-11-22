using System.Security.Cryptography;
using System.Text;

namespace Elk.Protocol;

/// <summary>CRC-16, polynomial 0x1021, init 0, MSB first. Used for both wire frames and stored record CRCs.
/// Verified against ElkRP: NetAES.AddToCRC, ElkComm2.dll FUN_10001fea, and stored zone ModCRC/UDCRC values.</summary>
public static class Crc16
{
    public static ushort Compute(ReadOnlySpan<byte> data)
    {
        uint crc = 0;
        foreach (var b in data) crc = Add(crc, b);
        return (ushort)crc;
    }

    private static uint Add(uint crc, byte b)
    {
        crc ^= (uint)b << 8;
        for (int i = 0; i < 8; i++)
            crc = (crc & 0x8000) != 0 ? ((crc << 1) ^ 0x1021) & 0xFFFF : (crc << 1) & 0xFFFF;
        return crc;
    }
}

/// <summary>Frame: 10 02 | payload (0x10 doubled) | 10 03 | crc_hi crc_lo. Mirrors NetAES.SendMessage.</summary>
public static class Framing
{
    private const byte Dle = 0x10;

    public static byte[] Encode(ReadOnlySpan<byte> payload)
    {
        var o = new List<byte>(payload.Length + 8) { Dle, 0x02 };
        foreach (var b in payload) { o.Add(b); if (b == Dle) o.Add(Dle); }
        var crc = Crc16.Compute(payload);
        o.Add(Dle); o.Add(0x03); o.Add((byte)(crc >> 8)); o.Add((byte)(crc & 0xFF));
        return o.ToArray();
    }
}

public enum FrameKind { Message, Ack, Nak }

public sealed record Frame(FrameKind Kind, byte[] Payload, bool CrcOk);

/// <summary>Streaming parser that follows NetAES.ParseM1Msg: 10 06 = ACK, 10 15 = NAK, a body starting with 0xFF is debug text.</summary>
public sealed class FrameParser
{
    private enum S { Idle, Dle1, Body, Dle2, Crc1, Crc2 }
    private S _s = S.Idle;
    private readonly List<byte> _buf = new();
    private bool _debugText;
    private byte _crcHi;
    private readonly bool _ffIsDebugText;

    /// <param name="ffIsDebugText">True (the default) for the PC side: a panel frame whose body starts with 0xFF is debug text. Pass false to parse what a PC sends to the panel, where 0xFF starts real write requests.</param>
    public FrameParser(bool ffIsDebugText = true) { _ffIsDebugText = ffIsDebugText; }

    public IEnumerable<Frame> Feed(ReadOnlySpan<byte> data)
    {
        var frames = new List<Frame>();
        foreach (var b in data)
        {
            switch (_s)
            {
                case S.Idle:
                    if (b == 0x10) _s = S.Dle1;
                    break;
                case S.Dle1:
                    if (b == 0x02) { Reset(); _s = S.Body; }
                    else if (b == 0x06) { frames.Add(new(FrameKind.Ack, new byte[] { 0x10, 0x06 }, true)); _s = S.Idle; }
                    else if (b == 0x15) { frames.Add(new(FrameKind.Nak, new byte[] { 0x10, 0x15 }, true)); _s = S.Idle; }
                    else _s = S.Idle;
                    break;
                case S.Body:
                    if (b == 0xFF && _buf.Count == 0 && _ffIsDebugText) _debugText = true;
                    else if (b == 0x10) _s = S.Dle2;
                    else if (!_debugText) _buf.Add(b);
                    break;
                case S.Dle2:
                    if (b == 0x10) { if (!_debugText) _buf.Add(0x10); _s = S.Body; }
                    else if (b == 0x02) { Reset(); _s = S.Body; }
                    else if (b == 0x03) _s = S.Crc1;
                    else if (b == 0x06) { frames.Add(new(FrameKind.Ack, new byte[] { 0x10, 0x06 }, true)); _s = S.Idle; }
                    else if (b == 0x15) { frames.Add(new(FrameKind.Nak, new byte[] { 0x10, 0x15 }, true)); _s = S.Idle; }
                    else _s = S.Idle;
                    break;
                case S.Crc1:
                    _crcHi = b; _s = S.Crc2;
                    break;
                case S.Crc2:
                    if (!_debugText)
                    {
                        var payload = _buf.ToArray();
                        var ok = Crc16.Compute(payload) == (ushort)((_crcHi << 8) | b);
                        frames.Add(new(FrameKind.Message, payload, ok));
                    }
                    _s = S.Idle;
                    break;
            }
        }
        return frames;
    }

    private void Reset() { _buf.Clear(); _debugText = false; }
}

/// <summary>AES layer used when ConnectMethod = 51 (NetAES). Zero padding, ECB, no IV.</summary>
public static class ElkCrypto
{
    /// <summary>Session key: AES-ECB encrypt the ASCII passphrase (zero padded to 16-byte blocks) using the ASCII MAC
    /// address (zero padded to 16 bytes) as key. 1..16 chars gives a 16-byte (AES-128) key, 17..32 gives 32 bytes (AES-256)
    /// because ElkRP passes the derived array straight to CreateEncryptor. [Inferred for the 17..32 case, not tested on a panel.]</summary>
    public static byte[] DeriveKey(string macAddress, string passphrase)
    {
        var k = new byte[16];
        var mac = Encoding.ASCII.GetBytes(macAddress);
        Array.Copy(mac, k, Math.Min(mac.Length, 16));
        var pass = Encoding.ASCII.GetBytes(passphrase);
        var padded = new byte[(pass.Length + 15) / 16 * 16];
        Array.Copy(pass, padded, pass.Length);
        using var aes = Aes.Create();
        aes.Key = k;
        return aes.EncryptEcb(padded, PaddingMode.None);
    }

    public static byte[] Encrypt(byte[] key, ReadOnlySpan<byte> framed)
    {
        var padded = new byte[(framed.Length + 15) / 16 * 16];
        framed.CopyTo(padded);
        using var aes = Aes.Create();
        aes.Key = key;
        return aes.EncryptEcb(padded, PaddingMode.None);
    }

    public static byte[] Decrypt(byte[] key, ReadOnlySpan<byte> cipher)
    {
        using var aes = Aes.Create();
        aes.Key = key;
        return aes.DecryptEcb(cipher[..(cipher.Length / 16 * 16)], PaddingMode.None);
    }
}
