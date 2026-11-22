using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

public class ProtocolTests
{
    // Zone voice words as stored for TESTACCT zones 1 and 2: 1H=1, 1L=215, 2L=22 / 23.
    private static void Voice(ZoneRecord z, params (int idx, byte val)[] v) { foreach (var (i, b) in v) z.Bytes[20 + i] = b; }

    [Fact]
    public void Zone1_Default_Crc_Matches_Stored_UDCRC()
    {
        var z = new ZoneRecord { Function = 1, Flag1 = 17, Name = "Zone 001" };
        Voice(z, (0, 1), (1, 215), (3, 22));
        Assert.Equal(2259, z.Crc);
    }

    [Fact]
    public void Zone1_Edited_Crc_Matches_Stored_ModCRC()
    {
        var z = new ZoneRecord { Function = 2, Flag1 = 17, Name = "ZONE-ONE-CHANGED" };
        Voice(z, (0, 1), (1, 215), (3, 22));
        Assert.Equal(3116, z.Crc);
    }

    [Fact]
    public void Zone2_Default_Crc_Matches_Stored_UDCRC()
    {
        var z = new ZoneRecord { Function = 1, Flag1 = 17, Name = "Zone 002" };
        Voice(z, (0, 1), (1, 215), (3, 23));
        Assert.Equal(36299, z.Crc);
    }

    [Fact]
    public void Frame_RoundTrip_With_Escaped_Dle()
    {
        var payload = new byte[] { 5, 0x10, 0, 0x10, 0x10, 9 };
        var frames = new FrameParser().Feed(Framing.Encode(payload)).ToList();
        var f = Assert.Single(frames);
        Assert.True(f.CrcOk);
        Assert.Equal(payload, f.Payload);
    }

    [Fact]
    public void Corrupt_Crc_Is_Flagged()
    {
        var enc = Framing.Encode(new byte[] { 1, 2, 3 });
        enc[^1] ^= 0xFF;
        Assert.False(new FrameParser().Feed(enc).Single().CrcOk);
    }

    [Fact]
    public void Ack_And_Nak_Are_Recognised()
    {
        var kinds = new FrameParser().Feed(new byte[] { 0x10, 0x06, 0x10, 0x15 }).Select(f => f.Kind).ToArray();
        Assert.Equal(new[] { FrameKind.Ack, FrameKind.Nak }, kinds);
    }

    [Fact]
    public void Encrypt_Decrypt_RoundTrip_With_Short_And_Long_Passphrase()
    {
        foreach (var pass in new[] { "short", "PROBEPASSPHRASE0123456789ABCDEFG" })
        {
            var key = ElkCrypto.DeriveKey("0A1B2C3D4E5F", pass);
            Assert.Equal(pass.Length <= 16 ? 16 : 32, key.Length);
            var framed = Framing.Encode(new byte[] { 5, 0, 0, 7 });
            var dec = ElkCrypto.Decrypt(key, ElkCrypto.Encrypt(key, framed));
            Assert.Equal(framed, dec[..framed.Length]);
        }
    }
}

public class AreaTests
{
    private static AreaRecord A(string n, byte e1, byte n1, byte e2, byte n2, byte f1, byte f2) =>
        new() { Name = n, Exit1 = e1, Entrance1 = n1, Exit2 = e2, Entrance2 = n2, Flags1 = f1, Flags2 = f2, ArmTimeWindow = 15 };

    [Theory]
    [InlineData("AREA-ONE-PROBE", 60, 30, 60, 30, 97, 80, 24532)]
    [InlineData("Alpha2", 71, 41, 82, 23, 54, 168, 33085)]
    [InlineData("Beta3", 71, 41, 82, 23, 54, 168, 22889)]
    [InlineData("AREA-EIGHT-PROBE", 60, 30, 60, 30, 97, 80, 18040)]
    public void Area_Crc_Matches_Stored_ModCRC(string n, byte e1, byte n1, byte e2, byte n2, byte f1, byte f2, int expected)
        => Assert.Equal(expected, A(n, e1, n1, e2, n2, f1, f2).Crc);

