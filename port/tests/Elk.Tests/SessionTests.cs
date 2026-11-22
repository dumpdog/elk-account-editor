using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

public class HandshakeTests
{
    [Fact]
    public void Connect_Request_Has_Digits_Last_First()
        => Assert.Equal(new byte[] { 0x7F, 0, 0, 1, 1, 0, 8, 6, 4, 2 }, Handshake.BuildConnect("246801"));

    [Fact]
    public void Identity_Block_Is_16_Bytes_And_Decrypts_To_Layout()
    {
        var blk = Handshake.IdentityBlock("246801", "31415926", new Random(1));
        Assert.Equal(16, blk.Length);
        using var aes = System.Security.Cryptography.Aes.Create();
        aes.Key = Handshake.FixedKey;
        var plain = aes.DecryptCbc(blk, new byte[16], System.Security.Cryptography.PaddingMode.None);
        Assert.Equal(new byte[] { 3, 1, 4, 1, 5, 9, 2, 6 }, plain[1..9]);
        Assert.Equal(new byte[] { 2, 4, 6, 8, 0, 1 }, plain[10..16]);
        Assert.NotEqual(0, plain[0]); Assert.NotEqual(0, plain[9]);
    }

    [Fact]
    public void Control_Messages_Match_Traced_Bytes()
    {
        Assert.Equal(new byte[] { 0x7F, 0, 0, 0xA5 }, Handshake.KeepAlive);
        Assert.Equal(new byte[] { 0x7F, 1, 0, 0xA5 }, Handshake.Disconnect);
        Assert.Equal(new byte[] { 5, 0, 0, 7 }, Handshake.ReadRequest(Section.Zone, 7));
        Assert.Equal(new byte[] { 0x7E, 3, 0, 2 }, Handshake.CrcQuery(Section.Area, 2));
        Assert.Equal(new byte[] { 133, 0, 0, 1, 9 }, Handshake.WriteRequest(Section.Zone, 1, new byte[] { 9 }));
    }

    [Fact]
    public void Connect_Reply_Parses_Versions_And_Serial()
    {
        var p = new byte[25]; p[0] = 0x7F; p[4] = 5; p[5] = 2; p[6] = 9; p[8] = 0x31; p[9] = 0x41; p[10] = 0x59; p[11] = 0x26;
        p[12] = 1; p[13] = 2; p[14] = 3; p[15] = 4; p[16] = 5; p[22] = 5; p[23] = 1; p[24] = 2;
        var r = ConnectReply.Parse(p);
        Assert.True(r.Accepted); Assert.False(r.InBootLoader);
        Assert.Equal("5.2.9", r.Firmware); Assert.Equal("31415926", r.SerialHex); Assert.Equal("1.2", r.Hardware);
        Assert.Equal("3.4.5", r.Boot); Assert.Equal("5.1.2", r.MinRpVersion);
    }
}

public class SessionTests
{
    private const string Mac = "0A1B2C3D4E5F", Pass = "PROBEPASSPHRASE0123456789ABCDEFG";

    private static (SimulatedPanel panel, ElkSession s) Start(bool split = false)
    {
        var panel = new SimulatedPanel(Mac, Pass, "246801") { SplitWrites = split };
        var z = new ZoneRecord { Function = 2, Flag1 = 17, Name = "ZONE-ONE-CHANGED" };
        z.Bytes[20] = 1; z.Bytes[21] = 215; z.Bytes[23] = 22;   // voice words as stored for sample zone 1
        panel.Records[(Section.Zone, 1)] = z.Bytes;
        panel.Records[(Section.Area, 2)] = new AreaRecord { Name = "Alpha2", Exit1 = 71, Entrance1 = 41, Exit2 = 82, Entrance2 = 23, Flags1 = 54, Flags2 = 168, ArmTimeWindow = 15 }.Bytes;
        return (panel, new ElkSession(Mac, Pass));
    }

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public async Task Login_Read_Write_Crc_Roundtrip(bool split)
    {
        var (panel, s) = Start(split);
        using (panel) using (s)
        {
            await s.ConnectAsync("127.0.0.1", panel.Port);
            var info = await s.LoginAsync("246801");
            Assert.Equal("5.2.9", info.Firmware);

            var zone = await s.ReadZoneAsync(1);
            Assert.Equal("ZONE-ONE-CHANGED", zone.Name);
            Assert.Equal(3116, zone.Crc);

            var area = await s.ReadAreaAsync(2);
            Assert.Equal("Alpha2", area.Name); Assert.Equal(33085, area.Crc);

            zone.Name = "RENAMED"; zone.Flag1 = (byte)ZoneFlag1.Chime;
            await s.WriteRecordAsync(Section.Zone, 1, zone.Bytes);
            Assert.Equal("RENAMED", (await s.ReadZoneAsync(1)).Name);
            Assert.Equal(zone.Crc, await s.QueryCrcAsync(Section.Zone, 1));

            await s.KeepAliveAsync();
            await s.DisconnectAsync();
        }
    }

    [Fact]
    public async Task Wrong_Access_Code_Is_Rejected()
    {
        var (panel, s) = Start();
        using (panel) using (s)
        {
            await s.ConnectAsync("127.0.0.1", panel.Port);
            await Assert.ThrowsAsync<UnauthorizedAccessException>(() => s.LoginAsync("000000"));
        }
    }

    [Fact]
    public async Task Wrong_Passphrase_Never_Gets_A_Reply()
    {
        var panel = new SimulatedPanel(Mac, Pass, "246801");
        using var s = new ElkSession(Mac, "different passphrase") { Timeout = TimeSpan.FromMilliseconds(400) };
        using (panel)
        {
            await s.ConnectAsync("127.0.0.1", panel.Port);
            await Assert.ThrowsAnyAsync<Exception>(() => s.LoginAsync("246801"));
        }
    }
}

public class KeypadSessionTests
{
    [Fact]
    public async Task Keypad_Two_Part_Read_And_Write()
    {
        using var panel = new SimulatedPanel("0A1B2C3D4E5F", "pw", "246801");
        var k = new KeypadRecord { Area = 8, Name = "KEYPAD-LAST-PRB" };
        k.Bytes[52] = (byte)'F';
        panel.Records[(Section.Keypad, 16)] = k.Bytes;
        using var s = new ElkSession("0A1B2C3D4E5F", "pw");
        await s.ConnectAsync("127.0.0.1", panel.Port);
        await s.LoginAsync("246801");
        var got = await s.ReadKeypadAsync(16);
        Assert.Equal(k.Bytes, got.Bytes);
        got.Name = "CHANGED"; got.Bytes[147] = 0x5A;
        await s.WriteKeypadAsync(16, got);
        var again = await s.ReadKeypadAsync(16);
        Assert.Equal("CHANGED", again.Name); Assert.Equal(0x5A, again.Bytes[147]);
        Assert.Equal(got.Crc1, again.Crc1); Assert.Equal(got.Crc2, again.Crc2);
    }
}
