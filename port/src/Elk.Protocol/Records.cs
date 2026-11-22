using System.Text;

namespace Elk.Protocol;

/// <summary>Zone record as stored and sent: 32 bytes. Layout from Zone_Definitions.cs (ZNBytesIDs) and
/// verified against the stored CRC values of three zones.
/// 0 function | 1 flag1 | 2 flag2 | 3 unused | 4..19 name (ASCII, NUL or space padded) | 20..31 six voice words (H,L).</summary>
public sealed class ZoneRecord
{
    public const int Size = 32;
    public byte[] Bytes { get; } = new byte[Size];

    public byte Function { get => Bytes[0]; set => Bytes[0] = value; }
    public byte Flag1 { get => Bytes[1]; set => Bytes[1] = value; }
    public byte Flag2 { get => Bytes[2]; set => Bytes[2] = value; }

    public string Name
    {
        get => Encoding.ASCII.GetString(Bytes, 4, 16).TrimEnd('\0', ' ');
        set => NameField.Put(Bytes, 4, value);
    }

    public ushort[] Voice
    {
        get { var v = new ushort[6]; for (int i = 0; i < 6; i++) v[i] = (ushort)((Bytes[20 + 2 * i] << 8) | Bytes[21 + 2 * i]); return v; }
    }

    /// <summary>The "modified" CRC ElkRP keeps (ModCRC): CRC-16 over all 32 bytes (software 4.0.0 and later; earlier DB versions used 20 bytes).</summary>
    public ushort Crc => Crc16.Compute(Bytes);

    public static ZoneRecord FromBytes(ReadOnlySpan<byte> data)
    {
        var z = new ZoneRecord();
        data[..Math.Min(Size, data.Length)].CopyTo(z.Bytes);
        return z;
    }
}

/// <summary>Zone flag1 bits. UI labels in brackets. All eight bits verified by toggling one checkbox in ElkRP
/// on a sample account (F10, F13, F14, F11, F15, F16, F8, F9).</summary>
[Flags]
public enum ZoneFlag1 : byte
{
    Bypassable = 1,          // [Bypassable]
    PeriodicTrip = 2,        // [Periodic trip] (ElkRP code calls it ActivityMon)
    FastResponse = 4,        // [Fast loop response]
    ForceArm = 8,            // [Force armable]
    Chime = 16,              // [Enable chime]
    CrossZoned = 32,         // [In cross zone pool]
    UseDialerDelay = 64,     // [Use dialer delay] (ElkRP code calls it Abort)
    ListenIn = 128           // [Listen in]
}

/// <summary>Zone flag2: bits 0-2 zone type (hook-up), bit 3 swinger shutdown, bits 4-6 area stored as (area - 1), bit 7 silent alarm.
/// Verified against the sample accounts: type 0/2/5, area 1 (0) and area 4 (48 = 3 &lt;&lt; 4), and the 136 pattern (swinger + silent).</summary>
public static class ZoneFlag2
{
    public const byte TypeMask = 0x07, SwingerShutdown = 0x08, AreaMask = 0x70, AreaShift = 4, SilentAlarm = 0x80;

    public static int Type(byte flag2) => flag2 & TypeMask;
    public static int Area(byte flag2) => ((flag2 & AreaMask) >> AreaShift) + 1;
    public static byte With(byte flag2, int? type = null, int? area = null)
    {
        if (type is int t) flag2 = (byte)((flag2 & ~TypeMask) | (t & TypeMask));
        if (area is int a) flag2 = (byte)((flag2 & ~AreaMask) | (((a - 1) << AreaShift) & AreaMask));
        return flag2;
    }
}

/// <summary>Area record, 24 bytes. Verified by CRC against five stored areas.
/// 0..15 name (NUL padded when typed, space padded in defaults) | 16 exit1 | 17 entrance1 | 18 exit2 | 19 entrance2 |
/// 20 flags1 | 21 flags2 | 22 arm time window | 23 unused.
/// flags1: bit0 quick arm, bit1/2 closing range-back, bit4 auto interior off, bit6 stay key scroll (bits 3,5,7 unused, bit5 stays set).
/// flags2: bit3 visible mode night instant, bit4 night, bit5 stay instant, bit6 stay, bit7 double-press arm.</summary>
public sealed class AreaRecord
{
    public const int Size = 24;
    public byte[] Bytes { get; } = new byte[Size];

    public string Name
    {
        get => System.Text.Encoding.ASCII.GetString(Bytes, 0, 16).TrimEnd('\0', ' ');
        set => NameField.Put(Bytes, 0, value);
    }
    public byte Exit1 { get => Bytes[16]; set => Bytes[16] = value; }
    public byte Entrance1 { get => Bytes[17]; set => Bytes[17] = value; }
    public byte Exit2 { get => Bytes[18]; set => Bytes[18] = value; }
    public byte Entrance2 { get => Bytes[19]; set => Bytes[19] = value; }
    public byte Flags1 { get => Bytes[20]; set => Bytes[20] = value; }
    public byte Flags2 { get => Bytes[21]; set => Bytes[21] = value; }
    public byte ArmTimeWindow { get => Bytes[22]; set => Bytes[22] = value; }
    public ushort Crc => Crc16.Compute(Bytes);
}

/// <summary>Keypad record, 148 bytes (Keypad_Definitions.cs KPBytesIDs). Two stored CRCs: Crc1 over bytes 0..27, Crc2 over 28..147
/// (split verified against the stored CRCs of sample keypad 1).
/// 0 area | 1..5 flags1-5 | 6..21 name | 22 LED state | 23..24 flags6-7 | 25..27 unused | 28..39 six F-key definitions (H,L) |
/// 40..51 six F-key light definitions (H,L) | 52..147 six F-key names, 16 chars each.</summary>
public sealed class KeypadRecord
{
    public const int Size = 148;
    public byte[] Bytes { get; } = new byte[Size];
    public byte Area { get => Bytes[0]; set => Bytes[0] = value; }
    public string Name
    {
        get => System.Text.Encoding.ASCII.GetString(Bytes, 6, 16).TrimEnd('\0', ' ');
        set => NameField.Put(Bytes, 6, value);
    }
    public ushort FKeyDefinition(int f) => (ushort)((Bytes[28 + 2 * f] << 8) | Bytes[29 + 2 * f]);   // f = 0..5
    public string FKeyName(int f) => System.Text.Encoding.ASCII.GetString(Bytes, 52 + 16 * f, 16).TrimEnd('\0', ' ');
    public ushort Crc1 => Crc16.Compute(Bytes.AsSpan(0, 28));
    public ushort Crc2 => Crc16.Compute(Bytes.AsSpan(28));
}

/// <summary>16-byte name fields. A panel pads them with spaces or NULs; rewriting an unchanged name must not alter the bytes (that changes the record CRC).</summary>
internal static class NameField
{
    public static void Put(byte[] bytes, int offset, string value)
    {
        string text = value.Length > 16 ? value[..16] : value;
        if (System.Text.Encoding.ASCII.GetString(bytes, offset, 16).TrimEnd('\0', ' ') == text) return;   // unchanged: keep the existing padding
        byte pad = bytes[offset + 15] == (byte)' ' ? (byte)' ' : (byte)0;                                // keep the field's existing padding style
        Array.Fill(bytes, pad, offset, 16);
        var b = System.Text.Encoding.ASCII.GetBytes(text);
        Array.Copy(b, 0, bytes, offset, b.Length);
    }
}
