using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

public class ZoneImportTests
{
    [Theory]
    [InlineData(1, new ushort[] { 471, 22 })]
    [InlineData(2, new ushort[] { 471, 23 })]
    [InlineData(3, new ushort[] { 471, 24 })]
    [InlineData(12, new ushort[] { 471, 33 })]
    [InlineData(20, new ushort[] { 471, 41 })]
    [InlineData(21, new ushort[] { 471, 41, 22 })]
    [InlineData(40, new ushort[] { 471, 43 })]
    [InlineData(100, new ushort[] { 471, 22, 49 })]
    [InlineData(105, new ushort[] { 471, 22, 49, 26 })]
    [InlineData(110, new ushort[] { 471, 22, 31 })]
    [InlineData(186, new ushort[] { 471, 22, 47, 27 })]   // the screenshot: Zone, One, Eighty, Six
    [InlineData(200, new ushort[] { 471, 23, 49 })]
    [InlineData(208, new ushort[] { 471, 23, 49, 29 })]
    public void Default_Voice_Words(int zone, ushort[] prefix)
    {
        var w = ZoneVoice.Words(zone);
        Assert.Equal(6, w.Length);
        Assert.Equal(prefix, w[..prefix.Length]);
        Assert.All(w[prefix.Length..], x => Assert.Equal(0, x));
    }

    [Fact]
    public void Record_Voice_Matches_Stored_Zone1_Bytes()
    {
        var z = new ZoneRecord { Function = 1, Flag1 = 17, Name = "Zone 001" };
        ZoneVoice.Apply(z, 1);
        Assert.Equal(2259, z.Crc);     // stored UDCRC of sample zone 1 (voice words 471, 22)
    }

    [Fact]
    public void Csv_Builds_Record_With_Flags_Type_Area_And_Voice()
    {
        var (zones, errors) = ZoneCsv.Parse("Zone,Name,Definition,Type,Area,Attributes\n186,Back Door,1,5,4,\"Bypassable|Enable Chime|Silent alarm\"\n");
        Assert.Empty(errors);
        var z = Assert.Single(zones).ToRecord();
        Assert.Equal("Back Door", z.Name); Assert.Equal(1, z.Function);
        Assert.Equal((byte)(ZoneFlag1.Bypassable | ZoneFlag1.Chime), z.Flag1);
        Assert.Equal(5, ZoneFlag2.Type(z.Flag2)); Assert.Equal(4, ZoneFlag2.Area(z.Flag2));
        Assert.NotEqual(0, z.Flag2 & ZoneFlag2.SilentAlarm); Assert.Equal(0, z.Flag2 & ZoneFlag2.SwingerShutdown);
        Assert.Equal(new ushort[] { 471, 22, 47, 27, 0, 0 }, z.Voice);
    }

    [Fact]
    public void Csv_Reports_Bad_Rows_With_Line_Numbers()
    {
        var (zones, errors) = ZoneCsv.Parse("Zone,Name,Definition,Type,Area,Attributes\n1,A,1,0,1,\n1,B,1,0,1,\n2,TOO-LONG-NAME-OVER-16,1,0,1,\n3,C,1,9,1,\n4,D,1,0,1,Nonsense\n209,E,1,0,1,\n");
        Assert.Single(zones);
        Assert.Equal(5, errors.Count);
        Assert.Contains(errors, e => e.StartsWith("line 3") && e.Contains("twice"));
    }
}
