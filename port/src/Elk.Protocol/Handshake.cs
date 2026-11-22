using System.Security.Cryptography;

namespace Elk.Protocol;

/// <summary>Section ids. A section is read with the id, written with id + 128, and its per-item CRC is queried with 0x7E.
/// Verified from the decompiled Send/Receive code for zones, areas and keypads; the others are from opcode tallies.</summary>
public enum Section : byte
{
    UserCodes = 1, Area = 3, Keypad = 4, Zone = 5, Globals = 7, Telephone = 8, VoiceMessage = 13, X10 = 14, WirelessZone = 15
}

public enum ConnectMode { Aes = 51, Secure = 49, Tls = 50 /* 49 and 50 append the encrypted identity block; 50 is the M1XEP secure port (TLS) */ }

public static class Handshake
{
    public const byte SessionOpcode = 0x7F, CrcQueryOpcode = 0x7E;

    /// <summary>Connect request. `7F 00 00 01` then the six RP access-code digits, LAST DIGIT FIRST, each as a value 0-9
    /// (ElkRP loops num9 = 6 down to 1). For modes 49/50 a 16-byte block is appended (see <see cref="IdentityBlock"/>).</summary>
    public static byte[] BuildConnect(string accessCode, ConnectMode mode = ConnectMode.Aes, string? serialHex = null, Random? rng = null)
    {
        if (accessCode.Length != 6 || !accessCode.All(char.IsAsciiDigit)) throw new ArgumentException("RP access code must be 6 digits.");
        var p = new List<byte> { SessionOpcode, 0, 0, 1 };
        for (int i = 5; i >= 0; i--) p.Add((byte)(accessCode[i] - '0'));
        if (mode != ConnectMode.Aes) p.AddRange(IdentityBlock(accessCode, serialHex ?? throw new ArgumentException("serial required"), rng ?? Random.Shared));
        return p.ToArray();
    }

    /// <summary>16 bytes: [0] random 1..255 | [1..8] serial number as 8 hex nibbles | [9] random 1..255 | [10..15] access code digits (forward order),
    /// AES-128-CBC with zero IV and the fixed key embedded in ElkRP (M1Fns.AES128_Encrypt_B).</summary>
    public static byte[] IdentityBlock(string accessCode, string serialHex, Random rng)
    {
        if (serialHex.Length != 8) throw new ArgumentException("serial must be 8 hex digits");
        var a = new byte[16];
        a[0] = (byte)rng.Next(1, 256);
        for (int i = 0; i < 8; i++) a[1 + i] = Convert.ToByte(serialHex[i].ToString(), 16);
        a[9] = (byte)rng.Next(1, 256);
        for (int i = 0; i < 6; i++) a[10 + i] = (byte)(accessCode[i] - '0');
        using var aes = Aes.Create();
        aes.Key = FixedKey;
        return aes.EncryptCbc(a, new byte[16], PaddingMode.None);
    }

    public static readonly byte[] FixedKey = { 5, 24, 65, 5, 25, 59, 9, 25, 84, 3, 10, 94, 6, 7, 97, 100 };

    public static byte[] KeepAlive => new byte[] { SessionOpcode, 0, 0, 0xA5 };
    public static byte[] Disconnect => new byte[] { SessionOpcode, 1, 0, 0xA5 };
    /// <summary>`part` is request byte 1; only keypads use it (0 = bytes 0..27, 1 = bytes 28..147).</summary>
    public static byte[] ReadRequest(Section s, byte item, byte part = 0) => new byte[] { (byte)s, part, 0, item };
    public static byte[] CrcQuery(Section s, byte item) => new byte[] { CrcQueryOpcode, (byte)s, 0, item };

    public static byte[] WriteRequest(Section s, byte item, ReadOnlySpan<byte> data, byte part = 0)
    {
        var p = new byte[4 + data.Length];
        p[0] = (byte)((byte)s + 128); p[1] = part; p[3] = item;
        data.CopyTo(p.AsSpan(4));
        return p;
    }
}

/// <summary>Panel reply to the connect request (opcode 0x7F). Offsets from DoConnectProtocol:
/// [1] status (0 = accepted) | [4..6] firmware major.minor.patch (when [4] >= 4; below that the panel is in its boot loader)
/// | [8..11] serial number as 8 hex digits | [12..13] hardware version | [14..16] boot version | [22..24] minimum RP version required.</summary>
public sealed record ConnectReply(bool Accepted, bool InBootLoader, string Firmware, string SerialHex, string Hardware, string Boot, string? MinRpVersion)
{
    public static ConnectReply Parse(byte[] p)
    {
        if (p.Length < 16 || p[0] != Handshake.SessionOpcode) throw new InvalidDataException("Not a connect reply.");
        bool ok = p[1] == 0;
        bool boot = p[4] < 4;
        var serial = string.Concat(p[8..12].Select(b => b.ToString("X2")));
        return new ConnectReply(ok, boot, boot ? "" : $"{p[4]}.{p[5]}.{p[6]}", serial, $"{p[12]}.{p[13]}", $"{p[14]}.{p[15]}.{p[16]}",
            p.Length >= 25 && p[22] > 0 && p[23] > 0 && p[24] > 1 ? $"{p[22]}.{p[23]}.{p[24]}" : null);
    }
}
