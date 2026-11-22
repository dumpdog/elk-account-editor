using System.Text;

namespace Elk.Protocol;

/// <summary>What an unused item looks like on a factory-default M1G, read from an untouched panel (2026-11-09, firmware 5.3.44).
/// Used to reset panel records that an account does not contain.</summary>
public static class DefaultRecords
{
    private static void Name(byte[] b, int at, string s, byte pad = 0) { Array.Fill(b, pad, at, 16); Encoding.ASCII.GetBytes(s).CopyTo(b, at); }

    public static byte[] Zone(int n)
    {
        var z = new ZoneRecord { Function = 0, Flag1 = 1, Flag2 = 0 };
        z.Name = $"Zone {n:D3}"; ZoneVoice.Apply(z, n);
        return z.Bytes;
    }
    public static byte[] User(int n) { var b = new byte[28]; b[6] = 0xE2; b[8] = 1; Name(b, 9, $"USER {n:D3}"); return b; }
    public static byte[] Area(int n)
    {
        var b = new byte[24]; Name(b, 0, $"Area {n}", (byte)' ');
        b[16] = 60; b[17] = 30; b[18] = 60; b[19] = 30; b[20] = 0x61; b[21] = 0x50; b[22] = 15;
        return b;
    }
    public static byte[] Keypad() { var b = new byte[148]; b[4] = 0x33; b[5] = 0x60; return b; }
    public static byte[] Phone() { var b = new byte[88]; b[20] = 0xA8; b[22] = 8; for (int i = 23; i < 71; i++) b[i] = (byte)'0'; return b; }
    public static byte[] AreaRc() => new byte[20];
    public static byte[] UserRc() => new byte[] { 1, 0, 0, 1 };
    public static byte[] ZoneRc() => new byte[4];
}
