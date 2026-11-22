using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

public class AccountExportTests
{
    // header junk, then a zone block of 2 stored zones (numbers 1 and 161), then trailing group values and junk
    private static string Sample()
    {
        var l = new List<string> { "\"Acct\",\"x\"", "1", "7" };
        foreach (var zn in new[] { 1, 161 })
        {
            var z = new ZoneRecord { Function = 3, Flag1 = 17, Name = "Old " + zn }; z.Bytes[3] = 9;
            l.Add(zn.ToString()); l.AddRange(z.Bytes.Select(b => b.ToString()));
        }
        l.Insert(2, "2"); l.RemoveAt(3); // count line "2" replaces the stray "7"
        l.AddRange(new[] { "0", "0", "\"tail\"" });
        return string.Join("\r\n", l) + "\r\n";
    }

    [Fact]
    public void Updates_Existing_And_Adds_New_In_Order_Keeping_Crlf()
    {
        var specs = new[] { new ZoneSpec(1, "Front Door", 1, 0, 2, ZoneFlag1.Bypassable, false, true), new ZoneSpec(40, "Kitchen", 2, 3, 1, 0, false, false) };
        var (text, log) = AccountExport.ApplyZones(Sample(), specs);
        Assert.Contains("\r\n", text);
        var lines = text.Split("\r\n").ToList();
        var blk = AccountExport.FindZoneBlock(lines)!;
        Assert.Equal(3, blk.Count);
        Assert.Equal("1", lines[blk.CountLine + 1]);
        Assert.Equal("40", lines[blk.CountLine + 1 + 33]);
        Assert.Equal("161", lines[blk.CountLine + 1 + 66]);
        var z1 = new ZoneRecord(); for (int b = 0; b < 32; b++) z1.Bytes[b] = byte.Parse(lines[blk.CountLine + 2 + b]);
        Assert.Equal("Front Door", z1.Name); Assert.Equal(9, z1.Bytes[3]);            // unused byte preserved
        Assert.Equal(1, z1.Function); Assert.Equal(1, z1.Flag1);
        Assert.Equal(2, ZoneFlag2.Area(z1.Flag2)); Assert.NotEqual(0, z1.Flag2 & ZoneFlag2.SilentAlarm);
        Assert.Equal("\"tail\"", lines[^2]);                                            // data after the block untouched
    }

    [Fact]
    public void Missing_Block_Is_Reported() => Assert.Throws<InvalidDataException>(() => AccountExport.ApplyZones("\"a\"\r\n5\r\n", new[] { new ZoneSpec(1, "x", 1, 0, 1, 0, false, false) }));
}