    [Fact]
    public void Inverted_Option_Flags_Are_Complements_On_Defined_Bits()
    {
        Assert.Equal(0x57, 97 ^ 54);   // bits 0,1,2,4,6 flipped; bit 5 stays set in both
        Assert.Equal(0xF8, 80 ^ 168);  // bits 3..7 flipped
    }
}

public class ZoneFlagTests
{
    [Fact] public void Bypassable_Off_Is_17_To_16() => Assert.Equal(16, 17 & ~(int)ZoneFlag1.Bypassable);
    [Fact] public void Chime_Off_Is_16_To_0() => Assert.Equal(0, 16 & ~(int)ZoneFlag1.Chime);
    [Fact] public void All_Attributes_Is_255() => Assert.Equal(255, (int)(ZoneFlag1)255);
    [Fact] public void Area4_Is_48() { Assert.Equal(48, ZoneFlag2.With(0, area: 4)); Assert.Equal(4, ZoneFlag2.Area(48)); }
    [Fact] public void Flag2_136_Is_Type0_Area1_Swinger_Silent()
    {
        Assert.Equal(0, ZoneFlag2.Type(136)); Assert.Equal(1, ZoneFlag2.Area(136));
        Assert.NotEqual(0, 136 & ZoneFlag2.SwingerShutdown); Assert.NotEqual(0, 136 & ZoneFlag2.SilentAlarm);
    }
    [Fact] public void Type5_Is_5() => Assert.Equal(5, ZoneFlag2.Type(5));
}

public class ZoneFlagSamplesTests
{
    [Theory]
    [InlineData(ZoneFlag1.UseDialerDelay, 64)]
    [InlineData(ZoneFlag1.ListenIn, 128)]
    [InlineData(ZoneFlag1.Bypassable, 1)]
    [InlineData(ZoneFlag1.PeriodicTrip, 2)]
    [InlineData(ZoneFlag1.FastResponse, 4)]
    [InlineData(ZoneFlag1.Chime, 16)]
    [InlineData(ZoneFlag1.CrossZoned, 32)]
    public void Single_Checkbox_Matches_Sample_Account(ZoneFlag1 f, int stored) => Assert.Equal(stored, (int)f);

    [Fact] public void Silent_And_Swinger_On_Area4_Match_F7_And_F12()
    {
        Assert.Equal(176, 48 | ZoneFlag2.SilentAlarm);
        Assert.Equal(56, 48 | ZoneFlag2.SwingerShutdown);
    }
}

public class ForceArmTest { [Fact] public void ForceArm_Is_Bit3_Per_F11() => Assert.Equal(8, (int)ZoneFlag1.ForceArm); }

public class KeypadTests
{
    [Fact]
    public void Default_Keypad_Crcs_Match_Stored_Values()
    {
        var k = new KeypadRecord { Area = 1, Name = "Keypad 01       " };
        k.Bytes[1] = 56; k.Bytes[4] = 20; k.Bytes[5] = 96;
        (int h, int l)[] defs = { (0, 10), (0, 23), (0, 22) }, lite = { (3, 233), (3, 237), (3, 236) };
        for (int i = 0; i < 3; i++) { k.Bytes[28 + 2 * i] = (byte)defs[i].h; k.Bytes[29 + 2 * i] = (byte)defs[i].l; k.Bytes[40 + 2 * i] = (byte)lite[i].h; k.Bytes[41 + 2 * i] = (byte)lite[i].l; }
        string[] names = { "F1 = Fire       ", "F2 = Police     ", "F3 = Medical    ", "F4 = Not Defined", "F5 = Not Defined", "F6 = Not Defined" };
        for (int i = 0; i < 6; i++) System.Text.Encoding.ASCII.GetBytes(names[i]).CopyTo(k.Bytes, 52 + 16 * i);
        Assert.Equal(55021, k.Crc1);
        Assert.Equal(14106, k.Crc2);
        Assert.Equal("F2 = Police", k.FKeyName(1));
        Assert.Equal(23, k.FKeyDefinition(1));
    }
}
